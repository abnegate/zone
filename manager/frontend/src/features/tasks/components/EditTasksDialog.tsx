import { Button, Modal, Select } from '@zone/ui';
import { useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { Project } from '../../projects/types';
import { Edits } from '../Edits';
import { PRIORITIES } from '../priority';
import { STATUS_OPTIONS } from '../status';
import type { Task, TaskChanges, TaskStatus } from '../types';
import './EditTasksDialog.css';

const UNCHANGED = '';
const ON = 'on';
const OFF = 'off';

const UNCHANGED_OPTION = { value: UNCHANGED, label: 'Unchanged' };

const STATUS_CHOICES = [UNCHANGED_OPTION, ...STATUS_OPTIONS];

const PRIORITY_CHOICES = [
  UNCHANGED_OPTION,
  ...PRIORITIES.map((priority) => ({
    value: String(priority.value),
    label: `${priority.value} · ${priority.label}`,
  })),
];

const SWITCH_CHOICES = [UNCHANGED_OPTION, { value: ON, label: 'On' }, { value: OFF, label: 'Off' }];

interface EditTasksDialogProps {
  tasks: readonly Task[];
  projects: readonly Project[];
  onSave: (changes: TaskChanges) => Promise<void>;
  onClose: () => void;
}

interface ProjectOptionProps {
  project: Project;
  checked: boolean;
  indeterminate: boolean;
  disabled: boolean;
  onToggle: () => void;
}

function titleOf(count: number): string {
  return `Edit ${count} ${count === 1 ? 'task' : 'tasks'}`;
}

function choiceOf(value: boolean | undefined): string {
  if (value === undefined) return UNCHANGED;
  return value ? ON : OFF;
}

function switchOf(choice: string): boolean | undefined {
  return choice === UNCHANGED ? undefined : choice === ON;
}

function sharedProjects(tasks: readonly Task[]): string[] {
  const [first, ...rest] = tasks;
  if (!first) return [];
  return first.project_ids.filter((id) => rest.every((task) => task.project_ids.includes(id)));
}

function partlySharedProjects(tasks: readonly Task[], shared: readonly string[]): Set<string> {
  const partly = new Set(tasks.flatMap((task) => task.project_ids));
  for (const id of shared) partly.delete(id);
  return partly;
}

function ProjectOption({
  project,
  checked,
  indeterminate,
  disabled,
  onToggle,
}: ProjectOptionProps) {
  const input = useRef<HTMLInputElement>(null);

  useLayoutEffect(() => {
    if (input.current) input.current.indeterminate = indeterminate;
  }, [indeterminate]);

  return (
    <label>
      <input
        ref={input}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={onToggle}
      />
      {project.name}
    </label>
  );
}

export function EditTasksDialog({ tasks, projects, onSave, onClose }: EditTasksDialogProps) {
  const [edits, setEdits] = useState(Edits.NONE);
  const [saving, setSaving] = useState(false);
  const shared = useMemo(() => sharedProjects(tasks), [tasks]);
  const partly = useMemo(() => partlySharedProjects(tasks, shared), [tasks, shared]);
  const changes = edits.request();
  const chosen = changes.project_ids;

  const change = <K extends keyof TaskChanges>(key: K, value: TaskChanges[K] | undefined) => {
    setEdits((current) => current.with(key, value));
  };

  const toggleProject = (id: string) => {
    setEdits((current) => {
      const projectIds = current.request().project_ids ?? shared;
      return current.with(
        'project_ids',
        projectIds.includes(id)
          ? projectIds.filter((projectId) => projectId !== id)
          : [...projectIds, id]
      );
    });
  };

  const save = async () => {
    if (!edits.touched || saving) return;
    setSaving(true);
    try {
      await onSave(edits.request());
    } catch {
      // The page reports the failure; the dialog stays open so the edit can be retried.
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal isOpen onClose={saving ? undefined : onClose} title={titleOf(tasks.length)} size="md">
      <div className="edit-tasks">
        <div className="form-row">
          <Select
            id="edit-tasks-status"
            label="Status"
            value={changes.status ?? UNCHANGED}
            disabled={saving}
            onValueChange={(next) =>
              change('status', next === UNCHANGED ? undefined : (next as TaskStatus))
            }
            wrapperClassName="form-group"
            options={STATUS_CHOICES}
          />
          <Select
            id="edit-tasks-priority"
            label="Priority"
            value={changes.priority === undefined ? UNCHANGED : String(changes.priority)}
            disabled={saving}
            onValueChange={(next) =>
              change('priority', next === UNCHANGED ? undefined : Number(next))
            }
            wrapperClassName="form-group"
            options={PRIORITY_CHOICES}
          />
        </div>

        <div className="form-row">
          <Select
            id="edit-tasks-agentic"
            label="Agentic"
            value={choiceOf(changes.is_agentic)}
            disabled={saving}
            onValueChange={(next) => change('is_agentic', switchOf(next))}
            wrapperClassName="form-group"
            options={SWITCH_CHOICES}
          />
          <Select
            id="edit-tasks-approval"
            label="Require plan approval"
            value={choiceOf(changes.require_plan_approval)}
            disabled={saving}
            onValueChange={(next) => change('require_plan_approval', switchOf(next))}
            wrapperClassName="form-group"
            options={SWITCH_CHOICES}
          />
        </div>

        {projects.length > 0 && (
          <fieldset className="form-group edit-tasks-projects">
            <legend className="form-label">Projects</legend>
            <div className="edit-tasks-project-list">
              {projects.map((project) => (
                <ProjectOption
                  key={project.id}
                  project={project}
                  checked={(chosen ?? shared).includes(project.id)}
                  indeterminate={!chosen && partly.has(project.id)}
                  disabled={saving}
                  onToggle={() => toggleProject(project.id)}
                />
              ))}
            </div>
          </fieldset>
        )}
      </div>

      <div className="modal-actions">
        <Button variant="ghost" disabled={saving} onClick={onClose}>
          Cancel
        </Button>
        <Button disabled={!edits.touched} loading={saving} onClick={save}>
          Save changes
        </Button>
      </div>
    </Modal>
  );
}
