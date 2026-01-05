import { useCallback, useEffect, useRef, useState } from 'react';
import { client } from '../api/client';
import type { CreateTaskRequest, Project, Source, Task, TaskProgressMessage } from '../types';
import { CreateTaskRequestSchema, getErrors } from '../validation';
import './TasksPage.css';

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
  const colors: Record<string, string> = {
    created: 'badge-gray',
    queued: 'badge-blue',
    in_progress: 'badge-yellow',
    blocked: 'badge-red',
    review: 'badge-purple',
    complete: 'badge-green',
  };
  return (
    <span className={`task-status-badge ${colors[status] || 'badge-gray'}`}>
      {status.replace('_', ' ')}
    </span>
  );
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

      const result = await client.startTask(task.id);

      // Connect to WebSocket for progress updates
      const ws = client.createTaskWebSocket(result.run_id);
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
      await client.stopTask(task.id);
      setStatus('idle');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to stop task');
    }
  }, [task.id]);

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
          <button type="button" onClick={onClose} className="close-btn" aria-label="Close">
            &times;
          </button>
        </header>

        <div className="task-execution-content">
          <div className="task-meta">
            <TaskStatusBadge status={task.status} />
            <p className="task-description">{task.description}</p>
          </div>

          <div className="execution-controls">
            {status === 'idle' && (
              <button type="button" onClick={startExecution} className="btn btn-primary">
                Start Execution
              </button>
            )}
            {status === 'running' && (
              <button type="button" onClick={stopExecution} className="btn btn-danger">
                Stop Execution
              </button>
            )}
            {(status === 'complete' || status === 'error') && (
              <button type="button" onClick={startExecution} className="btn btn-primary">
                Run Again
              </button>
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

interface CreateTaskModalProps {
  projects: Project[];
  sources: Source[];
  onClose: () => void;
  onCreated: (task: Task) => void;
}

function CreateTaskModal({ projects, sources, onClose, onCreated }: CreateTaskModalProps) {
  const [projectId, setProjectId] = useState(projects[0]?.id || '');
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [criteria, setCriteria] = useState('');
  const [priority, setPriority] = useState(1);
  const [isAgentic, setIsAgentic] = useState(false);
  const [sourceId, setSourceId] = useState<string>('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  // Get selected project to show its linked source
  const selectedProject = projects.find((p) => p.id === projectId);
  const projectSource = sources.find((s) => s.id === selectedProject?.source_id);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    const request: CreateTaskRequest = {
      project_id: projectId,
      title,
      description,
      priority,
      is_agentic: isAgentic,
    };
    if (criteria) {
      request.acceptance_criteria = criteria;
    }
    if (isAgentic && sourceId) {
      request.source_id = sourceId;
    }

    const errors = getErrors(CreateTaskRequestSchema, request);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setLoading(true);
    setError(null);

    try {
      const task = await client.createTask(request);
      onCreated(task);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create task');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="modal-overlay">
      <div className="modal-content">
        <header className="modal-header">
          <h2>Create New Task</h2>
          <button type="button" onClick={onClose} className="close-btn" aria-label="Close">
            &times;
          </button>
        </header>

        <form onSubmit={handleSubmit}>
          <div className="form-group">
            <label htmlFor="project">Project</label>
            <select
              id="project"
              value={projectId}
              onChange={(e) => setProjectId(e.target.value)}
              required
            >
              {projects.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </div>

          <div className="form-group">
            <label htmlFor="title">Title</label>
            <input
              type="text"
              id="title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="What needs to be done?"
              className={fieldErrors.title ? 'input-error' : ''}
            />
            {fieldErrors.title && <span className="field-error">{fieldErrors.title}</span>}
          </div>

          <div className="form-group">
            <label htmlFor="description">Description</label>
            <textarea
              id="description"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Detailed description of the task..."
              rows={4}
              className={fieldErrors.description ? 'input-error' : ''}
            />
            {fieldErrors.description && (
              <span className="field-error">{fieldErrors.description}</span>
            )}
          </div>

          <div className="form-group">
            <label htmlFor="criteria">Acceptance Criteria (optional)</label>
            <textarea
              id="criteria"
              value={criteria}
              onChange={(e) => setCriteria(e.target.value)}
              placeholder="How will we know when this task is complete?"
              rows={3}
            />
          </div>

          <div className="form-group">
            <label htmlFor="priority">Priority (1-5)</label>
            <input
              type="number"
              id="priority"
              value={priority}
              onChange={(e) => setPriority(Number.parseInt(e.target.value, 10))}
              min={1}
              max={5}
            />
          </div>

          <div className="form-group form-checkbox">
            <label htmlFor="isAgentic">
              <input
                type="checkbox"
                id="isAgentic"
                checked={isAgentic}
                onChange={(e) => setIsAgentic(e.target.checked)}
              />
              Enable Agentic Mode
            </label>
            <span className="form-hint">
              Allow this task to autonomously read/write code and query the knowledge base
            </span>
          </div>

          {isAgentic && (
            <div className="form-group">
              <label htmlFor="sourceId">Code Source</label>
              <select id="sourceId" value={sourceId} onChange={(e) => setSourceId(e.target.value)}>
                <option value="">
                  {projectSource
                    ? `Use project source (${projectSource.name})`
                    : 'Select a source...'}
                </option>
                {sources
                  .filter((s) => s.is_active)
                  .map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.name} ({s.source_type})
                    </option>
                  ))}
              </select>
              {projectSource && (
                <span className="form-hint">
                  Project uses: {projectSource.name} ({projectSource.source_type})
                </span>
              )}
              {!projectSource && sources.length === 0 && (
                <span className="form-hint form-hint-warning">
                  No sources configured. Add one in Sources page.
                </span>
              )}
            </div>
          )}

          {error && <div className="form-error">{error}</div>}

          <div className="form-actions">
            <button type="button" onClick={onClose} className="btn btn-secondary">
              Cancel
            </button>
            <button type="submit" className="btn btn-primary" disabled={loading}>
              {loading ? 'Creating...' : 'Create Task'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

export default function TasksPage() {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [sources, setSources] = useState<Source[]>([]);
  const [selectedTask, setSelectedTask] = useState<Task | null>(null);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filterProject, setFilterProject] = useState<string>('');
  const [filterStatus, setFilterStatus] = useState<string>('');

  const loadData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [tasksData, projectsData, sourcesData] = await Promise.all([
        client.getTasks(filterProject || undefined, filterStatus || undefined),
        client.getProjects(),
        client.getSources(),
      ]);
      setTasks(tasksData);
      setProjects(projectsData);
      setSources(sourcesData);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load data');
    } finally {
      setLoading(false);
    }
  }, [filterProject, filterStatus]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const handleTaskCreated = (task: Task) => {
    setTasks((prev) => [task, ...prev]);
  };

  const handleDeleteTask = async (taskId: string) => {
    if (!window.confirm('Are you sure you want to delete this task?')) return;

    try {
      await client.deleteTask(taskId);
      setTasks((prev) => prev.filter((t) => t.id !== taskId));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete task');
    }
  };

  const getProjectName = (projectId: string) => {
    const project = projects.find((p) => p.id === projectId);
    return project?.name || 'Unknown Project';
  };

  return (
    <div className="page tasks-page">
      <header className="page-header">
        <div className="header-content">
          <h1>Tasks</h1>
          <p className="subtitle">Autonomous agent workflows</p>
        </div>
        <button
          type="button"
          className="btn btn-primary"
          onClick={() => setShowCreateModal(true)}
          disabled={loading || projects.length === 0}
        >
          + New Task
        </button>
      </header>

      {error && <div className="error-banner">{error}</div>}

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
        <div className="empty-state">
          <p>No tasks found. Create a task to get started!</p>
        </div>
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
                </div>
              </div>
              <p className="task-project">{getProjectName(task.project_id)}</p>
              <p className="task-description">{task.description}</p>
              <div className="task-meta">
                <span className="task-priority">Priority: {task.priority}</span>
                {task.model_name && <span className="task-model">Model: {task.model_name}</span>}
                {task.is_agentic && task.source_id && (
                  <span className="task-source">
                    {sources.find((s) => s.id === task.source_id)?.name || 'Source'}
                  </span>
                )}
              </div>
              <div className="task-actions">
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => setSelectedTask(task)}
                >
                  Execute
                </button>
                <button
                  type="button"
                  className="btn btn-danger"
                  onClick={() => handleDeleteTask(task.id)}
                >
                  Delete
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {showCreateModal && projects.length > 0 && (
        <CreateTaskModal
          projects={projects}
          sources={sources}
          onClose={() => setShowCreateModal(false)}
          onCreated={handleTaskCreated}
        />
      )}

      {selectedTask && (
        <TaskExecutionView task={selectedTask} onClose={() => setSelectedTask(null)} />
      )}
    </div>
  );
}
