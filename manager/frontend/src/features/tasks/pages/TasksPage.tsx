import { useQuery } from '@tanstack/react-query';
import { Badge, Button, EmptyState } from '@zone/ui';
import { useState } from 'react';
import { client } from '../../../api/client';
import { useProjects } from '../../projects/hooks';
import { CreateTaskWizard } from '../components';
import { useTasks } from '../hooks';
import type { Task } from '../types';
import { TaskExecutionView } from './TaskExecutionView';
import './TasksPage.css';
import { useWorkspace } from '../../../shared/context';

function TaskStatusBadge({ status }: { status: string }) {
  const variants: Record<
    string,
    'secondary' | 'info' | 'warning' | 'destructive' | 'default' | 'success'
  > = {
    created: 'secondary',
    queued: 'info',
    in_progress: 'warning',
    blocked: 'destructive',
    review: 'default',
    complete: 'success',
  };
  return <Badge variant={variants[status] || 'secondary'}>{status.replace('_', ' ')}</Badge>;
}

function PrStatusBadge({ status }: { status: 'pending' | 'open' | 'merged' | 'closed' }) {
  const variants: Record<string, 'secondary' | 'success' | 'default' | 'destructive'> = {
    pending: 'secondary',
    open: 'success',
    merged: 'default',
    closed: 'destructive',
  };
  return <Badge variant={variants[status] || 'secondary'}>PR: {status}</Badge>;
}

export default function TasksPage() {
  const [filterProject, setFilterProject] = useState<string>('');
  const [filterStatus, setFilterStatus] = useState<string>('');

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
      <header className="tasks-header">
        <div>
          <h1 className="tasks-title">Tasks</h1>
          <p className="tasks-subtitle">Autonomous agent workflows</p>
        </div>
        <Button
          onClick={() => setShowCreateModal(true)}
          disabled={loading || projects.length === 0}
        >
          + New Task
        </Button>
      </header>

      <div className="tasks-workspace">
        {displayError && (
          <div className="rounded-md bg-destructive/10 border border-destructive/30 p-3 text-sm text-destructive mb-4">
            {displayError}
          </div>
        )}

        <div className="filters">
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

        {loading ? (
          <div className="tasks-list">
            {[1, 2, 3, 4].map((i) => (
              <div key={i} className="task-card skeleton-card">
                <div className="skeleton-header">
                  <div className="skeleton skeleton-title" />
                  <div className="skeleton skeleton-badge" />
                </div>
                <div className="skeleton skeleton-text short" />
                <div className="skeleton skeleton-text" />
                <div className="skeleton skeleton-text" style={{ width: '80%' }} />
                <div className="skeleton-meta">
                  <div className="skeleton skeleton-tag" />
                  <div className="skeleton skeleton-tag" />
                </div>
                <div className="skeleton-actions">
                  <div className="skeleton skeleton-btn" />
                  <div className="skeleton skeleton-btn" />
                </div>
              </div>
            ))}
          </div>
        ) : tasks.length === 0 ? (
          <EmptyState
            icon={
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                width="40"
                height="40"
              >
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
              <div
                key={task.id}
                className={`task-card ${task.is_agentic ? 'task-card-agentic' : ''}`}
              >
                <div className="task-card-header">
                  <h3>{task.title}</h3>
                  <div className="task-badges">
                    {task.is_agentic && <span className="task-agentic-badge">Agentic</span>}
                    <TaskStatusBadge status={task.status} />
                    {task.pr_status && <PrStatusBadge status={task.pr_status} />}
                  </div>
                </div>
                <p className="task-project">{getProjectNames(task.project_ids)}</p>
                <p className="task-description">{task.description}</p>
                {task.pr_url && (
                  <div className="task-pr-info">
                    <a
                      href={task.pr_url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="task-pr-link"
                    >
                      View Pull Request
                    </a>
                    {task.branch_name && (
                      <span className="task-branch">Branch: {task.branch_name}</span>
                    )}
                  </div>
                )}
                <div className="task-meta">
                  <span className="task-priority">Priority: {task.priority ?? 'N/A'}</span>
                  {task.model_name && <span className="task-model">Model: {task.model_name}</span>}
                  {task.is_agentic && task.source_id && (
                    <span className="task-source">
                      {sources.find((s) => s.id === task.source_id)?.name || 'Source'}
                    </span>
                  )}
                </div>
                <div className="task-actions">
                  <Button size="sm" onClick={() => setSelectedTask(task)}>
                    Execute
                  </Button>
                  <Button variant="destructive" size="sm" onClick={() => handleDeleteTask(task.id)}>
                    Delete
                  </Button>
                </div>
              </div>
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
        <TaskExecutionView
          key={selectedTask.id}
          task={selectedTask}
          onClose={() => setSelectedTask(null)}
        />
      )}
    </div>
  );
}
