import { useState } from 'react';
import {
  type Answer,
  APPROVAL_CANCELLED_DETAIL,
  APPROVAL_TIMED_OUT_DETAIL,
  AWAITING_APPROVAL_DETAIL,
  DENIED_DETAIL,
  PREVIEW_LABEL,
  REASON_MISSING,
  REASONED_TOOLS,
  REFUSALS,
  type Refusal,
  TOOL_ERROR_PREFIX,
  type ToolCallRecord,
  UNFINISHED_DETAIL,
} from '../types';
import { renderAnswers } from '../utils/answers';
import { JobCard } from './JobCard';
import { QuestionCard } from './QuestionCard';
import { Reasoning } from './Reasoning';

interface Verb {
  base: string;
  gerund: string;
  past: string;
}

function verb(base: string, gerund: string, past: string): Verb {
  return { base, gerund, past };
}

const LOAD = verb('load', 'loading', 'loaded');
const SEARCH = verb('search', 'searching', 'searched');
const FETCH = verb('fetch', 'fetching', 'fetched');
const LIST = verb('list', 'listing', 'listed');
const READ = verb('read', 'reading', 'read');
const CHECK = verb('check', 'checking', 'checked');
const CREATE = verb('create', 'creating', 'created');
const UPDATE = verb('update', 'updating', 'updated');
const SEND = verb('send', 'sending', 'sent');
const CANCEL = verb('cancel', 'cancelling', 'cancelled');
const WRITE = verb('write', 'writing', 'wrote');
const APPEND = verb('append', 'appending', 'appended');
const FORGET = verb('forget', 'forgetting', 'forgot');
const GENERATE = verb('generate', 'generating', 'generated');
const EDIT = verb('edit', 'editing', 'edited');
const QUERY = verb('query', 'querying', 'queried');
const OPEN = verb('open', 'opening', 'opened');
const COMMENT = verb('comment', 'commenting', 'commented');
const PATCH = verb('patch', 'patching', 'patched');
const RUN = verb('run', 'running', 'ran');
const ASK = verb('ask', 'asking', 'asked');
const WAIT = verb('wait', 'waiting', 'waited');

interface Action {
  verb: Verb;
  object: string;
}

function action(verb: Verb, object: string): Action {
  return { verb, object };
}

/// Tool names are written for the model, so the trace gives the reader a plain
/// description instead. An unknown name is spelled out from its identifier
/// rather than hidden, so a newly added tool still shows up as words.
const TOOL_ACTIONS: Record<string, Action> = {
  load_tools: action(LOAD, 'tools'),
  search_tools: action(SEARCH, 'for tools'),
  fetch_url: action(FETCH, 'a web page'),
  web_search: action(SEARCH, 'the web'),
  search_knowledge: action(SEARCH, 'the knowledge base'),
  search_chat_history: action(SEARCH, 'earlier messages'),
  list_sources: action(LIST, 'connected sources'),
  list_projects: action(LIST, 'projects'),
  list_tasks: action(LIST, 'tasks'),
  list_chats: action(LIST, 'chats'),
  list_members: action(LIST, 'members'),
  list_files: action(LIST, 'files'),
  read_file: action(READ, 'a file'),
  list_documents: action(LIST, 'workspace documents'),
  read_document: action(READ, 'a workspace document'),
  read_chat_evidence: action(READ, 'an earlier chat'),
  get_task_run: action(CHECK, 'a task run'),
  get_build_status: action(CHECK, 'GitHub build status'),
  read_check_logs: action(READ, 'GitHub check logs'),
  list_deployments: action(LIST, 'GitHub deployments'),
  list_issues: action(LIST, 'GitHub issues'),
  get_issue: action(READ, 'a GitHub issue'),
  read_repository_file: action(READ, 'a repository file'),
  create_task: action(CREATE, 'a task'),
  update_task: action(UPDATE, 'a task'),
  create_document: action(CREATE, 'a document'),
  update_document: action(UPDATE, 'a document'),
  send_message: action(SEND, 'a message'),
  create_reminder: action(CREATE, 'a reminder'),
  cancel_reminder: action(CANCEL, 'a reminder'),
  list_reminders: action(LIST, 'reminders'),
  memory_write: action(WRITE, 'memory'),
  memory_append: action(APPEND, 'to memory'),
  memory_delete: action(FORGET, 'memory'),
  memory_read: action(READ, 'memory'),
  memory_list: action(LIST, 'memories'),
  finalize_project: action(CREATE, 'a project'),
  generate_image: action(GENERATE, 'an image'),
  edit_image: action(EDIT, 'an image'),
  query_prometheus: action(QUERY, 'Prometheus'),
  list_grafana_dashboards: action(LIST, 'Grafana dashboards'),
  create_pull_request: action(OPEN, 'a pull request'),
  comment_on_issue: action(COMMENT, 'on GitHub'),
  apply_patch: action(PATCH, 'a file'),
  write_file: action(WRITE, 'a file'),
  run_shell: action(RUN, 'a shell command'),
  run_command: action(RUN, 'a command'),
  ask_user: action(ASK, 'you a question'),
  tail_job: action(READ, 'a job log'),
  wait_for: action(WAIT, 'for something to finish'),
};

const ACRONYMS = new Set(['url', 'id', 'api', 'pr', 'ci', 'sha', 'http', 'json', 'html']);

function capitalise(phrase: string): string {
  return phrase.charAt(0).toUpperCase() + phrase.slice(1);
}

function humanise(name: string): string {
  return capitalise(
    name
      .split(/_+/)
      .filter(Boolean)
      .map((word) => (ACRONYMS.has(word) ? word.toUpperCase() : word))
      .join(' ')
  );
}

const SERVER_SEPARATOR = '__';

function toolName(name: string): string {
  const index = name.indexOf(SERVER_SEPARATOR);
  const server = index === -1 ? '' : name.slice(0, index);
  const tool = index === -1 ? '' : name.slice(index + SERVER_SEPARATOR.length);
  if (!server || !tool) return humanise(name);
  return `${humanise(tool)} (${server})`;
}

type ToolState =
  | 'approval'
  | 'pending'
  | 'stopped'
  | 'denied'
  | 'timed_out'
  | 'cancelled'
  | 'ok'
  | 'failed';

/// A record stored before refusals were kept says why it was refused only in
/// the server's words, written bare on an agent's own permission card and
/// behind the error prefix otherwise.
const WORDED_REFUSALS = new Map<string, Refusal>(
  (
    [
      [DENIED_DETAIL, 'denied'],
      [APPROVAL_TIMED_OUT_DETAIL, 'timed_out'],
      [APPROVAL_CANCELLED_DETAIL, 'cancelled'],
    ] as const
  ).flatMap(([detail, refusal]) => [
    [detail, refusal],
    [`${TOOL_ERROR_PREFIX}${detail}`, refusal],
  ])
);

const REFUSED: ReadonlySet<ToolState> = new Set(REFUSALS);

function toolState(call: ToolCallRecord): ToolState {
  if (call.approval === 'pending') return 'approval';
  if (call.pending) return 'pending';
  if (call.approval === 'denied' && call.detail !== UNFINISHED_DETAIL) return 'denied';
  if (call.success) return 'ok';
  if (call.detail === UNFINISHED_DETAIL || call.detail === AWAITING_APPROVAL_DETAIL) {
    return 'stopped';
  }
  if (call.refusal !== undefined) return call.refusal ?? 'failed';
  return WORDED_REFUSALS.get(call.detail) ?? 'failed';
}

function toolLabel(name: string, state: ToolState): string {
  const known = TOOL_ACTIONS[name];
  if (!known) return toolName(name);
  const {
    verb: { base, gerund, past },
    object,
  } = known;
  switch (state) {
    case 'approval':
      return `Waiting to ${base} ${object}`;
    case 'pending':
      return capitalise(`${gerund} ${object}`);
    case 'stopped':
      return `Stopped ${gerund} ${object}`;
    case 'timed_out':
      return `Approval timed out for ${gerund} ${object}`;
    case 'cancelled':
      return `Cancelled ${gerund} ${object}`;
    case 'denied':
      return `Denied ${gerund} ${object}`;
    case 'failed':
      return `Failed to ${base} ${object}`;
    case 'ok':
      return capitalise(`${past} ${object}`);
  }
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

const LINE_COUNT = /\s*\((\d+) lines\)$/;
const SOURCE_NAME_END = ' [';
const SOURCES_NAMED = 3;
const COMMIT_SHA = /^[0-9a-f]{40}$/i;
const SHORT_SHA = 7;

function gitReference(value: unknown): string | null {
  const reference = text(value);
  return reference && COMMIT_SHA.test(reference) ? reference.slice(0, SHORT_SHA) : reference;
}

/// A listing is one line per source, each opening with the name; the trace
/// carries the first line and how many there were.
function sourceNames(detail: string): string | null {
  const counted = detail.match(LINE_COUNT);
  const names = detail
    .replace(LINE_COUNT, '')
    .split('\n')
    .map((line) => line.split(SOURCE_NAME_END)[0].trim())
    .filter(Boolean);
  const count = counted ? Number(counted[1]) : names.length;
  if (count > SOURCES_NAMED || count > names.length) return `${count} sources`;
  return names.join(', ') || null;
}

type Titles = ReadonlyMap<string, string>;

const TITLED_RECORD = /"id":"([^"]+)"[^{}]*?"title":"((?:[^"\\]|\\.)*)"/g;
const RECORD_ID = /"id":"([^"]+)"/;

function unescaped(value: string): string {
  try {
    return JSON.parse(`"${value}"`);
  } catch {
    return value;
  }
}

/// Every document record a turn's calls returned, by id, so a call that named
/// a document only by its id can be shown by the title another call carried.
function documentTitles(calls: ToolCallRecord[]): Titles {
  const titles = new Map<string, string>();
  for (const call of calls) {
    if (!call.success) continue;
    for (const match of call.detail.matchAll(TITLED_RECORD)) {
      titles.set(match[1], unescaped(match[2]));
    }
    if (call.name !== 'create_document') continue;
    const args = parseArguments(call.arguments);
    const title = args ? text(args.title) : null;
    const id = call.detail.match(RECORD_ID)?.[1];
    if (title && id) titles.set(id, title);
  }
  return titles;
}

function documentTitle(args: Arguments, titles: Titles): string | null {
  return text(args.title) ?? (isString(args.id) ? (titles.get(args.id) ?? null) : null);
}

type Derive = (args: Arguments, detail: string, titles: Titles) => string | null;

/// The detail column carries the first line of what the tool returned, and for
/// these tools that line is written for the model: a preamble saying where the
/// schemas will be or that a page is data and not instructions, or the opening
/// of a JSON record. The arguments say what the call was actually about, so the
/// reader gets those.
const DETAIL_FROM_ARGUMENTS: Partial<Record<string, Derive>> = {
  load_tools: (args) =>
    Array.isArray(args.names) ? args.names.filter(isString).map(toolName).join(', ') || null : null,
  fetch_url: (args) => text(args.url),
  web_search: (args) => (isString(args.query) ? `“${args.query}”` : null),
  list_sources: (_, detail) => sourceNames(detail),
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
  read_document: (args, _, titles) => documentTitle(args, titles),
  create_document: (args) => text(args.title),
  update_document: (args, _, titles) => documentTitle(args, titles),
  create_reminder: (args) =>
    join([text(args.content), isString(args.due_at) ? formatDue(args.due_at) : null], ' · '),
};

const JSON_LINE = /^\s*(?:\{\s*(?:"|\})|\[\s*(?:[{["\]]|\d))/;
const BARE_STREAM_LABEL = /^std(?:out|err):(?:\s*\(\d+ lines\))?$/;
const COMMAND_TOOLS = new Set(['run_command', 'run_shell']);

/// A failed call's detail is its error and a waiting call's is its state, and
/// both are the server's words about this call rather than a preamble. A JSON
/// record nothing here can summarise is left out rather than shown as braces,
/// and why a call did not run is not said twice when its label says it.
function toolDetail(call: ToolCallRecord, state: ToolState, titles: Titles): string {
  if (REFUSED.has(state) && call.name in TOOL_ACTIONS) return '';
  if (!call.success || call.pending || call.approval === 'pending') return call.detail;
  const derive = DETAIL_FROM_ARGUMENTS[call.name];
  const args = derive ? parseArguments(call.arguments) : null;
  const derived = derive && args ? derive(args, call.detail, titles) : null;
  if (derived) return derived;
  const bareLabel = COMMAND_TOOLS.has(call.name) && BARE_STREAM_LABEL.test(call.detail);
  return JSON_LINE.test(call.detail) || bareLabel ? '' : call.detail;
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
    <p
      className={`tool-call-reason${stated ? '' : ' tool-call-reason--missing'}`}
      data-testid="tool-call-reason"
    >
      {stated || REASON_MISSING}
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
  titles,
  thinking,
  answered,
  live,
  onDecide,
  onAnswer,
}: {
  call: ToolCallRecord;
  titles: Titles;
  thinking: boolean;
  answered: boolean;
  live: boolean;
  onDecide?: (id: string, approved: boolean) => void;
  onAnswer?: (content: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const args = formatArguments(call.arguments);
  const state = toolState(call);
  const status = REFUSED.has(state) || state === 'stopped' ? 'failed' : state;

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
        <span className="tool-call-name">{toolLabel(call.name, state)}</span>
        <span className="tool-call-detail">{toolDetail(call, state, titles)}</span>
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
  const titles = documentTitles(calls);

  return (
    <ol className="tool-trace" data-testid="tool-trace">
      {calls.map((call) => (
        <ToolTraceRow
          key={call.id}
          call={call}
          titles={titles}
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
