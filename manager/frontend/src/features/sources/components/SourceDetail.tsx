import { Button } from '@zone/ui';
import { useCallback, useMemo, useState } from 'react';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { getErrors } from '../../../validation';
import { formStateFromConfig, GITHUB_SOURCE_TYPE, getSourceById, getSourceLabel } from '../config';
import { type SourceVerifyResponse, updateSourceRequestSchema } from '../schemas';
import type { Source, UpdateSourceRequest } from '../types';
import { isAppBacked } from '../utils';
import { FormFieldRenderer, FormFieldsRenderer } from './FormFields';
import { GitHubSourceConnection } from './GitHubSourceConnection';

interface SourceDetailProps {
  source: Source;
  onClose: () => void;
  onSaved: () => Promise<void> | void;
  updateSource: (request: UpdateSourceRequest) => Promise<Source>;
  verifySource: () => Promise<SourceVerifyResponse>;
  readOnly: boolean;
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
  readOnly,
}: SourceDetailProps) {
  const definition = getSourceById(source.source_type);
  const { currentOrganization } = useWorkspace();
  const organizationId = currentOrganization?.id ?? null;
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
    setFormState((previous) => ({ ...previous, [id]: value }));
  }, []);

  const handleSave = async () => {
    if (!dirty) return;

    const request: UpdateSourceRequest = {};
    if (name !== source.name) request.name = name;
    if (description !== descriptionBaseline) request.description = description;
    if (isActive !== source.is_active) request.is_active = isActive;
    if (dirtyConfig && definition) request.config = definition.buildConfig(formState);
    if (credentials !== '') {
      request.credentials = credentials;
      const instance = definition?.getInstance?.(baseline);
      if (instance !== undefined) request.instance = instance;
    }

    const errors = getErrors(updateSourceRequestSchema(source.source_type), request);
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
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Failed to update source');
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
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Failed to verify source');
    } finally {
      setVerifying(false);
    }
  };

  return (
    <section className="source-details">
      <header className="source-details-header">
        <h3>{source.name}</h3>
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
          <input
            type="text"
            id="source-type"
            value={getSourceLabel(source.source_type)}
            readOnly
            disabled
          />
        </div>
        <div className="form-group">
          <label htmlFor="source-url">URL</label>
          <input
            type="text"
            id="source-url"
            className="input-mono"
            value={source.url}
            readOnly
            disabled
          />
        </div>
        {source.source_type === GITHUB_SOURCE_TYPE && (
          <GitHubSourceConnection
            source={source}
            organizationId={organizationId}
            updateSource={updateSource}
            onSaved={onSaved}
            onError={setError}
            readOnly={readOnly}
          />
        )}
        <div className="form-group">
          <label htmlFor="source-name">Name</label>
          <input
            type="text"
            id="source-name"
            value={name}
            disabled={readOnly}
            onChange={(event) => setName(event.target.value)}
          />
        </div>
        <div className="form-group">
          <label htmlFor="source-description">
            Description
            <span className="label-optional">optional</span>
          </label>
          <textarea
            id="source-description"
            value={description}
            disabled={readOnly}
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
          disabled={readOnly}
        />
        {definition && definition.formFields.length > 0 && (
          <FormFieldsRenderer
            fields={definition.formFields}
            state={formState}
            onChange={handleFieldChange}
            disabled={readOnly}
          />
        )}
        {definition?.credentialField && !isAppBacked(source) && !readOnly && (
          <FormFieldRenderer
            field={{ ...definition.credentialField, required: false }}
            value={credentials}
            onChange={(_, value) => setCredentials(value as string)}
          />
        )}
      </div>

      {!readOnly && (
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
      )}
    </section>
  );
}
