import { useQuery } from '@tanstack/react-query';
import { Badge, Button, EmptyState } from '@zone/ui';
import { useEffect, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { client } from '../../../api/client';
import PageBar from '../../../shared/components/PageBar/PageBar';
import { useProjects } from '../../projects/hooks';
import { CreateTaskWizard } from '../components';
import { useTasks } from '../hooks';
import type { Task } from '../types';
import { TaskExecutionView } from './TaskExecutionView';
import './TasksPage.css';
import { useWorkspace } from '../../../shared/context';

type Tint = 'neutral' | 'info' | 'warning' | 'destructive' | 'accent' | 'success';

const STATUS_TINTS: Record<string, Tint> = {
  created: 'neutral',
  queued: 'info',
  in_progress: 'info',
  blocked: 'destructive',
  review: 'warning',
  complete: 'success',
};

const PR_TINTS: Record<string, Tint> = {
  pending: 'neutral',
  open: 'success',
  merged: 'accent',
  closed: 'destructive',
};

const SKELETON_CARDS = [1, 2, 3, 4];

function TaskStatusBadge({ status }: { status: string }) {
  return <Badge variant={STATUS_TINTS[status] ?? 'neutral'}>{status.replace('_', ' ')}</Badge>;
}

function PrStatusBadge({ status }: { status: 'pending' | 'open' | 'merged' | 'closed' }) {
  return <Badge variant={PR_TINTS[status] ?? 'neutral'}>PR: {status}</Badge>;
}

export default function TasksPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const [filterProject, setFilterProject] = useState<string>('');
  const [filterStatus, setFilterStatus] = useState<string>('');
  const linkedTaskId = searchParams.get('id');

  const {
    tasks,
    loading: tasksLoading,
    error: tasksError,
    createTask,
    deleteTask: deleteTaskMutation,
  } = useTasks(filterProject || undefined, filterStatus || undefined);

  const { projects, loading: projectsLoading } = useProjects('all');
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const { data: sources = [] } = useQuery({
    queryKey: ['sources', workspaceId],
    queryFn: () => client.getSources(workspaceId as string),
    enabled: !!workspaceId,
  });

  const [selectedTask, setSelectedTask] = useState<Task | null>(null);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!linkedTaskId) return;
    const found = tasks.find((task) => task.id === linkedTaskId);
    if (found) {
      setSelectedTask(found);
    }
  }, [linkedTaskId, tasks]);

  const closeSelectedTask = () => {
    setSelectedTask(null);
    if (searchParams.get('id')) {
      const next = new URLSearchParams(searchParams);
      next.delete('id');
      setSearchParams(next, { replace: true });
    }
  };

  const handleTaskCreated = async (_task: Task) => {
    // Task is already added to the list by the createTask hook
  };

  const handleDeleteTask = async (taskId: string) => {
    if (!window.confirm('Are you sure you want to delete this task?')) return;

    try {
      await deleteTaskMutation(taskId);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete task');
    }
  };

  const loading = tasksLoading || projectsLoading;
  const displayError = tasksError || error;

  const getProjectNames = (projectIds: string[]) => {
    if (!projectIds || projectIds.length === 0) return 'No projects';
    return projectIds.map((id) => projects.find((p) => p.id === id)?.name || 'Unknown').join(', ');
  };

  return (
    <div className="page page--workspace tasks-page">
      <PageBar title="Tasks" subtitle="Autonomous agent workflows">
        <div className="tasks-filters">
          <select
            value={filterProject}
            onChange={(e) => setFilterProject(e.target.value)}
            aria-label="Filter by project"
            disabled={loading}
          >
            <option value="">All Projects</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          <select
            value={filterStatus}
            onChange={(e) => setFilterStatus(e.target.value)}
            aria-label="Filter by status"
            disabled={loading}
          >
            <option value="">All Statuses</option>
            <option value="created">Created</option>
            <option value="queued">Queued</option>
            <option value="in_progress">In Progress</option>
            <option value="blocked">Blocked</option>
            <option value="review">Review</option>
            <option value="complete">Complete</option>
          </select>
        </div>
        <Button
          onClick={() => setShowCreateModal(true)}
          disabled={loading || projects.length === 0}
        >
          + New Task
        </Button>
      </PageBar>

      <div className="page-body tasks-body">
        {displayError && (
          <div className="error-banner" role="alert">
            {displayError}
          </div>
        )}

        {loading ? (
          <div className="tasks-list">
            {SKELETON_CARDS.map((i) => (
              <div key={i} className="task-card skeleton-card">
                <div className="task-card-title">
                  <div className="skeleton skeleton-title" />
                  <div className="skeleton skeleton-badge" />
                </div>
                <div className="skeleton skeleton-text short" />
                <div className="task-description">
                  <div className="skeleton skeleton-text" />
                  <div className="skeleton skeleton-text" style={{ width: '80%' }} />
                </div>
                <div className="task-meta">
                  <div className="skeleton skeleton-tag" />
                  <div className="skeleton skeleton-tag" />
                </div>
                <div className="task-actions">
                  <div className="skeleton skeleton-btn" />
                  <div className="skeleton skeleton-btn" />
                </div>
              </div>
            ))}
          </div>
        ) : tasks.length === 0 ? (
          <EmptyState
            icon={
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
                <path d="M9 12l2 2 4-4" />
              </svg>
            }
            title="No tasks yet"
            description="Create your first task to automate your workflow"
            action={<Button onClick={() => setShowCreateModal(true)}>Create Task</Button>}
          />
        ) : (
          <div className="tasks-list">
            {tasks.map((task) => (
              <article
                key={task.id}
                className={`task-card ${task.is_agentic ? 'task-card-agentic' : ''}`}
              >
                <div className="task-card-title">
                  <h3 title={task.title}>{task.title}</h3>
                  <div className="task-badges">
                    {task.is_agentic && <Badge variant="accent">Agentic</Badge>}
                    <TaskStatusBadge status={task.status} />
                    {task.pr_status && <PrStatusBadge status={task.pr_status} />}
                  </div>
                </div>
                <p className="task-project">{getProjectNames(task.project_ids)}</p>
                <p className="task-description">{task.description}</p>
                <div className="task-meta">
                  <span className="task-priority">Priority: {task.priority ?? 'N/A'}</span>
                  {task.model_name && <span className="task-model">Model: {task.model_name}</span>}
                  {task.is_agentic && task.source_id && (
                    <span className="task-source">
                      {sources.find((s) => s.id === task.source_id)?.name || 'Source'}
                    </span>
                  )}
                  {task.pr_url && (
                    <span className="task-pr">
                      <a
                        href={task.pr_url}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="task-pr-link"
                      >
                        View PR
                      </a>
                      {task.branch_name && (
                        <code className="task-branch" title={task.branch_name}>
                          {task.branch_name}
                        </code>
                      )}
                    </span>
                  )}
                </div>
                <div className="task-actions">
                  <Button size="sm" variant="secondary" onClick={() => setSelectedTask(task)}>
                    Execute
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="task-delete"
                    onClick={() => handleDeleteTask(task.id)}
                  >
                    Delete
                  </Button>
                </div>
              </article>
            ))}
          </div>
        )}
      </div>

      <CreateTaskWizard
        isOpen={showCreateModal && projects.length > 0}
        onClose={() => setShowCreateModal(false)}
        onCreated={handleTaskCreated}
        createTask={createTask}
        projects={projects}
        sources={sources}
      />

      {selectedTask && (
        <TaskExecutionView key={selectedTask.id} task={selectedTask} onClose={closeSelectedTask} />
      )}
    </div>
  );
}
