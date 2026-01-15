import { useEffect, useRef, useState, useCallback } from 'react';
import { useQuery } from '@tanstack/react-query';
import { client } from '../../../api/client';
import { useTasks } from '../hooks';
import { useProjects } from '../../projects/hooks';
import { tasksApi } from '../../../api/tasks';
import { CreateTaskWizard } from '../components';
import { Button, Badge, EmptyState } from '@zone/ui';
import type { Task, TaskProgressMessage } from '../types';
import './TasksPage.css';

// Note: Using client.getSources() since there's no sources feature API yet.
// When a sources feature is created, import from there instead.

// Phase display names for progress visualization
const PHASES: Record<string, { name: string; progress: number }> = {
  architect_planning: { name: 'Architect Planning', progress: 15 },
  developer_tests: { name: 'Writing Tests', progress: 30 },
  developer_implementation: { name: 'Implementing', progress: 50 },
  griller_review: { name: 'Code Review', progress: 65 },
  developer_fixes: { name: 'Fixing Issues', progress: 80 },
  architect_review: { name: 'Final Review', progress: 90 },
  developer_final: { name: 'Finalizing', progress: 100 },
};

function TaskStatusBadge({ status }: { status: string }) {
  const variants: Record<string, 'secondary' | 'info' | 'warning' | 'destructive' | 'default' | 'success'> = {
    created: 'secondary',
    queued: 'info',
    in_progress: 'warning',
    blocked: 'destructive',
    review: 'default',
    complete: 'success',
  };
  return (
    <Badge variant={variants[status] || 'secondary'}>
      {status.replace('_', ' ')}
    </Badge>
  );
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

function TaskProgressBar({ progress, phase }: { progress: number; phase: string | null }) {
  return (
    <div className="task-progress-bar-container">
      <div className="task-progress-bar" style={{ width: `${progress}%` }} />
      {phase && (
        <span className="task-progress-label">
          {PHASES[phase]?.name || phase} ({progress}%)
        </span>
      )}
    </div>
  );
}

interface TaskExecutionViewProps {
  task: Task;
  onClose: () => void;
}

function TaskExecutionView({ task, onClose }: TaskExecutionViewProps) {
  const [progress, setProgress] = useState(0);
  const [currentPhase, setCurrentPhase] = useState<string | null>(null);
  const [logs, setLogs] = useState<
    { phase: string; agent: string; level: string; message: string }[]
  >([]);
  const [status, setStatus] = useState<'idle' | 'running' | 'complete' | 'error'>('idle');
  const [error, setError] = useState<string | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const logsEndRef = useRef<HTMLDivElement>(null);

  const startExecution = useCallback(async () => {
    try {
      setStatus('running');
      setError(null);
      setLogs([]);
      setProgress(0);

      const result = await tasksApi.runTask(task.id);

      // Connect to WebSocket for progress updates
      const ws = tasksApi.createTaskWebSocket(result.run_id);
      wsRef.current = ws;

      ws.onmessage = (event) => {
        const msg: TaskProgressMessage = JSON.parse(event.data);

        switch (msg.type) {
          case 'phase_started':
          case 'phase_completed':
            if (msg.progress_percent !== undefined) {
              setProgress(msg.progress_percent);
            }
            if (msg.phase) {
              setCurrentPhase(msg.phase);
            }
            break;
          case 'log':
            setLogs((prev) => [
              ...prev,
              {
                phase: msg.phase || '',
                agent: msg.agent_type || '',
                level: msg.log_level || 'info',
                message: msg.message || '',
              },
            ]);
            break;
          case 'complete':
            setStatus('complete');
            setProgress(100);
            ws.close();
            break;
          case 'error':
            setStatus('error');
            setError(msg.error || 'Unknown error');
            ws.close();
            break;
        }
      };

      ws.onerror = () => {
        setStatus('error');
        setError('WebSocket connection failed');
      };
    } catch (err) {
      setStatus('error');
      setError(err instanceof Error ? err.message : 'Failed to start task');
    }
  }, [task.id]);

  const stopExecution = useCallback(async () => {
    if (wsRef.current) {
      wsRef.current.close();
    }
    try {
      await tasksApi.cancelTaskRun(task.id);
      setStatus('idle');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to stop task');
    }
  }, [task.id]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: logs.length triggers auto-scroll when new logs arrive
  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [logs.length]);

  useEffect(() => {
    return () => {
      if (wsRef.current) {
        wsRef.current.close();
      }
    };
  }, []);

  return (
    <div className="task-execution-overlay">
      <div className="task-execution-modal">
        <header className="task-execution-header">
          <h2>{task.title}</h2>
          <Button variant="ghost" size="icon" onClick={onClose} aria-label="Close">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" width="20" height="20">
              <path d="M6 18L18 6M6 6l12 12" />
            </svg>
          </Button>
        </header>

        <div className="task-execution-content">
          <div className="task-meta">
            <TaskStatusBadge status={task.status} />
            <p className="task-description">{task.description}</p>
          </div>

          <div className="execution-controls">
            {status === 'idle' && (
              <Button onClick={startExecution}>
                Start Execution
              </Button>
            )}
            {status === 'running' && (
              <Button variant="destructive" onClick={stopExecution}>
                Stop Execution
              </Button>
            )}
            {(status === 'complete' || status === 'error') && (
              <Button onClick={startExecution}>
                Run Again
              </Button>
            )}
          </div>

          {status !== 'idle' && (
            <>
              <TaskProgressBar progress={progress} phase={currentPhase} />

              <div className="execution-phases">
                {Object.entries(PHASES).map(([key, { name, progress: phaseProgress }]) => (
                  <div
                    key={key}
                    className={`phase-item ${
                      progress >= phaseProgress
                        ? 'phase-complete'
                        : currentPhase === key
                          ? 'phase-active'
                          : 'phase-pending'
                    }`}
                  >
                    <span className="phase-indicator" />
                    <span className="phase-name">{name}</span>
                  </div>
                ))}
              </div>

              <div className="execution-logs">
                <h3>Execution Logs</h3>
                <div className="logs-container">
                  {logs.map((log, index) => (
                    <div key={`${log.phase}-${index}`} className={`log-entry log-${log.level}`}>
                      <span className="log-phase">[{PHASES[log.phase]?.name || log.phase}]</span>
                      <span className="log-agent">{log.agent}</span>
                      <span className="log-message">{log.message}</span>
                    </div>
                  ))}
                  <div ref={logsEndRef} />
                </div>
              </div>

              {status === 'complete' && (
                <div className="execution-result success">Task completed successfully!</div>
              )}
              {status === 'error' && <div className="execution-result error">Error: {error}</div>}
            </>
          )}
        </div>
      </div>
    </div>
  );
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
  const { data: sources = [] } = useQuery({
    queryKey: ['sources'],
    queryFn: () => client.getSources('00000000-0000-0000-0000-000000000001'),
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
    return projectIds
      .map((id) => projects.find((p) => p.id === id)?.name || 'Unknown')
      .join(', ');
  };

  return (
    <div className="page tasks-page">
      <header className="flex justify-between items-start mb-6">
        <div>
          <h1 className="text-2xl font-semibold text-foreground">Tasks</h1>
          <p className="text-muted-foreground mt-1">Autonomous agent workflows</p>
        </div>
        <Button
          onClick={() => setShowCreateModal(true)}
          disabled={loading || projects.length === 0}
        >
          + New Task
        </Button>
      </header>

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
              width="48"
              height="48"
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

      <CreateTaskWizard
        isOpen={showCreateModal && projects.length > 0}
        onClose={() => setShowCreateModal(false)}
        onCreated={handleTaskCreated}
        createTask={createTask}
        projects={projects}
        sources={sources}
      />

      {selectedTask && (
        <TaskExecutionView task={selectedTask} onClose={() => setSelectedTask(null)} />
      )}
    </div>
  );
}
