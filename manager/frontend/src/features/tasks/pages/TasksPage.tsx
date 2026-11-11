import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Button, EmptyState, Select } from '@zone/ui';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { toast } from 'sonner';
import { client } from '../../../api/client';
import PageBar from '../../../shared/components/PageBar/PageBar';
import PlusIcon from '../../../shared/components/PlusIcon/PlusIcon';
import { useWorkspace } from '../../../shared/context';
import { useProjects } from '../../projects/hooks';
import { CreateTaskWizard, DeleteTaskDialog, TaskDetail } from '../components';
import { Board, Skeleton, View } from '../components/board';
import { TASKS_KEY, useTask, useTasks } from '../hooks';
import { TitleDraft } from '../TitleDraft';
import type { BoardActions, Renaming, Task, UpdateTaskRequest } from '../types';
import { TaskExecutionView } from './TaskExecutionView';
import './TasksPage.css';

const ALL_PROJECTS = '';

const MOVE_FAILED = 'The task could not be moved';

export default function TasksPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const queryClient = useQueryClient();
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const [scope, setScope] = useState(workspaceId);
  const [filterProject, setFilterProject] = useState(ALL_PROJECTS);
  const [renaming, setRenaming] = useState<Renaming | null>(null);
  const [view, setView] = useState(() => new View());
  const linkedTaskId = searchParams.get('id');
  const taskParam = searchParams.get('task');

  if (scope !== workspaceId) {
    setScope(workspaceId);
    setFilterProject(ALL_PROJECTS);
    setRenaming(null);
    setView(new View());
  }

  const {
    tasks,
    loading: tasksLoading,
    error: tasksError,
    createTask,
    updateTask: updateListedTask,
    moveTask,
    deleteTask,
  } = useTasks(filterProject || undefined);

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
  const [showCreateModal, setShowCreateModal] = useState(false);

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

  const closeEditor = () => {
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
      open: (task) => {
        view.opened = task.id;
        openEditor(task.id);
      },
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
    }),
    [openEditor, updateListedTask, moveTask, view]
  );

  const loading = tasksLoading || projectsLoading;
  const displayError = tasksError || (editorFailed ? editorError : null);
  const filtered = filterProject !== ALL_PROJECTS;

  return (
    <div className="page page--workspace tasks-page">
      <PageBar title="Tasks" subtitle="Autonomous agent workflows">
        {listing ? (
          <>
            <div className="tasks-filters">
              <Select
                compact
                value={filterProject}
                onValueChange={setFilterProject}
                aria-label="Filter by project"
                disabled={loading}
                options={[
                  { value: ALL_PROJECTS, label: 'All Projects' },
                  ...projects.map((project) => ({ value: project.id, label: project.name })),
                ]}
              />
            </div>
            <Button
              onClick={() => setShowCreateModal(true)}
              disabled={loading || projects.length === 0}
            >
              <PlusIcon />
              New task
            </Button>
          </>
        ) : null}
      </PageBar>

      <div className="page-body tasks-body">
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
            projects={projects}
            sources={sources}
            onClose={closeEditor}
            onExecute={() => setExecutionTask(editorTask)}
            onSave={handleSave}
          />
        ) : loading ? (
          <Skeleton />
        ) : tasks.length > 0 ? (
          <Board
            tasks={tasks}
            projects={projectNames}
            renaming={renaming}
            removed={removed}
            view={view}
            actions={actions}
          />
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
                : 'Create your first task to automate your workflow'
            }
            action={
              filtered ? (
                <Button variant="secondary" onClick={() => setFilterProject(ALL_PROJECTS)}>
                  Show all tasks
                </Button>
              ) : (
                <Button onClick={() => setShowCreateModal(true)} disabled={projects.length === 0}>
                  Create Task
                </Button>
              )
            }
          />
        )}
      </div>

      <CreateTaskWizard
        isOpen={showCreateModal && projects.length > 0}
        onClose={() => setShowCreateModal(false)}
        onCreated={() => {}}
        createTask={createTask}
        projects={projects}
        sources={sources}
      />

      {executionTask && (
        <TaskExecutionView key={executionTask.id} task={executionTask} onClose={closeExecution} />
      )}

      {deleting && (
        <DeleteTaskDialog
          task={deleting}
          onConfirm={() => confirmDelete(deleting)}
          onClose={() => setDeleting(null)}
        />
      )}
    </div>
  );
}
