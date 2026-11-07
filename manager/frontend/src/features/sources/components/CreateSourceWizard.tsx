import type { WizardStep } from '@zone/ui';
import { useCallback, useMemo, useRef, useState } from 'react';
import { Wizard } from '../../../components';
import { getErrors } from '../../../validation';
import type { GitHubRepository } from '../../github';
import { GITHUB_SOURCE_TYPE, getSourceById, initializeFormState, sourceRegistry } from '../config';
import { useFocusTrap } from '../hooks/useFocusTrap';
import { useSourceKinds } from '../hooks/useSourceKinds';
import { CreateSourceRequestSchema } from '../schemas';
import type { CreateSourceRequest, Source, SourceType } from '../types';
import { FormFieldsRenderer, withCredentialField } from './FormFields';
import { GitHubConfiguration } from './GitHubConfiguration';
import { GitHubConnectionMode } from './GitHubConnectionMode';

interface CreateSourceWizardProps {
  isOpen: boolean;
  onClose: () => void;
  onCreated: (source: Source) => void;
  createSource: (request: CreateSourceRequest) => Promise<Source>;
}

function keepOpen(): void {}

function buildAppRequest(
  repository: GitHubRepository,
  branch: string,
  name: string,
  description: string
): CreateSourceRequest {
  return {
    name: name || repository.full_name,
    source_type: GITHUB_SOURCE_TYPE,
    config: {
      owner: repository.owner,
      repo: repository.name,
      branch: branch.trim() || repository.default_branch,
    },
    github_installation_id: repository.installation_id,
    url: repository.html_url,
    ...(description ? { description } : {}),
  };
}

const WIZARD_STEPS: WizardStep[] = [
  {
    id: 'type',
    title: 'Source Type',
    description: 'Choose your data source',
  },
  {
    id: 'config',
    title: 'Configuration',
    description: 'Set up connection details',
  },
  {
    id: 'details',
    title: 'Details',
    description: 'Name and description',
  },
];

export function CreateSourceWizard({
  isOpen,
  onClose,
  onCreated,
  createSource,
}: CreateSourceWizardProps) {
  const [currentStep, setCurrentStep] = useState(0);
  const [sourceType, setSourceType] = useState<SourceType>(GITHUB_SOURCE_TYPE);
  const [formState, setFormState] = useState<Record<string, unknown>>(() =>
    initializeFormState(GITHUB_SOURCE_TYPE)
  );
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [credentials, setCredentials] = useState('');
  const [connectionMode, setConnectionMode] = useState<GitHubConnectionMode>(
    GitHubConnectionMode.App
  );
  const [repository, setRepository] = useState<GitHubRepository | null>(null);
  const [branch, setBranch] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const dialog = useRef<HTMLDivElement>(null);
  useFocusTrap(dialog, isOpen);

  const currentSource = getSourceById(sourceType);
  const appMode = sourceType === GITHUB_SOURCE_TYPE && connectionMode === GitHubConnectionMode.App;
  const serverKinds = useSourceKinds(isOpen);
  const enabledSources = useMemo(
    () =>
      sourceRegistry.filter(
        (source) => source.enabled && (serverKinds === null || serverKinds.has(source.id))
      ),
    [serverKinds]
  );

  const handleSourceTypeChange = useCallback((newType: SourceType) => {
    setSourceType(newType);
    setFormState(initializeFormState(newType));
    setCredentials('');
    setConnectionMode(GitHubConnectionMode.App);
    setRepository(null);
    setBranch('');
    setFieldErrors({});
  }, []);

  const handleRepositoryChange = useCallback((selected: GitHubRepository | null) => {
    setRepository(selected);
    setBranch(selected?.default_branch ?? '');
  }, []);

  const handleFieldChange = useCallback((id: string, value: unknown) => {
    setFormState((prev) => ({ ...prev, [id]: value }));
    setFieldErrors((prev) => {
      if (prev[id]) {
        const next = { ...prev };
        delete next[id];
        return next;
      }
      return prev;
    });
  }, []);

  const handleStepChange = useCallback((step: number) => {
    setCurrentStep(step);
    setError(null);
  }, []);

  const canProceed = useMemo(() => {
    if (currentStep === 0) {
      return !!sourceType;
    }
    if (currentStep === 1) {
      if (!currentSource) return false;
      if (appMode) return repository !== null;
      for (const item of currentSource.formFields) {
        if ('fields' in item) {
          for (const field of item.fields) {
            if (field.required && !formState[field.id]) return false;
          }
        } else if (item.required && !formState[item.id]) {
          return false;
        }
      }
      return true;
    }
    return true;
  }, [currentStep, sourceType, currentSource, formState, appMode, repository]);

  const handleClose = useCallback(() => {
    setCurrentStep(0);
    setSourceType(GITHUB_SOURCE_TYPE);
    setFormState(initializeFormState(GITHUB_SOURCE_TYPE));
    setName('');
    setDescription('');
    setCredentials('');
    setConnectionMode(GitHubConnectionMode.App);
    setRepository(null);
    setBranch('');
    setError(null);
    setFieldErrors({});
    onClose();
  }, [onClose]);

  const handleComplete = useCallback(async () => {
    if (!currentSource) return;

    const request: CreateSourceRequest | null = appMode
      ? repository && buildAppRequest(repository, branch, name, description)
      : {
          name: name || currentSource.getDefaultName(formState),
          source_type: sourceType,
          config: currentSource.buildConfig(formState),
          description: description || undefined,
          credentials: credentials || undefined,
          url: currentSource.getUrl?.(formState) || undefined,
        };
    if (!request) return;

    const errors = getErrors(CreateSourceRequestSchema, request);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setLoading(true);
    setError(null);

    try {
      const source = await createSource(request);
      onCreated(source);
      handleClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create source');
    } finally {
      setLoading(false);
    }
  }, [
    currentSource,
    formState,
    name,
    description,
    credentials,
    sourceType,
    appMode,
    repository,
    branch,
    createSource,
    onCreated,
    handleClose,
  ]);

  const credentialFields = currentSource && (
    <>
      <FormFieldsRenderer
        fields={withCredentialField(currentSource.formFields, currentSource.credentialField)}
        state={
          currentSource.credentialField
            ? { ...formState, [currentSource.credentialField.id]: credentials }
            : formState
        }
        onChange={(id, value) =>
          id === currentSource.credentialField?.id
            ? setCredentials(value as string)
            : handleFieldChange(id, value)
        }
      />
      {currentSource.formHint && <p className="form-hint">{currentSource.formHint}</p>}
    </>
  );

  const renderStepContent = () => {
    switch (currentStep) {
      case 0:
        return (
          <div className="wizard-step-content">
            <p className="wizard-step-intro">
              Select the type of data source you want to connect. Each source type has different
              configuration options.
            </p>
            <div className="source-type-grid">
              {enabledSources.map((source) => (
                <button
                  key={source.id}
                  type="button"
                  className={`source-type-option ${sourceType === source.id ? 'selected' : ''}`}
                  onClick={() => handleSourceTypeChange(source.id)}
                >
                  <div className={`source-type-icon-wrapper ${source.iconWrapperClass}`}>
                    {source.icon}
                  </div>
                  <div className="source-type-info">
                    <span className="source-type-name">{source.name}</span>
                    <span className="source-type-desc">{source.description}</span>
                  </div>
                </button>
              ))}
            </div>
          </div>
        );

      case 1:
        return (
          <div className="wizard-step-content">
            {currentSource && currentSource.formFields.length > 0 ? (
              <>
                <p className="wizard-step-intro">
                  Configure the connection settings for your {currentSource.name} source.
                </p>
                {sourceType === GITHUB_SOURCE_TYPE ? (
                  <GitHubConfiguration
                    mode={connectionMode}
                    onModeChange={setConnectionMode}
                    repository={repository}
                    onRepositoryChange={handleRepositoryChange}
                    branch={branch}
                    onBranchChange={setBranch}
                  >
                    {credentialFields}
                  </GitHubConfiguration>
                ) : (
                  credentialFields
                )}
              </>
            ) : (
              <p className="wizard-step-intro">{currentSource?.name} integration is coming soon.</p>
            )}
          </div>
        );

      case 2:
        return (
          <div className="wizard-step-content">
            <p className="wizard-step-intro">
              Give your source a name and optional description for easy identification.
            </p>
            <div className="form-group">
              <label htmlFor="name">
                Display Name
                <span className="label-optional">optional</span>
              </label>
              <input
                type="text"
                id="name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={
                  (appMode ? repository?.full_name : currentSource?.getDefaultName(formState)) ||
                  'Auto-generated if empty'
                }
              />
              <span className="form-hint">Leave empty to auto-generate based on configuration</span>
            </div>
            <div className="form-group">
              <label htmlFor="description">
                Description
                <span className="label-optional">optional</span>
              </label>
              <textarea
                id="description"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="What is this source used for?"
                rows={3}
              />
            </div>
          </div>
        );

      default:
        return null;
    }
  };

  return (
    <Wizard
      ref={dialog}
      tabIndex={-1}
      // Every Wizard titles itself with the same element id, so a wizard opened over another names itself
      aria-labelledby={undefined}
      aria-label="Add Source"
      isOpen={isOpen}
      onClose={loading ? keepOpen : handleClose}
      title="Add Source"
      subtitle="Connect a repository, calendar, email, or other data source"
      size="lg"
      steps={WIZARD_STEPS}
      currentStep={currentStep}
      onStepChange={handleStepChange}
      onComplete={handleComplete}
      onCancel={handleClose}
      completeLabel={loading ? 'Adding...' : 'Add Source'}
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

export default CreateSourceWizard;
