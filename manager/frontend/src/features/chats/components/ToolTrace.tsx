import { useState } from 'react';
import {
  type Answer,
  PREVIEW_LABEL,
  REASON_LABEL,
  REASON_MISSING,
  REASONED_TOOLS,
  type ToolCallRecord,
} from '../types';
import { renderAnswers } from '../utils/answers';
import { JobCard } from './JobCard';
import { QuestionCard } from './QuestionCard';
import { Reasoning } from './Reasoning';

/// Tool names are written for the model, so the trace gives the reader a plain
/// description instead. An unknown name is spelled out from its identifier
/// rather than hidden, so a newly added tool still shows up as words.
const TOOL_LABELS: Record<string, string> = {
  load_tools: 'Loaded tools',
  search_tools: 'Searched for tools',
  fetch_url: 'Fetched a web page',
  web_search: 'Searched the web',
  search_knowledge: 'Searched the knowledge base',
  search_chat_history: 'Searched earlier messages',
  list_sources: 'Listed connected sources',
  list_projects: 'Listed projects',
  list_tasks: 'Listed tasks',
  list_chats: 'Listed chats',
  list_members: 'Listed members',
  list_files: 'Listed files',
  read_file: 'Read a file',
  list_documents: 'Listed workspace documents',
  read_document: 'Read a workspace document',
  read_chat_evidence: 'Read an earlier chat',
  get_task_run: 'Checked a task run',
  get_build_status: 'Checked GitHub build status',
  read_check_logs: 'Read GitHub check logs',
  list_deployments: 'Listed GitHub deployments',
  list_issues: 'Listed GitHub issues',
  get_issue: 'Read a GitHub issue',
  read_repository_file: 'Read a repository file',
  create_task: 'Created a task',
  update_task: 'Updated a task',
  create_document: 'Created a document',
  update_document: 'Updated a document',
  send_message: 'Sent a message',
  create_reminder: 'Created a reminder',
  cancel_reminder: 'Cancelled a reminder',
  list_reminders: 'Listed reminders',
  memory_write: 'Wrote memory',
  memory_append: 'Appended to memory',
  memory_delete: 'Forgot memory',
  memory_read: 'Read memory',
  memory_list: 'Listed memories',
  finalize_project: 'Created a project',
  generate_image: 'Generated an image',
  edit_image: 'Edited an image',
  query_prometheus: 'Queried Prometheus',
  list_grafana_dashboards: 'Listed Grafana dashboards',
  create_pull_request: 'Opened a pull request',
  comment_on_issue: 'Commented on GitHub',
  apply_patch: 'Patched a file',
  write_file: 'Wrote a file',
  run_shell: 'Ran a shell command',
  run_command: 'Ran a command',
  ask_user: 'Asked you a question',
  tail_job: 'Read a job log',
  wait_for: 'Waited for something to finish',
};

const ACRONYMS = new Set(['url', 'id', 'api', 'pr', 'ci', 'sha', 'http', 'json', 'html']);

function humanise(name: string): string {
  const words = name
    .split(/_+/)
    .filter(Boolean)
    .map((word) => (ACRONYMS.has(word) ? word.toUpperCase() : word))
    .join(' ');
  return words.charAt(0).toUpperCase() + words.slice(1);
}

function toolLabel(name: string): string {
  return TOOL_LABELS[name] ?? humanise(name);
}

type Arguments = Record<string, unknown>;

function parseArguments(raw: string): Arguments | null {
  try {
    const parsed: unknown = JSON.parse(raw);
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed)
      ? (parsed as Arguments)
      : null;
  } catch {
    return null;
  }
}

function isString(value: unknown): value is string {
  return typeof value === 'string';
}

function text(value: unknown): string | null {
  return isString(value) && value.trim() ? value.trim() : null;
}

function reference(value: unknown): string | null {
  return typeof value === 'number' ? `#${value}` : null;
}

function join(parts: (string | null)[], separator: string): string | null {
  const present = parts.filter((part): part is string => part !== null);
  return present.length ? present.join(separator) : null;
}

function formatDue(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' });
}

const SOURCE_HANDLE = /\s*\[source_id: [^\]]*\]/g;
const COMMIT_SHA = /^[0-9a-f]{40}$/i;
const SHORT_SHA = 7;

function gitReference(value: unknown): string | null {
  const reference = text(value);
  return reference && COMMIT_SHA.test(reference) ? reference.slice(0, SHORT_SHA) : reference;
}

type Derive = (args: Arguments, detail: string) => string | null;

/// The detail column carries the first line of what the tool returned, and for
/// these tools that line is written for the model: a preamble saying where the
/// schemas will be or that a page is data and not instructions, or the opening
/// of a JSON record. The arguments say what the call was actually about, so the
/// reader gets those.
const DETAIL_FROM_ARGUMENTS: Partial<Record<string, Derive>> = {
  load_tools: (args) =>
    Array.isArray(args.names) ? args.names.filter(isString).map(humanise).join(', ') || null : null,
  fetch_url: (args) => text(args.url),
  web_search: (args) => (isString(args.query) ? `“${args.query}”` : null),
  list_sources: (_, detail) => detail.replace(SOURCE_HANDLE, ''),
  read_repository_file: (args) => join([text(args.path), gitReference(args.ref)], ' @ '),
  get_build_status: (args) => gitReference(args.ref),
  list_deployments: (args) => gitReference(args.ref),
  read_check_logs: (args) =>
    join(
      [typeof args.job_id === 'number' ? `job ${args.job_id}` : null, reference(args.number)],
      ' · '
    ),
  list_issues: (args) => text(args.state),
  get_issue: (args) => reference(args.number),
  comment_on_issue: (args) => reference(args.number),
  assess_pull_requests: (args) => reference(args.number) ?? text(args.state),
  assess_release_pipelines: (args) => text(args.tag),
  create_pull_request: (args) =>
    join([text(args.title), join([text(args.head), text(args.base)], ' → ')], ' · '),
  list_documents: (args) => (isString(args.query) ? `“${args.query}”` : null),
  read_document: (args) => text(args.id),
  create_document: (args) => text(args.title),
  update_document: (args) => text(args.title) ?? text(args.id),
  create_reminder: (args) =>
    join([text(args.content), isString(args.due_at) ? formatDue(args.due_at) : null], ' · '),
};

const JSON_LINE = /^\s*(?:\{\s*(?:"|\})|\[\s*(?:[{["\]]|\d))/;

/// A failed call's detail is its error and a waiting call's is its state, and
/// both are the server's words about this call rather than a preamble. A JSON
/// record nothing here can summarise is left out rather than shown as braces.
function toolDetail(call: ToolCallRecord): string {
  if (!call.success || call.pending || call.approval === 'pending') return call.detail;
  const derive = DETAIL_FROM_ARGUMENTS[call.name];
  const args = derive ? parseArguments(call.arguments) : null;
  const derived = derive && args ? derive(args, call.detail) : null;
  if (derived) return derived;
  return JSON_LINE.test(call.detail) ? '' : call.detail;
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

/// Show the arguments as the model wrote them, pretty-printed when they parse.
function formatArguments(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed || trimmed === '{}') return null;
  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch {
    return trimmed;
  }
}

/// Shown for every tool that owes a reason, and for any call that volunteered
/// one, so a tool this client has not heard of still shows what it claimed.
function StatedReason({ call }: { call: ToolCallRecord }) {
  const stated = call.reason?.trim();
  if (!stated && !REASONED_TOOLS.has(call.name)) return null;

  return (
    <p className="tool-call-reason" data-testid="tool-call-reason">
      <span className="tool-call-reason-label">{REASON_LABEL}</span>
      <span className={`tool-call-reason-text${stated ? '' : ' tool-call-reason-text--missing'}`}>
        {stated || REASON_MISSING}
      </span>
    </p>
  );
}

/// The server's account of the call, read from the arguments the model wrote
/// rather than from what it said about them. Absent means the server rendered
/// none, which is nothing to announce: an unclaimed fact, unlike an unanswered
/// obligation, leaves no gap for a reader to notice.
function ObservedPreview({ call }: { call: ToolCallRecord }) {
  const observed = call.preview?.trim();
  if (!observed) return null;

  return (
    <p className="tool-call-preview" data-testid="tool-call-preview">
      <span className="tool-call-preview-label">{PREVIEW_LABEL}</span>
      <span className="tool-call-preview-text">{observed}</span>
    </p>
  );
}

function ToolTraceRow({
  call,
  thinking,
  answered,
  live,
  onDecide,
  onAnswer,
}: {
  call: ToolCallRecord;
  thinking: boolean;
  answered: boolean;
  live: boolean;
  onDecide?: (id: string, approved: boolean) => void;
  onAnswer?: (content: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const args = formatArguments(call.arguments);
  const status =
    call.approval === 'pending'
      ? 'approval'
      : call.pending
        ? 'pending'
        : call.success
          ? 'ok'
          : 'failed';

  return (
    <li className={`tool-call tool-call--${status}`}>
      {call.reasoning?.trim() ? <Reasoning content={call.reasoning} open={thinking} /> : null}
      <button
        type="button"
        className="tool-call-summary"
        onClick={() => setExpanded((v) => !v)}
        aria-expanded={expanded}
        disabled={!args}
        data-testid="tool-call"
      >
        <span className="tool-call-status" aria-hidden="true" />
        <span className="tool-call-name">{toolLabel(call.name)}</span>
        <span className="tool-call-detail">{toolDetail(call)}</span>
        {!call.pending && call.duration_ms > 0 && (
          <span className="tool-call-duration">{formatDuration(call.duration_ms)}</span>
        )}
      </button>
      <ObservedPreview call={call} />
      <StatedReason call={call} />
      {call.questions?.length ? (
        <QuestionCard
          questions={call.questions}
          answered={answered}
          onSubmit={(answers: Answer[]) => onAnswer?.(renderAnswers(call.questions ?? [], answers))}
        />
      ) : null}
      <JobCard call={call} live={live} />
      {call.approval === 'pending' && onDecide && (
        <div className="tool-call-approval">
          <button
            type="button"
            className="tool-call-approve"
            data-testid="tool-approve"
            onClick={() => onDecide(call.id, true)}
          >
            Approve
          </button>
          <button
            type="button"
            className="tool-call-deny"
            data-testid="tool-deny"
            onClick={() => onDecide(call.id, false)}
          >
            Deny
          </button>
        </div>
      )}
      {expanded && args && <pre className="tool-call-args">{args}</pre>}
    </li>
  );
}

export function ToolTrace({
  calls,
  thinking = true,
  answered = false,
  live = false,
  onDecide,
  onAnswer,
}: {
  calls: ToolCallRecord[];
  /** Whether the thinking written before each call is shown or folded away. */
  thinking?: boolean;
  /**
   * Whether a question on this trace has already been answered. An answer is an
   * ordinary user message, so the caller decides this by looking for a user
   * message newer than the assistant message the card sits on — no server state
   * says a question is settled, and none needs to.
   */
  answered?: boolean;
  /**
   * Whether the turn these calls belong to is still being written. Background
   * work outlives no turn, so this is what separates a job that may still be
   * running from one a reload can only report as over.
   */
  live?: boolean;
  onDecide?: (id: string, approved: boolean) => void;
  onAnswer?: (content: string) => void;
}) {
  if (calls.length === 0) return null;

  return (
    <ol className="tool-trace" data-testid="tool-trace">
      {calls.map((call) => (
        <ToolTraceRow
          key={call.id}
          call={call}
          thinking={thinking}
          answered={answered}
          live={live}
          onDecide={onDecide}
          onAnswer={onAnswer}
        />
      ))}
    </ol>
  );
}
