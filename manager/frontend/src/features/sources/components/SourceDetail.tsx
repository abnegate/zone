import { Button } from '@zone/ui';
import { useCallback, useMemo, useState } from 'react';
import { getErrors } from '../../../validation';
import { formStateFromConfig, getSourceById, getSourceLabel } from '../config';
import { type SourceVerifyResponse, UpdateSourceRequestSchema } from '../schemas';
import type { Source, UpdateSourceRequest } from '../types';
import { FormFieldRenderer, FormFieldsRenderer } from './FormFields';

interface SourceDetailProps {
  source: Source;
  onClose: () => void;
  onSaved: () => Promise<void> | void;
  updateSource: (request: UpdateSourceRequest) => Promise<Source>;
  verifySource: () => Promise<SourceVerifyResponse>;
}

function configChanged(
  current: Record<string, unknown>,
  baseline: Record<string, unknown>,
  fieldIds: string[]
): boolean {
  return fieldIds.some((id) => current[id] !== baseline[id]);
}

export function SourceDetail({
  source,
  onClose,
  onSaved,
  updateSource,
  verifySource,
}: SourceDetailProps) {
  const definition = getSourceById(source.source_type);
  const baseline = useMemo(
    () => formStateFromConfig(source.source_type, source.config),
    [source.source_type, source.config]
  );

  const [name, setName] = useState(source.name);
  const [description, setDescription] = useState(source.description ?? '');
  const [isActive, setIsActive] = useState(source.is_active);
  const [formState, setFormState] = useState<Record<string, unknown>>(baseline);
  const [credentials, setCredentials] = useState('');
  const [saving, setSaving] = useState(false);
  const [verifying, setVerifying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  const descriptionBaseline = source.description ?? '';
  const dirtyConfig = definition
    ? configChanged(formState, baseline, definition.getFieldIds())
    : false;
  const dirty =
    name !== source.name ||
    description !== descriptionBaseline ||
    isActive !== source.is_active ||
    dirtyConfig ||
    credentials !== '';

  const handleFieldChange = useCallback((id: string, value: unknown) => {
    setFormState((prev) => ({ ...prev, [id]: value }));
  }, []);

  const handleSave = async () => {
    if (!dirty) return;

    const request: UpdateSourceRequest = {};
    if (name !== source.name) request.name = name;
    if (description !== descriptionBaseline) request.description = description;
    if (isActive !== source.is_active) request.is_active = isActive;
    if (dirtyConfig && definition) {
      request.config = definition.buildConfig(formState);
      const url = definition.getUrl?.(formState);
      if (url !== undefined) request.url = url;
    }
    if (credentials !== '') request.credentials = credentials;

    const errors = getErrors(UpdateSourceRequestSchema, request);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setSaving(true);
    setError(null);
    try {
      await updateSource(request);
      setCredentials('');
      await onSaved();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to update source');
    } finally {
      setSaving(false);
    }
  };

  const handleVerify = async () => {
    setVerifying(true);
    setError(null);
    try {
      const result = await verifySource();
      if (!result.verified) {
        setError(result.message || 'Verification failed');
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to verify source');
    } finally {
      setVerifying(false);
    }
  };

  return (
    <section className="source-details">
      <header className="source-details-header">
        <h2>{source.name}</h2>
        <Button variant="ghost" size="icon" onClick={onClose} aria-label="Close">
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            width="16"
            height="16"
            aria-hidden="true"
          >
            <path d="M6 18L18 6M6 6l12 12" />
          </svg>
        </Button>
      </header>

      <div className="source-details-body">
        {error && (
          <div className="form-error" role="alert">
            {error}
          </div>
        )}
        {Object.keys(fieldErrors).length > 0 && (
          <div className="form-error">
            {Object.entries(fieldErrors).map(([field, message]) => (
              <div key={field}>{message}</div>
            ))}
          </div>
        )}

        <div className="form-group">
          <label htmlFor="source-type">Type</label>
          <input id="source-type" value={getSourceLabel(source.source_type)} readOnly disabled />
        </div>
        <div className="form-group">
          <label htmlFor="source-url">URL</label>
          <input id="source-url" className="input-mono" value={source.url} readOnly disabled />
        </div>
        <div className="form-group">
          <label htmlFor="source-name">Name</label>
          <input id="source-name" value={name} onChange={(event) => setName(event.target.value)} />
        </div>
        <div className="form-group">
          <label htmlFor="source-description">
            Description
            <span className="label-optional">optional</span>
          </label>
          <textarea
            id="source-description"
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            rows={3}
          />
        </div>
        <FormFieldRenderer
          field={{
            id: 'is_active',
            label: 'Active',
            type: 'toggle',
            toggleTitle: 'Active',
          }}
          value={isActive}
          onChange={(_, value) => setIsActive(value as boolean)}
        />
        {definition && definition.formFields.length > 0 && (
          <FormFieldsRenderer
            fields={definition.formFields}
            state={formState}
            onChange={handleFieldChange}
          />
        )}
        {definition?.credentialField && (
          <FormFieldRenderer
            field={{ ...definition.credentialField, required: false }}
            value={credentials}
            onChange={(_, value) => setCredentials(value as string)}
          />
        )}
      </div>

      <footer className="source-details-actions">
        <Button variant="ghost" size="sm" onClick={handleVerify} loading={verifying}>
          {verifying ? 'Verifying...' : 'Verify'}
        </Button>
        <Button variant="ghost" size="sm" onClick={onClose}>
          Cancel
        </Button>
        <Button onClick={handleSave} disabled={!dirty || saving} loading={saving}>
          Save
        </Button>
      </footer>
    </section>
  );
}
