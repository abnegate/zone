import { Badge, Button } from '@zone/ui';
import { Link } from 'react-router-dom';
import type { AutomationStage, AutomationTask, ProjectAutomation } from '../types';

interface AutomationPanelProps {
  automation: ProjectAutomation | null;
  loading: boolean;
  error: string | null;
  resuming: boolean;
  onResume: () => void;
}

const STAGE_LABELS: Record<AutomationStage, string> = {
  idle: 'Waiting to start',
  running: 'Running',
  no_changes: 'No change made',
  awaiting_checks: 'Waiting for checks',
  awaiting_reviews: 'Under review',
  fixing: 'Fixing review findings',
  merging: 'Merging',
  post_merge: 'Watching post-merge jobs',
  merged: 'Merged',
  paused: 'Needs a person',
};

const STAGE_VARIANTS: Record<AutomationStage, 'success' | 'warning' | 'destructive' | 'default'> = {
  idle: 'default',
  running: 'default',
  no_changes: 'warning',
  awaiting_checks: 'default',
  awaiting_reviews: 'default',
  fixing: 'warning',
  merging: 'default',
  post_merge: 'default',
  merged: 'success',
  paused: 'destructive',
};

function stageOf(task: AutomationTask): string {
  if (task.stage) return STAGE_LABELS[task.stage];
  if (task.status === 'complete') return 'Complete';
  if (!task.is_agentic) return 'Manual';
  return 'Not started';
}

function variantOf(task: AutomationTask): 'success' | 'warning' | 'destructive' | 'default' {
  if (task.stage) return STAGE_VARIANTS[task.stage];
  return task.status === 'complete' ? 'success' : 'default';
}

/**
 * Where a project that runs itself is: its counts, the reason it stopped when
 * it did, and every task with its stage, its reviewers and its pull request.
 */
export function AutomationPanel({
  automation,
  loading,
  error,
  resuming,
  onResume,
}: AutomationPanelProps) {
  if (loading && !automation) {
    return (
      <div className="automation-panel" data-testid="automation-panel">
        <span className="spinner" /> Reading automation…
      </div>
    );
  }
  if (error && !automation) {
    return (
      <div className="automation-panel automation-panel--error" data-testid="automation-panel">
        {error}
      </div>
    );
  }
  if (!automation) return null;

  const paused = automation.paused_reason || automation.counts.paused > 0;

  return (
    <div className="automation-panel" data-testid="automation-panel">
      <div className="automation-summary">
        <span>
          {automation.counts.complete} of {automation.counts.agentic} tasks merged
          {automation.counts.in_flight > 0 ? `, ${automation.counts.in_flight} in flight` : ''}
        </span>
        <span className="automation-links">
          {automation.planner_chat_id && (
            <Link to={`/chats?id=${automation.planner_chat_id}`}>Interview</Link>
          )}
          {automation.updates_chat_id && (
            <Link to={`/chats?id=${automation.updates_chat_id}`}>Updates</Link>
          )}
        </span>
      </div>
      {automation.completed_at && (
        <p className="automation-note automation-note--done">
          Every agentic task merged; automation finished.
        </p>
      )}
      {paused && (
        <div className="automation-paused" data-testid="automation-paused">
          <p>
            <strong>Paused.</strong>{' '}
            {automation.paused_reason ??
              `${automation.counts.paused} task${automation.counts.paused === 1 ? '' : 's'} need a person.`}
          </p>
          <Button size="sm" onClick={onResume} disabled={resuming}>
            {resuming ? 'Resuming…' : 'Resume'}
          </Button>
        </div>
      )}
      <ul className="automation-tasks">
        {automation.tasks.map((task) => (
          <li key={task.task_id} className="automation-task">
            <div className="automation-task-head">
              <span className="automation-task-kind">{task.kind ?? 'task'}</span>
              <span className="automation-task-title">{task.title}</span>
              <Badge variant={variantOf(task)}>{stageOf(task)}</Badge>
            </div>
            <div className="automation-task-meta">
              {task.runs > 0 && (
                <span>
                  {task.runs} run{task.runs === 1 ? '' : 's'}
                </span>
              )}
              {task.review_rounds > 0 && (
                <span>
                  {task.review_rounds} review round{task.review_rounds === 1 ? '' : 's'}
                </span>
              )}
              {task.reviewers && <span>Reviewed by {task.reviewers}</span>}
              {task.pr_url && (
                <a href={task.pr_url} target="_blank" rel="noopener noreferrer">
                  Pull request
                </a>
              )}
            </div>
            {task.reason && task.stage !== 'merged' && (
              <p className="automation-task-reason">{task.reason}</p>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
