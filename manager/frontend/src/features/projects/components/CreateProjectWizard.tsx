import { useQueryClient } from '@tanstack/react-query';
import type { WizardStep } from '@zone/ui';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { client } from '../../../api/client';
import { Button, Wizard } from '../../../components';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { Source } from '../../../types';
import { getErrors } from '../../../validation';
import { CreateSourceWizard } from '../../sources/components/CreateSourceWizard';
import { sourcesQueryKey } from '../../sources/hooks/sourcesQueryKey';
import type { CreateSourceRequest } from '../../sources/types';
import { useCanCreateSources } from '../hooks/useCanCreateSources';
import { CreateProjectRequestSchema } from '../schemas';
import type { CreateProjectRequest, Project, ProjectStatus } from '../types';
import { NEW_SOURCE_OPTION, SourceStep } from './SourceStep';

interface CreateProjectWizardProps {
  isOpen: boolean;
  onClose: () => void;
  onCreated: (project: Project) => void;
  createProject: (request: CreateProjectRequest) => Promise<Project>;
  onAuto?: () => void;
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

function keepOpen(): void {}

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
  onAuto,
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
  const [creatingSource, setCreatingSource] = useState(false);
  const [focusedOption, setFocusedOption] = useState<string | null>(null);
  const canCreateSources = useCanCreateSources(workspaceId);
  const queryClient = useQueryClient();

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
    setFocusedOption(null);
    setError(null);
  }, []);

  const createSource = useCallback(
    (request: CreateSourceRequest): Promise<Source> => {
      if (!workspaceId) return Promise.reject(new Error('No workspace selected'));
      return client.createSource(workspaceId, request);
    },
    [workspaceId]
  );

  const handleNewSource = useCallback(() => {
    setFocusedOption(null);
    setCreatingSource(true);
  }, []);

  const handleNewSourceClose = useCallback(() => {
    setCreatingSource(false);
    setFocusedOption((focused) => focused ?? NEW_SOURCE_OPTION);
  }, []);

  const handleSourceCreated = useCallback(
    (source: Source) => {
      setSources((previous) =>
        previous.some((existing) => existing.id === source.id) ? previous : [...previous, source]
      );
      setSourceId(source.id);
      setFocusedOption(source.id);
      queryClient.invalidateQueries({ queryKey: sourcesQueryKey(workspaceId) });
    },
    [queryClient, workspaceId]
  );

  const canProceed = useMemo(() => {
    if (currentStep === 0) {
      return name.trim().length > 0;
    }
    return true;
  }, [currentStep, name]);

  const handleClose = useCallback(() => {
    setCurrentStep(0);
    setName('');
    setDescription('');
    setStatus('active');
    setSourceId('');
    setCreatingSource(false);
    setFocusedOption(null);
    setError(null);
    setFieldErrors({});
    onClose();
  }, [onClose]);

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
  }, [
    currentWorkspace,
    name,
    description,
    status,
    sourceId,
    createProject,
    onCreated,
    handleClose,
  ]);

  const visibleSources = useMemo(
    () => sources.filter((source) => source.is_active || source.id === sourceId),
    [sources, sourceId]
  );

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
            {onAuto && (
              <div className="project-wizard-auto">
                <p>Or describe what you want and let Zone plan the project and its tasks.</p>
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => {
                    handleClose();
                    onAuto();
                  }}
                  data-testid="auto-project-button"
                >
                  Auto project
                </Button>
              </div>
            )}
          </div>
        );

      case 1:
        return (
          <SourceStep
            sources={visibleSources}
            loading={sourcesLoading}
            selectedId={sourceId}
            onSelect={setSourceId}
            canCreate={canCreateSources}
            onCreate={handleNewSource}
            focusId={focusedOption}
          />
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
    <>
      <Wizard
        isOpen={isOpen}
        onClose={creatingSource ? keepOpen : handleClose}
        inert={creatingSource}
        title="New Project"
        subtitle="Create a new project to organize your work"
        className="project-wizard"
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
      <CreateSourceWizard
        isOpen={isOpen && creatingSource}
        onClose={handleNewSourceClose}
        onCreated={handleSourceCreated}
        createSource={createSource}
      />
    </>
  );
}

export default CreateProjectWizard;
