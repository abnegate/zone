import type { WizardStep } from '@zone/ui';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { client } from '../../../api/client';
import { Wizard } from '../../../components';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { Source } from '../../../types';
import { getErrors } from '../../../validation';
import { CreateProjectRequestSchema } from '../schemas';
import type { CreateProjectRequest, Project, ProjectStatus } from '../types';

interface CreateProjectWizardProps {
  isOpen: boolean;
  onClose: () => void;
  onCreated: (project: Project) => void;
  createProject: (request: CreateProjectRequest) => Promise<Project>;
}

const WIZARD_STEPS: WizardStep[] = [
  {
    id: 'basics',
    title: 'Project Details',
    description: 'Name and description',
  },
  {
    id: 'source',
    title: 'Source',
    description: 'Link a data source',
  },
  {
    id: 'status',
    title: 'Status',
    description: 'Set initial status',
  },
];

const statusLabels: Record<ProjectStatus, string> = {
  active: 'Active',
  on_hold: 'On Hold',
  cancelled: 'Cancelled',
};

const statusDescriptions: Record<ProjectStatus, string> = {
  active: 'Project is actively being worked on',
  on_hold: 'Project is temporarily paused',
  cancelled: 'Project has been cancelled',
};

export function CreateProjectWizard({
  isOpen,
  onClose,
  onCreated,
  createProject,
}: CreateProjectWizardProps) {
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const [currentStep, setCurrentStep] = useState(0);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [status, setStatus] = useState<ProjectStatus>('active');
  const [sourceId, setSourceId] = useState('');
  const [sources, setSources] = useState<Source[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [sourcesLoading, setSourcesLoading] = useState(false);

  // Load sources when wizard opens
  useEffect(() => {
    if (isOpen && workspaceId) {
      setSourcesLoading(true);
      client
        .getSources(workspaceId)
        .then(setSources)
        .catch((err) => console.error('Failed to load sources:', err))
        .finally(() => setSourcesLoading(false));
    }
  }, [isOpen, workspaceId]);

  const handleStepChange = useCallback((step: number) => {
    setCurrentStep(step);
    setError(null);
  }, []);

  const canProceed = useMemo(() => {
    if (currentStep === 0) {
      return name.trim().length > 0;
    }
    return true;
  }, [currentStep, name]);

  const handleComplete = useCallback(async () => {
    if (!currentWorkspace) {
      setError('No workspace selected');
      return;
    }

    const request: CreateProjectRequest = {
      name: name.trim(),
      workspace_id: currentWorkspace.id,
      description: description.trim() || undefined,
      status,
      source_id: sourceId || undefined,
    };

    const errors = getErrors(CreateProjectRequestSchema, request);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setLoading(true);
    setError(null);

    try {
      const project = await createProject(request);
      onCreated(project);
      handleClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create project');
    } finally {
      setLoading(false);
    }
  }, [currentWorkspace, name, description, status, sourceId, createProject, onCreated]);

  const handleClose = useCallback(() => {
    setCurrentStep(0);
    setName('');
    setDescription('');
    setStatus('active');
    setSourceId('');
    setError(null);
    setFieldErrors({});
    onClose();
  }, [onClose]);

  const activeSources = useMemo(() => sources.filter((s) => s.is_active), [sources]);

  const renderStepContent = () => {
    switch (currentStep) {
      case 0:
        return (
          <div className="wizard-step-content">
            <p className="wizard-step-intro">
              Give your project a name and optionally add a description to help identify it.
            </p>
            <div className="form-group">
              <label htmlFor="project-name">Project Name</label>
              <input
                type="text"
                id="project-name"
                value={name}
                onChange={(e) => {
                  setName(e.target.value);
                  if (fieldErrors.name) {
                    setFieldErrors((prev) => {
                      const { name: _name, ...next } = prev;
                      return next;
                    });
                  }
                }}
                placeholder="My Awesome Project"
                className={fieldErrors.name ? 'input-error' : ''}
              />
              {fieldErrors.name && <span className="field-error">{fieldErrors.name}</span>}
            </div>
            <div className="form-group">
              <label htmlFor="project-description">
                Description
                <span className="label-optional">optional</span>
              </label>
              <textarea
                id="project-description"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Brief description of the project..."
                rows={3}
              />
            </div>
          </div>
        );

      case 1:
        return (
          <div className="wizard-step-content">
            <p className="wizard-step-intro">
              Link a data source to this project. Sources provide code repositories, calendars, or
              other data for your project.
            </p>
            {sourcesLoading ? (
              <div className="wizard-loading">Loading sources...</div>
            ) : activeSources.length === 0 ? (
              <div className="wizard-empty-state">
                <p>No sources available.</p>
                <p className="wizard-empty-hint">
                  You can add sources from the Sources page and link them later.
                </p>
              </div>
            ) : (
              <div className="source-selection-grid">
                <button
                  type="button"
                  className={`source-selection-option ${!sourceId ? 'selected' : ''}`}
                  onClick={() => setSourceId('')}
                >
                  <div className="source-selection-icon none">
                    <svg
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      width="24"
                      height="24"
                    >
                      <circle cx="12" cy="12" r="10" />
                      <path d="M8 12h8" />
                    </svg>
                  </div>
                  <div className="source-selection-info">
                    <span className="source-selection-name">No Source</span>
                    <span className="source-selection-desc">Create project without a source</span>
                  </div>
                </button>
                {activeSources.map((source) => (
                  <button
                    key={source.id}
                    type="button"
                    className={`source-selection-option ${sourceId === source.id ? 'selected' : ''}`}
                    onClick={() => setSourceId(source.id)}
                  >
                    <div className={`source-selection-icon ${source.source_type}`}>
                      <span className="source-type-initial">
                        {source.source_type.charAt(0).toUpperCase()}
                      </span>
                    </div>
                    <div className="source-selection-info">
                      <span className="source-selection-name">{source.name}</span>
                      <span className="source-selection-desc">{source.source_type}</span>
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>
        );

      case 2:
        return (
          <div className="wizard-step-content">
            <p className="wizard-step-intro">
              Set the initial status for your project. You can change this at any time.
            </p>
            <div className="status-selection-grid">
              {(Object.keys(statusLabels) as ProjectStatus[]).map((statusOption) => (
                <button
                  key={statusOption}
                  type="button"
                  className={`status-selection-option ${status === statusOption ? 'selected' : ''}`}
                  onClick={() => setStatus(statusOption)}
                >
                  <div className={`status-indicator status-${statusOption}`} />
                  <div className="status-selection-info">
                    <span className="status-selection-name">{statusLabels[statusOption]}</span>
                    <span className="status-selection-desc">
                      {statusDescriptions[statusOption]}
                    </span>
                  </div>
                </button>
              ))}
            </div>
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
      title="New Project"
      subtitle="Create a new project to organize your work"
      steps={WIZARD_STEPS}
      currentStep={currentStep}
      onStepChange={handleStepChange}
      onComplete={handleComplete}
      onCancel={handleClose}
      completeLabel={loading ? 'Creating...' : 'Create Project'}
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

export default CreateProjectWizard;
