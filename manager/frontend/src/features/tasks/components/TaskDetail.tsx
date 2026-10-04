import { Button } from '@zone/ui';
import { useEffect, useMemo, useState } from 'react';
import type { Source } from '../../../types';
import type { Project } from '../../projects/types';
import type { Task, TaskStatus, UpdateTaskRequest } from '../types';

type Draft = {
  title: string;
  description: string;
  acceptance_criteria: string;
  status: TaskStatus;
  priority: number | null;
  project_ids: string[];
  is_agentic: boolean;
  require_plan_approval: boolean;
  source_id: string;
  model_name: string;
};

type TaskDetailProps = {
  task: Task;
  projects: Project[];
  sources: Source[];
  onClose: () => void;
  onExecute: () => void;
  onSave: (request: UpdateTaskRequest) => Promise<void>;
};

const STATUSES: { value: TaskStatus; label: string }[] = [
  { value: 'created', label: 'Created' },
  { value: 'queued', label: 'Queued' },
  { value: 'in_progress', label: 'In Progress' },
  { value: 'blocked', label: 'Blocked' },
  { value: 'review', label: 'Review' },
  { value: 'complete', label: 'Complete' },
];

const PRIORITIES: { value: number; label: string }[] = [
  { value: 1, label: 'Lowest' },
  { value: 2, label: 'Low' },
  { value: 3, label: 'Medium' },
  { value: 4, label: 'High' },
  { value: 5, label: 'Highest' },
];

function sameIds(left: string[], right: string[]): boolean {
  if (left.length !== right.length) return false;
  const a = [...left].sort();
  const b = [...right].sort();
  return a.every((id, index) => id === b[index]);
}

function hydrate(task: Task): Draft {
  return {
    title: task.title,
    description: task.description,
    acceptance_criteria: task.acceptance_criteria ?? '',
    status: task.status,
    priority: task.priority,
    project_ids: [...task.project_ids],
    is_agentic: task.is_agentic,
    require_plan_approval: task.require_plan_approval ?? false,
    source_id: task.source_id ?? '',
    model_name: task.model_name ?? '',
  };
}

function changed(task: Task, draft: Draft): UpdateTaskRequest {
  const request: UpdateTaskRequest = {};
  const title = draft.title.trim();
  const description = draft.description.trim();
  if (title !== task.title) request.title = title;
  if (description !== task.description) request.description = description;
  if (draft.acceptance_criteria !== (task.acceptance_criteria ?? '')) {
    request.acceptance_criteria = draft.acceptance_criteria;
  }
  if (draft.status !== task.status) request.status = draft.status;
  if (draft.priority !== task.priority && draft.priority != null) {
    request.priority = draft.priority;
  }
  if (!sameIds(draft.project_ids, task.project_ids)) {
    request.project_ids = draft.project_ids;
  }
  if (draft.is_agentic !== task.is_agentic) request.is_agentic = draft.is_agentic;
  if (draft.require_plan_approval !== (task.require_plan_approval ?? false)) {
    request.require_plan_approval = draft.require_plan_approval;
  }
  if (draft.source_id !== (task.source_id ?? '') && draft.source_id) {
    request.source_id = draft.source_id;
  }
  const model = draft.model_name.trim();
  if (model !== (task.model_name ?? '') && model) {
    request.model_name = model;
  }
  return request;
}

export function TaskDetail({
  task,
  projects,
  sources,
  onClose,
  onExecute,
  onSave,
}: TaskDetailProps) {
  const [draft, setDraft] = useState<Draft>(() => hydrate(task));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDraft(hydrate(task));
    setError(null);
  }, [task]);

  const request = useMemo(() => changed(task, draft), [task, draft]);
  const canSave =
    Object.keys(request).length > 0 &&
    draft.title.trim().length > 0 &&
    draft.description.trim().length > 0 &&
    !saving;

  const toggleProject = (id: string) => {
    setDraft((current) => {
      const selected = current.project_ids.includes(id)
        ? current.project_ids.filter((projectId) => projectId !== id)
        : [...current.project_ids, id];
      return { ...current, project_ids: selected };
    });
  };

  const handleSave = async () => {
    if (!canSave) return;
    setSaving(true);
    setError(null);
    try {
      await onSave(request);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save task');
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="task-details">
      <header className="task-details-header">
        <h2>{task.title}</h2>
        <Button variant="ghost" onClick={onClose}>
          Close
        </Button>
      </header>

      <div className="task-details-content">
        {error && (
          <div className="error-banner" role="alert">
            {error}
          </div>
        )}

        <div className="form-group">
          <label htmlFor="task-detail-title">Title</label>
          <input
            id="task-detail-title"
            type="text"
            value={draft.title}
            onChange={(event) => setDraft((current) => ({ ...current, title: event.target.value }))}
          />
        </div>

        <div className="form-group">
          <label htmlFor="task-detail-description">Description</label>
          <textarea
            id="task-detail-description"
            rows={4}
            value={draft.description}
            onChange={(event) =>
              setDraft((current) => ({ ...current, description: event.target.value }))
            }
          />
        </div>

        <div className="form-group">
          <label htmlFor="task-detail-criteria">
            Acceptance Criteria
            <span className="label-optional">optional</span>
          </label>
          <textarea
            id="task-detail-criteria"
            rows={3}
            value={draft.acceptance_criteria}
            onChange={(event) =>
              setDraft((current) => ({ ...current, acceptance_criteria: event.target.value }))
            }
          />
        </div>

        <div className="form-row">
          <div className="form-group">
            <label htmlFor="task-detail-status">Status</label>
            <select
              id="task-detail-status"
              className="ui-select"
              value={draft.status}
              onChange={(event) =>
                setDraft((current) => ({ ...current, status: event.target.value as TaskStatus }))
              }
            >
              {STATUSES.map((status) => (
                <option key={status.value} value={status.value}>
                  {status.label}
                </option>
              ))}
            </select>
          </div>

          <div className="form-group">
            <span id="task-detail-priority-label" className="form-label">
              Priority
            </span>
            <div
              className="priority-selector"
              role="group"
              aria-labelledby="task-detail-priority-label"
            >
              {PRIORITIES.map((priority) => (
                <button
                  key={priority.value}
                  type="button"
                  className={`priority-option ${draft.priority === priority.value ? 'selected' : ''}`}
                  onClick={() => setDraft((current) => ({ ...current, priority: priority.value }))}
                >
                  <span className="priority-number">{priority.value}</span>
                  <span className="priority-label">{priority.label}</span>
                </button>
              ))}
            </div>
          </div>
        </div>

        <fieldset className="form-group">
          <legend className="form-label">Projects</legend>
          <div className="task-details-projects">
            {projects.map((project) => (
              <label key={project.id}>
                <input
                  type="checkbox"
                  checked={draft.project_ids.includes(project.id)}
                  onChange={() => toggleProject(project.id)}
                />
                {project.name}
              </label>
            ))}
          </div>
        </fieldset>

        <div className="form-group">
          <label htmlFor="task-detail-model">Model</label>
          <input
            id="task-detail-model"
            type="text"
            value={draft.model_name}
            onChange={(event) =>
              setDraft((current) => ({ ...current, model_name: event.target.value }))
            }
          />
        </div>

        <div className="form-group">
          <label className="toggle-label">
            <span className="toggle-wrapper">
              <input
                type="checkbox"
                checked={draft.is_agentic}
                onChange={(event) =>
                  setDraft((current) => ({ ...current, is_agentic: event.target.checked }))
                }
              />
              <span className="toggle-slider" />
            </span>
            <span className="toggle-text">
              <span className="toggle-title">Enable Agentic Mode</span>
              <span className="toggle-desc">
                Allow this task to autonomously read/write code and query the knowledge base
              </span>
            </span>
          </label>
        </div>

        {draft.is_agentic && (
          <>
            <div className="form-group">
              <label className="toggle-label">
                <span className="toggle-wrapper">
                  <input
                    type="checkbox"
                    checked={draft.require_plan_approval}
                    onChange={(event) =>
                      setDraft((current) => ({
                        ...current,
                        require_plan_approval: event.target.checked,
                      }))
                    }
                  />
                  <span className="toggle-slider" />
                </span>
                <span className="toggle-text">
                  <span className="toggle-title">Require plan approval</span>
                  <span className="toggle-desc">
                    A run writes its plan and waits for you to approve it before changing anything
                  </span>
                </span>
              </label>
            </div>

            <div className="form-group">
              <label htmlFor="task-detail-source">Code Source</label>
              <select
                id="task-detail-source"
                className="ui-select"
                value={draft.source_id}
                onChange={(event) =>
                  setDraft((current) => ({ ...current, source_id: event.target.value }))
                }
              >
                <option value="">Select a source...</option>
                {sources.map((source) => (
                  <option key={source.id} value={source.id}>
                    {source.name} ({source.source_type})
                  </option>
                ))}
              </select>
            </div>
          </>
        )}
      </div>

      <footer className="task-details-actions">
        <Button variant="secondary" onClick={onExecute}>
          Execute
        </Button>
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button onClick={handleSave} disabled={!canSave}>
          Save
        </Button>
      </footer>
    </section>
  );
}
