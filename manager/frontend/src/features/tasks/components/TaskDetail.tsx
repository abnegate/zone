import { Button, Select } from '@zone/ui';
import { useEffect, useId, useMemo, useRef, useState } from 'react';
import PencilIcon from '../../../shared/components/PencilIcon/PencilIcon';
import type { Source } from '../../../types';
import type { Project } from '../../projects/types';
import { READ_ONLY } from '../access';
import { PRIORITIES } from '../priority';
import { relationsOf } from '../relations';
import { STATUS_OPTIONS } from '../status';
import { awaitsTitle } from '../title';
import type { Task, TaskStatus, UpdateTaskRequest } from '../types';
import { DiscardChangesDialog } from './DiscardChangesDialog';
import { Links } from './Links';
import { TaskTitleInput } from './TaskTitleInput';

type Draft = {
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
  tasks: readonly Task[];
  projects: Project[];
  sources: Source[];
  focusTitle?: boolean;
  onOpen: (task: Task) => void;
  onClose: () => void;
  onExecute: () => void;
  onSave: (request: UpdateTaskRequest) => Promise<void>;
  writable?: boolean;
};

const SAVE_FAILED = 'Failed to save task';

function sameIds(left: string[], right: string[]): boolean {
  if (left.length !== right.length) return false;
  const sorted = [...right].sort();
  return [...left].sort().every((id, index) => id === sorted[index]);
}

function hydrate(task: Task): Draft {
  return {
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

function rebase(draft: Draft, previous: Draft, next: Draft): Draft {
  const edited = (Object.keys(next) as (keyof Draft)[]).filter(
    (key) => JSON.stringify(draft[key]) !== JSON.stringify(previous[key])
  );
  return Object.assign({ ...next }, Object.fromEntries(edited.map((key) => [key, draft[key]])));
}

function changed(task: Task, draft: Draft): UpdateTaskRequest {
  const request: UpdateTaskRequest = {};
  const description = draft.description.trim();
  if (description !== task.description.trim()) request.description = description;
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
  tasks,
  projects,
  sources,
  focusTitle = false,
  onOpen,
  onClose,
  onExecute,
  onSave,
  writable = true,
}: TaskDetailProps) {
  const [draft, setDraft] = useState<Draft>(() => hydrate(task));
  const [saving, setSaving] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [leaving, setLeaving] = useState<Task | null>(null);
  const hydrated = useRef(task);
  const renameButton = useRef<HTMLButtonElement>(null);
  const titleText = useRef<HTMLSpanElement>(null);
  const wasRenaming = useRef(false);

  useEffect(() => {
    if (focusTitle) (renameButton.current ?? titleText.current)?.focus();
  }, [focusTitle]);

  useEffect(() => {
    if (wasRenaming.current && !renaming) renameButton.current?.focus();
    wasRenaming.current = renaming;
  }, [renaming]);

  useEffect(() => {
    const previous = hydrated.current;
    hydrated.current = task;
    if (previous === task) return;
    if (previous.id !== task.id) {
      setDraft(hydrate(task));
      setError(null);
      setLeaving(null);
      return;
    }
    setDraft((current) => rebase(current, hydrate(previous), hydrate(task)));
  }, [task]);

  const request = useMemo(() => changed(task, draft), [task, draft]);
  const relations = useMemo(() => relationsOf(task, tasks), [task, tasks]);
  const edited = Object.keys(request).length > 0;
  const canSave = writable && edited && draft.description.trim().length > 0 && !saving;
  const access = useId();

  const openRelated = (related: Task) => {
    if (edited) setLeaving(related);
    else onOpen(related);
  };

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
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : SAVE_FAILED);
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="task-details">
      <header className="task-details-header">
        {renaming && writable ? (
          <TaskTitleInput
            title={task.title}
            className="task-details-title"
            onSave={(title) => onSave({ title })}
            onDone={() => setRenaming(false)}
          />
        ) : (
          <h2 className="task-details-title" aria-busy={awaitsTitle(task) || undefined}>
            {writable ? (
              <button
                ref={renameButton}
                type="button"
                className={`task-title-button${awaitsTitle(task) ? ' task-title-pending' : ''}`}
                aria-label={`Rename ${task.title}`}
                title="Rename"
                onClick={() => setRenaming(true)}
              >
                <span className="task-title-text">{task.title}</span>
                <PencilIcon />
              </button>
            ) : (
              <span ref={titleText} className="task-title-text" tabIndex={-1}>
                {task.title}
              </span>
            )}
          </h2>
        )}
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

        <Links task={task} relations={relations} onOpen={openRelated} />

        <div className="form-group">
          <label htmlFor="task-detail-description">Description</label>
          <textarea
            id="task-detail-description"
            rows={4}
            readOnly={!writable}
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
            readOnly={!writable}
            value={draft.acceptance_criteria}
            onChange={(event) =>
              setDraft((current) => ({ ...current, acceptance_criteria: event.target.value }))
            }
          />
        </div>

        <fieldset className="task-details-fields" disabled={!writable}>
          <div className="form-row">
            <Select
              id="task-detail-status"
              label="Status"
              value={draft.status}
              onValueChange={(next) =>
                setDraft((current) => ({ ...current, status: next as TaskStatus }))
              }
              wrapperClassName="form-group"
              options={[...STATUS_OPTIONS]}
            />

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
                    onClick={() =>
                      setDraft((current) => ({ ...current, priority: priority.value }))
                    }
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

              <Select
                id="task-detail-source"
                label="Code Source"
                value={draft.source_id}
                onValueChange={(next) => setDraft((current) => ({ ...current, source_id: next }))}
                wrapperClassName="form-group"
                options={[
                  { value: '', label: 'Select a source...' },
                  ...sources.map((source) => ({
                    value: source.id,
                    label: `${source.name} (${source.source_type})`,
                  })),
                ]}
              />
            </>
          )}
        </fieldset>
      </div>

      <footer className="task-details-actions">
        {!writable && (
          <p id={access} className="task-details-access">
            {READ_ONLY}
          </p>
        )}
        <Button variant="secondary" onClick={onExecute}>
          {writable ? 'Execute' : 'Runs'}
        </Button>
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button
          onClick={handleSave}
          disabled={!canSave}
          aria-describedby={writable ? undefined : access}
        >
          Save
        </Button>
      </footer>
      {leaving && (
        <DiscardChangesDialog
          task={task}
          onDiscard={() => {
            setLeaving(null);
            onOpen(leaving);
          }}
          onClose={() => setLeaving(null)}
        />
      )}
    </section>
  );
}
