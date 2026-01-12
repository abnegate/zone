import { useState, useCallback, useMemo } from 'react';
import { Wizard } from '../../../components';
import type { WizardStep } from '@zone/ui';
import type { CreateTaskRequest, Task } from '../types';
import type { Project } from '../../projects/types';
import type { Source } from '../../../types';
import { getErrors } from '../../../validation';
import { CreateTaskRequestSchema } from '../schemas';

interface CreateTaskWizardProps {
  isOpen: boolean;
  onClose: () => void;
  onCreated: (task: Task) => void;
  createTask: (request: CreateTaskRequest) => Promise<Task>;
  projects: Project[];
  sources: Source[];
}

const WIZARD_STEPS: WizardStep[] = [
  {
    id: 'project',
    title: 'Project',
    description: 'Select a project',
  },
  {
    id: 'details',
    title: 'Task Details',
    description: 'Title and description',
  },
  {
    id: 'settings',
    title: 'Settings',
    description: 'Priority and options',
  },
];

export function CreateTaskWizard({
  isOpen,
  onClose,
  onCreated,
  createTask,
  projects,
  sources,
}: CreateTaskWizardProps) {
  const [currentStep, setCurrentStep] = useState(0);
  const [projectId, setProjectId] = useState(projects[0]?.id || '');
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [criteria, setCriteria] = useState('');
  const [priority, setPriority] = useState(3);
  const [isAgentic, setIsAgentic] = useState(false);
  const [sourceId, setSourceId] = useState<string>('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  // Get selected project to show its linked source
  const selectedProject = useMemo(
    () => projects.find((p) => p.id === projectId),
    [projects, projectId]
  );
  const projectSource = useMemo(
    () => sources.find((s) => s.id === selectedProject?.source_id),
    [sources, selectedProject]
  );

  const handleStepChange = useCallback((step: number) => {
    setCurrentStep(step);
    setError(null);
  }, []);

  const canProceed = useMemo(() => {
    if (currentStep === 0) {
      return !!projectId;
    }
    if (currentStep === 1) {
      return title.trim().length > 0 && description.trim().length > 0;
    }
    return true;
  }, [currentStep, projectId, title, description]);

  const handleComplete = useCallback(async () => {
    const request: CreateTaskRequest = {
      project_id: projectId,
      title: title.trim(),
      description: description.trim(),
      priority,
      is_agentic: isAgentic,
    };
    if (criteria.trim()) {
      request.acceptance_criteria = criteria.trim();
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
      const task = await createTask(request);
      onCreated(task);
      handleClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create task');
    } finally {
      setLoading(false);
    }
  }, [projectId, title, description, criteria, priority, isAgentic, sourceId, createTask, onCreated]);

  const handleClose = useCallback(() => {
    setCurrentStep(0);
    setProjectId(projects[0]?.id || '');
    setTitle('');
    setDescription('');
    setCriteria('');
    setPriority(3);
    setIsAgentic(false);
    setSourceId('');
    setError(null);
    setFieldErrors({});
    onClose();
  }, [onClose, projects]);

  const activeSources = useMemo(() => sources.filter((s) => s.is_active), [sources]);

  const renderStepContent = () => {
    switch (currentStep) {
      case 0:
        return (
          <div className="wizard-step-content">
            <p className="wizard-step-intro">
              Select the project this task belongs to. Tasks are organized under projects.
            </p>
            {projects.length === 0 ? (
              <div className="wizard-empty-state">
                <p>No projects available.</p>
                <p className="wizard-empty-hint">
                  Create a project first from the Projects page.
                </p>
              </div>
            ) : (
              <div className="project-selection-grid">
                {projects.map((project) => (
                  <button
                    key={project.id}
                    type="button"
                    className={`project-selection-option ${projectId === project.id ? 'selected' : ''}`}
                    onClick={() => setProjectId(project.id)}
                  >
                    <div className={`project-status-dot status-${project.status}`} />
                    <div className="project-selection-info">
                      <span className="project-selection-name">{project.name}</span>
                      {project.description && (
                        <span className="project-selection-desc">{project.description}</span>
                      )}
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>
        );

      case 1:
        return (
          <div className="wizard-step-content">
            <p className="wizard-step-intro">
              Describe what needs to be done. Be specific about the task requirements.
            </p>
            <div className="form-group">
              <label htmlFor="task-title">Title</label>
              <input
                type="text"
                id="task-title"
                value={title}
                onChange={(e) => {
                  setTitle(e.target.value);
                  if (fieldErrors.title) {
                    setFieldErrors((prev) => {
                      const next = { ...prev };
                      delete next.title;
                      return next;
                    });
                  }
                }}
                placeholder="What needs to be done?"
                className={fieldErrors.title ? 'input-error' : ''}
                autoFocus
              />
              {fieldErrors.title && <span className="field-error">{fieldErrors.title}</span>}
            </div>
            <div className="form-group">
              <label htmlFor="task-description">Description</label>
              <textarea
                id="task-description"
                value={description}
                onChange={(e) => {
                  setDescription(e.target.value);
                  if (fieldErrors.description) {
                    setFieldErrors((prev) => {
                      const next = { ...prev };
                      delete next.description;
                      return next;
                    });
                  }
                }}
                placeholder="Detailed description of the task..."
                rows={4}
                className={fieldErrors.description ? 'input-error' : ''}
              />
              {fieldErrors.description && (
                <span className="field-error">{fieldErrors.description}</span>
              )}
            </div>
            <div className="form-group">
              <label htmlFor="task-criteria">
                Acceptance Criteria
                <span className="label-optional">optional</span>
              </label>
              <textarea
                id="task-criteria"
                value={criteria}
                onChange={(e) => setCriteria(e.target.value)}
                placeholder="How will we know when this task is complete?"
                rows={3}
              />
            </div>
          </div>
        );

      case 2:
        return (
          <div className="wizard-step-content">
            <p className="wizard-step-intro">
              Configure task priority and enable agentic mode for autonomous execution.
            </p>
            <div className="form-group">
              <label>Priority</label>
              <div className="priority-selector">
                {[1, 2, 3, 4, 5].map((p) => (
                  <button
                    key={p}
                    type="button"
                    className={`priority-option ${priority === p ? 'selected' : ''}`}
                    onClick={() => setPriority(p)}
                  >
                    <span className="priority-number">{p}</span>
                    <span className="priority-label">
                      {p === 1 ? 'Lowest' : p === 2 ? 'Low' : p === 3 ? 'Medium' : p === 4 ? 'High' : 'Highest'}
                    </span>
                  </button>
                ))}
              </div>
            </div>

            <div className="form-group">
              <label className="toggle-label">
                <span className="toggle-wrapper">
                  <input
                    type="checkbox"
                    checked={isAgentic}
                    onChange={(e) => setIsAgentic(e.target.checked)}
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

            {isAgentic && (
              <div className="form-group">
                <label htmlFor="task-source">Code Source</label>
                <select
                  id="task-source"
                  value={sourceId}
                  onChange={(e) => setSourceId(e.target.value)}
                  className="ui-select"
                >
                  <option value="">
                    {projectSource
                      ? `Use project source (${projectSource.name})`
                      : 'Select a source...'}
                  </option>
                  {activeSources.map((s) => (
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
                {!projectSource && activeSources.length === 0 && (
                  <span className="form-hint form-hint-warning">
                    No sources configured. Add one in Sources page.
                  </span>
                )}
              </div>
            )}
          </div>
        );

      default:
        return null;
    }
  };

  return (
    <Wizard
      isOpen={isOpen}
      onClose={handleClose}
      title="New Task"
      subtitle="Create a new task for autonomous execution"
      steps={WIZARD_STEPS}
      currentStep={currentStep}
      onStepChange={handleStepChange}
      onComplete={handleComplete}
      onCancel={handleClose}
      completeLabel={loading ? 'Creating...' : 'Create Task'}
      loading={loading}
      canProceed={canProceed}
      allowStepClick
    >
      {renderStepContent()}
      {Object.keys(fieldErrors).length > 0 && (
        <div className="form-error">
          {Object.entries(fieldErrors).map(([field, message]) => (
            <div key={field}>{message}</div>
          ))}
        </div>
      )}
      {error && <div className="form-error">{error}</div>}
    </Wizard>
  );
}

export default CreateTaskWizard;
