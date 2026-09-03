import type { WizardStep } from '@zone/ui';
import { useCallback, useMemo, useState } from 'react';
import { Wizard } from '../../../components';
import { getErrors } from '../../../validation';
import {
  type FormField,
  type FormRow,
  getSourceById,
  initializeFormState,
  sourceRegistry,
} from '../config';
import { CreateSourceRequestSchema } from '../schemas';
import type { CreateSourceRequest, Source, SourceType } from '../types';

interface CreateSourceWizardProps {
  isOpen: boolean;
  onClose: () => void;
  onCreated: () => void;
  createSource: (request: CreateSourceRequest) => Promise<Source>;
}

// Dynamic form field renderer
function FormFieldRenderer({
  field,
  value,
  onChange,
}: {
  field: FormField;
  value: unknown;
  onChange: (id: string, value: unknown) => void;
}) {
  if (field.type === 'toggle') {
    return (
      <div className="form-group">
        <label className="toggle-label">
          <span className="toggle-wrapper">
            <input
              type="checkbox"
              checked={value as boolean}
              onChange={(e) => onChange(field.id, e.target.checked)}
            />
            <span className="toggle-slider" />
          </span>
          <span className="toggle-text">
            <span className="toggle-title">{field.toggleTitle || field.label}</span>
            {field.toggleDescription && (
              <span className="toggle-desc">{field.toggleDescription}</span>
            )}
          </span>
        </label>
      </div>
    );
  }

  if (field.type === 'textarea') {
    return (
      <div className="form-group">
        <label htmlFor={field.id}>
          {field.label}
          {!field.required && <span className="label-optional">optional</span>}
        </label>
        <textarea
          id={field.id}
          value={value as string}
          onChange={(e) => onChange(field.id, e.target.value)}
          placeholder={field.placeholder}
          required={field.required}
          rows={6}
        />
        {field.hint && <span className="form-hint">{field.hint}</span>}
      </div>
    );
  }

  return (
    <div className="form-group">
      <label htmlFor={field.id}>
        {field.label}
        {!field.required && <span className="label-optional">optional</span>}
      </label>
      <input
        type={field.type}
        id={field.id}
        value={value as string | number}
        onChange={(e) =>
          onChange(
            field.id,
            field.type === 'number' ? Number.parseInt(e.target.value, 10) || 0 : e.target.value
          )
        }
        placeholder={field.placeholder}
        required={field.required}
        className={field.monospace ? 'input-mono' : undefined}
      />
      {field.hint && <span className="form-hint">{field.hint}</span>}
    </div>
  );
}

// Render form fields (handles both single fields and rows)
function FormFieldsRenderer({
  fields,
  state,
  onChange,
}: {
  fields: (FormField | FormRow)[];
  state: Record<string, unknown>;
  onChange: (id: string, value: unknown) => void;
}) {
  return (
    <>
      {fields.map((item) => {
        if ('fields' in item) {
          const rowKey = item.fields.map((f) => f.id).join('-');
          return (
            <div key={rowKey} className="form-row">
              {item.fields.map((field) => (
                <FormFieldRenderer
                  key={field.id}
                  field={field}
                  value={state[field.id]}
                  onChange={onChange}
                />
              ))}
            </div>
          );
        }
        return (
          <FormFieldRenderer
            key={item.id}
            field={item}
            value={state[item.id]}
            onChange={onChange}
          />
        );
      })}
    </>
  );
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
  const [sourceType, setSourceType] = useState<SourceType>('github');
  const [formState, setFormState] = useState<Record<string, unknown>>(() =>
    initializeFormState('github')
  );
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [credentials, setCredentials] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  const currentSource = getSourceById(sourceType);
  const enabledSources = useMemo(() => sourceRegistry.filter((s) => s.enabled), []);

  const handleSourceTypeChange = useCallback((newType: SourceType) => {
    setSourceType(newType);
    setFormState(initializeFormState(newType));
    setCredentials('');
    setFieldErrors({});
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
      // Check required fields
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
  }, [currentStep, sourceType, currentSource, formState]);

  const handleComplete = useCallback(async () => {
    if (!currentSource) return;

    const config = currentSource.buildConfig(formState);
    const defaultName = currentSource.getDefaultName(formState);

    const request: CreateSourceRequest = {
      name: name || defaultName,
      source_type: sourceType,
      config,
      description: description || undefined,
      credentials: credentials || undefined,
      url: currentSource.getUrl?.(formState) || undefined,
    };

    const errors = getErrors(CreateSourceRequestSchema, request);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setLoading(true);
    setError(null);

    try {
      await createSource(request);
      onCreated();
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
    createSource,
    onCreated,
  ]);

  const handleClose = useCallback(() => {
    setCurrentStep(0);
    setSourceType('github');
    setFormState(initializeFormState('github'));
    setName('');
    setDescription('');
    setCredentials('');
    setError(null);
    setFieldErrors({});
    onClose();
  }, [onClose]);

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
                <FormFieldsRenderer
                  fields={currentSource.formFields}
                  state={formState}
                  onChange={handleFieldChange}
                />
                {currentSource.credentialField && (
                  <FormFieldRenderer
                    field={currentSource.credentialField}
                    value={credentials}
                    onChange={(_, value) => setCredentials(value as string)}
                  />
                )}
                {currentSource.formHint && (
                  <p className="form-section-hint">{currentSource.formHint}</p>
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
                placeholder={currentSource?.getDefaultName(formState) || 'Auto-generated if empty'}
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
      isOpen={isOpen}
      onClose={handleClose}
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
