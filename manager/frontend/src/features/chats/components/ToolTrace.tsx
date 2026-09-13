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
/// description instead. Unknown names fall through unchanged rather than being
/// hidden, so a newly added tool still shows up.
const TOOL_LABELS: Record<string, string> = {
  search_knowledge: 'Searched the knowledge base',
  search_chat_history: 'Searched earlier messages',
  list_sources: 'Listed connected sources',
  list_projects: 'Listed projects',
  list_tasks: 'Listed tasks',
  list_documents: 'Listed workspace documents',
  read_document: 'Read a workspace document',
  get_build_status: 'Checked GitHub build status',
  list_deployments: 'Listed GitHub deployments',
  list_issues: 'Listed GitHub issues',
  read_repository_file: 'Read a repository file',
  create_task: 'Created a task',
  update_task: 'Updated a task',
  create_document: 'Created a document',
  update_document: 'Updated a document',
  send_message: 'Sent a message',
  create_reminder: 'Created a reminder',
  cancel_reminder: 'Cancelled a reminder',
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

function toolLabel(name: string): string {
  return TOOL_LABELS[name] ?? name;
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
  answered,
  onDecide,
  onAnswer,
}: {
  call: ToolCallRecord;
  answered: boolean;
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
      {call.reasoning?.trim() ? <Reasoning content={call.reasoning} open /> : null}
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
        <span className="tool-call-detail">{call.detail}</span>
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
      <JobCard call={call} />
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
  answered = false,
  onDecide,
  onAnswer,
}: {
  calls: ToolCallRecord[];
  /**
   * Whether a question on this trace has already been answered. An answer is an
   * ordinary user message, so the caller decides this by looking for a user
   * message newer than the assistant message the card sits on — no server state
   * says a question is settled, and none needs to.
   */
  answered?: boolean;
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
          answered={answered}
          onDecide={onDecide}
          onAnswer={onAnswer}
        />
      ))}
    </ol>
  );
}
