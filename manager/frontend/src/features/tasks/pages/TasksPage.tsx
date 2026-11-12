import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Button, EmptyState, Select } from '@zone/ui';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { toast } from 'sonner';
import { client } from '../../../api/client';
import PageBar from '../../../shared/components/PageBar/PageBar';
import PlusIcon from '../../../shared/components/PlusIcon/PlusIcon';
import { permitsWrites, useWorkspace } from '../../../shared/context';
import { useProjects } from '../../projects/hooks';
import { ActiveRunError } from '../ActiveRunError';
import { READ_ONLY } from '../access';
import {
  CreateTaskWizard,
  DeleteTaskDialog,
  EditTasksDialog,
  SelectionBar,
  TaskDetail,
} from '../components';
import { Board, Skeleton, Unfolded, View } from '../components/board';
import { RANGE } from '../gesture';
import { TASKS_KEY, useTask, useTasks, useWorkspaceTasks } from '../hooks';
import { Selection } from '../Selection';
import { groupByStage, stageOf } from '../status';
import { TitleDraft } from '../TitleDraft';
import type { BoardActions, Renaming, Task, TaskChanges, UpdateTaskRequest } from '../types';
import { TaskExecutionView } from './TaskExecutionView';
import './TasksPage.css';

const ALL_PROJECTS = '';

const MOVE_FAILED = 'The task could not be moved';

const UPDATE_FAILED = 'The tasks could not be updated';

const RELATIONS_FAILED =
  'Tasks in other projects could not be loaded, so related tasks outside this project are missing';

const CLEARED = 'Selection cleared';

const ESCAPE = 'Escape';

const SHIELDED =
  'input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="dialog"], [role="menu"], [role="listbox"], [role="combobox"]';

const DRAGGING = '.board[data-dragging]';

const CARD = '.board li[data-task-id]';

const TITLE = '.task-card-open';

const CHECKBOX = '.task-card-select';

const COLUMN = '.board section[data-stage]';

function idsOf(tasks: readonly Task[]): string[] {
  return tasks.map((task) => task.id);
}

function columnOf(task: Task, tasks: readonly Task[]): string[] {
  return idsOf(groupByStage(tasks).get(stageOf(task.status).id) ?? []);
}

function quantity(count: number): string {
  return `${count} ${count === 1 ? 'task' : 'tasks'}`;
}

function selected(count: number): string {
  return `${quantity(count)} selected`;
}

function refusal(failure: unknown, tasks: readonly Task[]): string {
  if (!(failure instanceof Error)) return UPDATE_FAILED;
  if (!(failure instanceof ActiveRunError)) return failure.message;
  const titles = new Map(tasks.map((task) => [task.id, task.title]));
  const running = failure.ids.flatMap((id) => titles.get(id) ?? []);
  if (running.length === 0) return failure.message;
  if (running.length === 1) return `${running[0]} is running, so its status can't change`;
  return `${running.length} tasks are running, so their status can't change: ${running.join(', ')}`;
}

function lost(): boolean {
  const active = document.activeElement;
  return active === null || active === document.body;
}

function stranded(): boolean {
  return lost() || document.activeElement?.matches(CHECKBOX) === true;
}

function returnFocus(id: string | null): void {
  if (!stranded()) return;
  const card = Array.from(document.querySelectorAll<HTMLElement>(CARD)).find(
    (candidate) => candidate.dataset.taskId === id
  );
  (card?.querySelector<HTMLElement>(TITLE) ?? document.querySelector<HTMLElement>(COLUMN))?.focus();
}

function keepsSelection(event: KeyboardEvent): boolean {
  if (event.key !== ESCAPE || event.defaultPrevented) return true;
  if (document.querySelector(DRAGGING)) return true;
  return event.target instanceof Element && event.target.closest(SHIELDED) !== null;
}

export default function TasksPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const queryClient = useQueryClient();
  const { currentWorkspace, workspaces, writable } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const [scope, setScope] = useState(workspaceId);
  const [filterProject, setFilterProject] = useState(ALL_PROJECTS);
  const [renaming, setRenaming] = useState<Renaming | null>(null);
  const [view, setView] = useState(() => new View());
  const [selection, setSelection] = useState(Selection.EMPTY);
  const [editing, setEditing] = useState(false);
  const [announcement, setAnnouncement] = useState('');
  const toggled = useRef<string | null>(null);
  const linkedTaskId = searchParams.get('id');
  const taskParam = searchParams.get('task');

  if (scope !== workspaceId) {
    setScope(workspaceId);
    setFilterProject(ALL_PROJECTS);
    setRenaming(null);
    setView(new View());
    setSelection(Selection.EMPTY);
    setEditing(false);
  }

  const filter = (project: string) => {
    setFilterProject(project);
    setSelection(Selection.EMPTY);
  };

  const {
    tasks,
    loading: tasksLoading,
    error: tasksError,
    createTask,
    updateTask: updateListedTask,
    updateTasks,
    moveTask,
    deleteTask,
  } = useTasks(filterProject || undefined);
  const latest = useRef(tasks);
  const listed = useMemo(() => idsOf(tasks), [tasks]);
  const visible = useMemo(
    () => (writable ? selection.retain(listed) : Selection.EMPTY),
    [writable, selection, listed]
  );
  const chosen = useMemo(() => tasks.filter((task) => visible.has(task.id)), [tasks, visible]);

  if (visible !== selection) {
    setSelection(visible);
  }

  const spoken = visible.size > 0 ? selected(visible.size) : announcement && CLEARED;
  if (spoken !== announcement) {
    setAnnouncement(spoken);
  }

  useLayoutEffect(() => {
    latest.current = tasks;
  }, [tasks]);

  if (editing && visible.size === 0) {
    setEditing(false);
  }
  const filtered = filterProject !== ALL_PROJECTS;
  const workspace = useWorkspaceTasks(filtered);
  const known = workspace.tasks ?? tasks;

  const { projects, loading: projectsLoading } = useProjects('all');
  const { data: sources = [] } = useQuery({
    queryKey: ['sources', workspaceId],
    queryFn: () => client.getSources(workspaceId as string),
    enabled: !!workspaceId,
  });
  const {
    task: editorTask,
    loading: editorLoading,
    error: editorError,
    updateTask,
  } = useTask(taskParam);

  const [executionTask, setExecutionTask] = useState<Task | null>(null);
  const [deleting, setDeleting] = useState<Task | null>(null);
  const [removed, setRemoved] = useState<string | null>(null);
  const [followed, setFollowed] = useState<string | null>(null);
  const [showCreateModal, setShowCreateModal] = useState(false);

  if (!writable && (renaming !== null || deleting !== null)) {
    setRenaming(null);
    setDeleting(null);
  }

  const writableFor = (task: Task) =>
    task.workspace_id === workspaceId
      ? writable
      : permitsWrites(workspaces.find((workspace) => workspace.id === task.workspace_id));

  const projectNames = useMemo(
    () => new Map(projects.map((project) => [project.id, project.name])),
    [projects]
  );

  const editorReady = Boolean(taskParam) && editorTask?.id === taskParam;
  const editorFailed = Boolean(taskParam) && !editorLoading && !editorReady && Boolean(editorError);
  const listing = !taskParam || editorFailed;
  const showSpinner = Boolean(taskParam) && !editorReady && !editorFailed;

  useEffect(() => {
    if (taskParam && linkedTaskId) {
      const next = new URLSearchParams(searchParams);
      next.delete('id');
      setSearchParams(next, { replace: true });
      setExecutionTask(null);
      return;
    }
    if (!linkedTaskId || taskParam) return;
    const found = tasks.find((task) => task.id === linkedTaskId);
    if (found) {
      setExecutionTask(found);
    }
  }, [linkedTaskId, tasks, taskParam, searchParams, setSearchParams]);

  const openEditor = useCallback(
    (id: string) => {
      const next = new URLSearchParams(searchParams);
      next.set('task', id);
      next.delete('id');
      setSearchParams(next);
      setExecutionTask(null);
    },
    [searchParams, setSearchParams]
  );

  const open = useCallback(
    (task: Task, host?: Task) => {
      if (host) view.trail.push(host.id);
      view.trail.push(task.id);
      openEditor(task.id);
    },
    [view, openEditor]
  );

  const follow = (task: Task) => {
    setFollowed(task.id);
    open(task);
  };

  const closeEditor = () => {
    setFollowed(null);
    setExecutionTask(null);
    const next = new URLSearchParams(searchParams);
    next.delete('task');
    setSearchParams(next, { replace: true });
  };

  const closeExecution = () => {
    setExecutionTask(null);
    if (searchParams.get('id')) {
      const next = new URLSearchParams(searchParams);
      next.delete('id');
      setSearchParams(next, { replace: true });
    }
  };

  const handleSave = async (request: UpdateTaskRequest) => {
    await updateTask(request);
    await queryClient.invalidateQueries({ queryKey: TASKS_KEY });
  };

  const confirmDelete = async (task: Task) => {
    await deleteTask(task.id);
    setRemoved(task.id);
    setDeleting(null);
  };

  const actions = useMemo<BoardActions>(
    () => ({
      open,
      run: setExecutionTask,
      rename: (task) => setRenaming({ id: task.id, draft: new TitleDraft(task.title) }),
      retitle: async (task, title) => {
        await updateListedTask(task.id, { title });
      },
      finish: (task) => setRenaming((current) => (current?.id === task.id ? null : current)),
      remove: setDeleting,
      move: async (task, status) => {
        try {
          await moveTask(task.id, status);
          return true;
        } catch (failure) {
          toast.error(`Couldn't move ${task.title}`, {
            description: failure instanceof Error ? failure.message : MOVE_FAILED,
          });
          return false;
        }
      },
      select: (task, gesture) => {
        const column = gesture === RANGE ? columnOf(task, latest.current) : null;
        toggled.current = task.id;
        setSelection((current) =>
          column ? current.extend(task.id, column) : current.toggle(task.id)
        );
      },
    }),
    [open, updateListedTask, moveTask]
  );

  const clearSelection = useCallback(() => {
    setSelection(Selection.EMPTY);
  }, []);

  const saveSelection = async (changes: TaskChanges) => {
    const ids = [...visible.ids];
    try {
      await updateTasks(ids, changes);
    } catch (failure) {
      const description = refusal(failure, chosen);
      toast.error(`Couldn't update ${quantity(ids.length)}`, { description });
      throw new Error(description, { cause: failure });
    }
    toast.success(`Updated ${quantity(ids.length)}`);
    setEditing(false);
    clearSelection();
  };

  const loading = tasksLoading || projectsLoading;
  const selecting = listing && !loading && visible.size > 0;
  const escapable = selecting && !editing;

  useEffect(() => {
    if (!escapable) return undefined;
    const clear = (event: KeyboardEvent) => {
      if (keepsSelection(event)) return;
      clearSelection();
    };
    window.addEventListener('keydown', clear);
    return () => window.removeEventListener('keydown', clear);
  }, [escapable, clearSelection]);

  const emptied = visible.size === 0;
  useEffect(() => {
    if (emptied && toggled.current !== null) returnFocus(toggled.current);
  }, [emptied]);

  const displayError =
    tasksError ||
    (editorFailed ? editorError : null) ||
    (workspace.error ? RELATIONS_FAILED : null);

  return (
    <div className="page page--workspace tasks-page">
      <PageBar title="Tasks" subtitle="Autonomous agent workflows">
        {listing ? (
          <>
            <div className="tasks-filters">
              <Select
                compact
                value={filterProject}
                onValueChange={filter}
                aria-label="Filter by project"
                disabled={loading}
                options={[
                  { value: ALL_PROJECTS, label: 'All Projects' },
                  ...projects.map((project) => ({ value: project.id, label: project.name })),
                ]}
              />
            </div>
            {writable ? (
              <Button
                onClick={() => setShowCreateModal(true)}
                disabled={loading || projects.length === 0}
              >
                <PlusIcon />
                New task
              </Button>
            ) : (
              <p className="tasks-access">{READ_ONLY}</p>
            )}
          </>
        ) : null}
      </PageBar>

      <div className="page-body tasks-body" data-selecting={selecting || undefined}>
        {displayError && (
          <div className="error-banner" role="alert">
            {displayError}
          </div>
        )}

        {showSpinner ? (
          <div className="loading" role="status">
            Loading task...
          </div>
        ) : editorReady && editorTask ? (
          <TaskDetail
            task={editorTask}
            tasks={known}
            projects={projects}
            sources={sources}
            focusTitle={followed === editorTask.id}
            onOpen={follow}
            onClose={closeEditor}
            onExecute={() => setExecutionTask(editorTask)}
            onSave={handleSave}
            writable={writableFor(editorTask)}
          />
        ) : loading ? (
          <Skeleton />
        ) : tasks.length > 0 ? (
          <Unfolded value={view.unfolded}>
            <Board
              tasks={tasks}
              known={known}
              projects={projectNames}
              renaming={renaming}
              removed={removed}
              view={view}
              selected={visible.ids}
              writable={writable}
              actions={actions}
            />
          </Unfolded>
        ) : tasksError ? null : (
          <EmptyState
            className="tasks-empty"
            icon={
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
                <path d="M9 12l2 2 4-4" />
              </svg>
            }
            title={filtered ? 'No tasks match' : 'No tasks yet'}
            description={
              filtered
                ? 'No tasks belong to this project yet'
                : writable
                  ? 'Create your first task to automate your workflow'
                  : READ_ONLY
            }
            action={
              filtered ? (
                <Button variant="secondary" onClick={() => filter(ALL_PROJECTS)}>
                  Show all tasks
                </Button>
              ) : writable ? (
                <Button onClick={() => setShowCreateModal(true)} disabled={projects.length === 0}>
                  Create Task
                </Button>
              ) : undefined
            }
          />
        )}
      </div>

      <CreateTaskWizard
        isOpen={writable && showCreateModal && projects.length > 0}
        onClose={() => setShowCreateModal(false)}
        onCreated={() => {}}
        createTask={createTask}
        projects={projects}
        sources={sources}
      />

      {executionTask && (
        <TaskExecutionView
          key={executionTask.id}
          task={executionTask}
          writable={writableFor(executionTask)}
          onClose={closeExecution}
        />
      )}

      {selecting && (
        <SelectionBar
          count={visible.size}
          onEdit={() => setEditing(true)}
          onClear={clearSelection}
        />
      )}

      {editing && writable && (
        <EditTasksDialog
          tasks={chosen}
          projects={projects}
          onSave={saveSelection}
          onClose={() => setEditing(false)}
        />
      )}

      <div className="sr-only tasks-announcement" aria-live="polite" aria-atomic="true">
        {announcement}
      </div>

      {writable && deleting && (
        <DeleteTaskDialog
          task={deleting}
          onConfirm={() => confirmDelete(deleting)}
          onClose={() => setDeleting(null)}
        />
      )}
    </div>
  );
}
