import { useState } from 'react';
import type { ToolCallRecord } from '../types';

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

function ToolTraceRow({
  call,
  onDecide,
}: {
  call: ToolCallRecord;
  onDecide?: (id: string, approved: boolean) => void;
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
  onDecide,
}: {
  calls: ToolCallRecord[];
  onDecide?: (id: string, approved: boolean) => void;
}) {
  if (calls.length === 0) return null;

  return (
    <ol className="tool-trace" data-testid="tool-trace">
      {calls.map((call) => (
        <ToolTraceRow key={call.id} call={call} onDecide={onDecide} />
      ))}
    </ol>
  );
}
