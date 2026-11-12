import { Button, Checkbox, Modal, Select } from '@zone/ui';
import { useMemo, useState } from 'react';
import type { Project } from '../../projects/types';
import { Edits, ProjectEdit } from '../Edits';
import { PRIORITIES } from '../priority';
import { STATUS_OPTIONS } from '../status';
import type { Task, TaskChanges, TaskSettings, TaskStatus } from '../types';
import './EditTasksDialog.css';

const UNCHANGED = '';
const ON = 'on';
const OFF = 'off';
const INDETERMINATE = 'indeterminate';

type CheckState = boolean | typeof INDETERMINATE;

const SAVE_FAILED = 'The tasks could not be updated';

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

const PARTLY_SHARED_CYCLE: readonly ProjectEdit[] = [
  ProjectEdit.UNCHANGED,
  ProjectEdit.ADD,
  ProjectEdit.REMOVE,
];
const SHARED_CYCLE: readonly ProjectEdit[] = [ProjectEdit.UNCHANGED, ProjectEdit.REMOVE];
const UNUSED_CYCLE: readonly ProjectEdit[] = [ProjectEdit.UNCHANGED, ProjectEdit.ADD];

interface EditTasksDialogProps {
  tasks: readonly Task[];
  projects: readonly Project[];
  onSave: (changes: TaskChanges) => Promise<void>;
  onClose: () => void;
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

function membershipOf(tasks: readonly Task[]): ReadonlyMap<string, CheckState> {
  const counts = new Map<string, number>();
  for (const task of tasks) {
    for (const id of new Set(task.project_ids)) counts.set(id, (counts.get(id) ?? 0) + 1);
  }
  return new Map(
    [...counts].map(([id, count]) => [id, count === tasks.length ? true : INDETERMINATE])
  );
}

function cycleOf(original: CheckState): readonly ProjectEdit[] {
  if (original === INDETERMINATE) return PARTLY_SHARED_CYCLE;
  return original ? SHARED_CYCLE : UNUSED_CYCLE;
}

function nextEdit(original: CheckState, edit: ProjectEdit): ProjectEdit {
  const cycle = cycleOf(original);
  return cycle[(cycle.indexOf(edit) + 1) % cycle.length];
}

function stateOf(original: CheckState, edit: ProjectEdit): CheckState {
  if (edit === ProjectEdit.ADD) return true;
  if (edit === ProjectEdit.REMOVE) return false;
  return original;
}

export function EditTasksDialog({ tasks, projects, onSave, onClose }: EditTasksDialogProps) {
  const [edits, setEdits] = useState(Edits.NONE);
  const [saving, setSaving] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const membership = useMemo(() => membershipOf(tasks), [tasks]);
  const changes = edits.request();

  const originalOf = (id: string): CheckState => membership.get(id) ?? false;

  const change = <K extends keyof TaskSettings>(key: K, value: TaskSettings[K] | undefined) => {
    setEdits((current) => current.with(key, value));
  };

  const cycleProject = (id: string) => {
    setEdits((current) => current.project(id, nextEdit(originalOf(id), current.projectEdit(id))));
  };

  const save = async () => {
    if (!edits.touched || saving) return;
    setSaving(true);
    setFailure(null);
    try {
      await onSave(edits.request());
    } catch (error) {
      setFailure(error instanceof Error ? error.message : SAVE_FAILED);
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
                <Checkbox
                  key={project.id}
                  id={`edit-tasks-project-${project.id}`}
                  label={project.name}
                  checked={stateOf(originalOf(project.id), edits.projectEdit(project.id))}
                  disabled={saving}
                  onCheckedChange={() => cycleProject(project.id)}
                />
              ))}
            </div>
          </fieldset>
        )}

        {failure && (
          <div className="modal-error" role="alert">
            {failure}
          </div>
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
