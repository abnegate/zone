import { useCallback, useEffect, useState } from 'react';
import { client } from '../api/client';
import { Button } from '../components';
import {
  type FormField,
  type FormRow,
  getSourceBadgeColor,
  getSourceById,
  getSourceLabel,
  initializeFormState,
  sourceRegistry,
} from '../sources';
import type { CreateSourceRequest, Source, SourceType } from '../types';
import { CreateSourceRequestSchema, getErrors } from '../validation';
import './SourcesPage.css';

function SourceTypeBadge({ type }: { type: SourceType }) {
  return (
    <span className={`source-type-badge ${getSourceBadgeColor(type)}`}>{getSourceLabel(type)}</span>
  );
}

function SourceStatusBadge({ source }: { source: Source }) {
  if (!source.is_active) {
    return <span className="source-status-badge badge-gray">Inactive</span>;
  }
  if (source.last_error) {
    return <span className="source-status-badge badge-red">Error</span>;
  }
  if (source.last_verified_at) {
    return <span className="source-status-badge badge-green">Verified</span>;
  }
  return <span className="source-status-badge badge-yellow">Unverified</span>;
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
      {fields.map((item, index) => {
        if ('fields' in item) {
          // It's a FormRow with multiple fields
          return (
            <div key={index} className="form-row">
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
        // It's a single FormField
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

interface CreateSourceModalProps {
  onClose: () => void;
  onCreated: (source: Source) => void;
}

function CreateSourceModal({ onClose, onCreated }: CreateSourceModalProps) {
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

  // Reset form state when source type changes
  const handleSourceTypeChange = (newType: SourceType) => {
    setSourceType(newType);
    setFormState(initializeFormState(newType));
    setCredentials('');
    setFieldErrors({});
  };

  const handleFieldChange = (id: string, value: unknown) => {
    setFormState((prev) => ({ ...prev, [id]: value }));
    // Clear field error when user types
    if (fieldErrors[id]) {
      setFieldErrors((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!currentSource) return;

    const config = currentSource.buildConfig(formState);
    const defaultName = currentSource.getDefaultName(formState);

    const request: CreateSourceRequest = {
      name: name || defaultName,
      source_type: sourceType,
      config,
      description: description || undefined,
      credentials: credentials || undefined,
    };

    // Validate the request
    const errors = getErrors(CreateSourceRequestSchema, request);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setLoading(true);
    setError(null);

    try {
      const source = await client.createSource(request);
      onCreated(source);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create source');
    } finally {
      setLoading(false);
    }
  };

  const enabledSources = sourceRegistry.filter((s) => s.enabled);

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-content source-modal" onClick={(e) => e.stopPropagation()}>
        <header className="modal-header">
          <div>
            <h2>Add Source</h2>
            <p className="modal-subtitle">
              Connect a repository, calendar, email, or other data source
            </p>
          </div>
          <button type="button" onClick={onClose} className="close-btn" aria-label="Close">
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              width="20"
              height="20"
            >
              <path d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </header>

        <form onSubmit={handleSubmit}>
          {/* Source Type Selector */}
          <div className="source-type-selector">
            {enabledSources.map((source) => (
              <button
                key={source.id}
                type="button"
                className={`source-type-card ${sourceType === source.id ? 'selected' : ''}`}
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

          {/* Config Section */}
          <div className="form-section">
            <div className="form-section-header">
              <span className="form-section-title">Configuration</span>
            </div>

            {currentSource && currentSource.formFields.length > 0 && (
              <div className="form-section-content">
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
              </div>
            )}

            {currentSource && currentSource.formFields.length === 0 && (
              <div className="form-section-content">
                <p className="form-hint" style={{ margin: 0 }}>
                  {currentSource.name} integration is coming soon.
                </p>
              </div>
            )}
          </div>

          {/* Optional Details */}
          <div className="form-section form-section-collapsed">
            <details>
              <summary className="form-section-header clickable">
                <span className="form-section-title">Additional Options</span>
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  className="chevron-icon"
                >
                  <path d="M6 9l6 6 6-6" />
                </svg>
              </summary>
              <div className="form-section-content">
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
                    placeholder="Auto-generated if empty"
                  />
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
                    rows={2}
                  />
                </div>
              </div>
            </details>
          </div>

          {Object.keys(fieldErrors).length > 0 && (
            <div className="form-error">
              {Object.entries(fieldErrors).map(([field, message]) => (
                <div key={field}>{message}</div>
              ))}
            </div>
          )}
          {error && <div className="form-error">{error}</div>}

          <div className="form-actions">
            <Button type="button" onClick={onClose} variant="secondary">
              Cancel
            </Button>
            <Button type="submit" variant="primary" loading={loading}>
              {loading ? 'Adding...' : 'Add Source'}
            </Button>
          </div>
        </form>
      </div>
    </div>
  );
}

export default function SourcesPage() {
  const [sources, setSources] = useState<Source[]>([]);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [verifying, setVerifying] = useState<string | null>(null);

  const loadData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const sourcesData = await client.getSources();
      setSources(sourcesData);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load data');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const handleSourceCreated = (source: Source) => {
    setSources((prev) => [source, ...prev]);
  };

  const handleVerify = async (sourceId: string) => {
    setVerifying(sourceId);
    try {
      const result = await client.verifySource(sourceId);
      const updated = await client.getSource(sourceId);
      setSources((prev) => prev.map((s) => (s.id === sourceId ? updated : s)));
      if (!result.success) {
        setError(`Verification failed: ${result.message}`);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to verify source');
    } finally {
      setVerifying(null);
    }
  };

  const handleDelete = async (sourceId: string) => {
    if (!window.confirm('Are you sure you want to delete this source?')) return;

    try {
      await client.deleteSource(sourceId);
      setSources((prev) => prev.filter((s) => s.id !== sourceId));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete source');
    }
  };

  const handleToggleActive = async (source: Source) => {
    try {
      const updated = await client.updateSource(source.id, { is_active: !source.is_active });
      setSources((prev) => prev.map((s) => (s.id === source.id ? updated : s)));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to update source');
    }
  };

  return (
    <div className="page sources-page">
      <header className="page-header">
        <div className="header-content">
          <h1>Sources</h1>
          <p className="subtitle">Connect repositories, calendars, email, and other data sources</p>
        </div>
        <Button variant="primary" onClick={() => setShowCreateModal(true)}>
          + Add Source
        </Button>
      </header>

      {error && <div className="error-banner">{error}</div>}

      {loading ? (
        <div className="sources-list">
          {[1, 2, 3].map((i) => (
            <div key={i} className="source-card skeleton-card">
              <div className="skeleton-header">
                <div className="skeleton skeleton-title" />
                <div className="skeleton skeleton-badge" />
              </div>
              <div className="skeleton skeleton-text" />
              <div className="skeleton skeleton-text short" />
              <div className="skeleton-actions">
                <div className="skeleton skeleton-btn" />
                <div className="skeleton skeleton-btn" />
              </div>
            </div>
          ))}
        </div>
      ) : sources.length === 0 ? (
        <div className="empty-state">
          <p>No sources configured. Add a source to get started!</p>
          <p className="empty-hint">
            Sources can be code repositories, calendars, email inboxes, web URLs, or text content.
          </p>
        </div>
      ) : (
        <div className="sources-list">
          {sources.map((source) => (
            <div
              key={source.id}
              className={`source-card ${!source.is_active ? 'source-inactive' : ''}`}
            >
              <div className="source-card-header">
                <h3>{source.name}</h3>
                <div className="source-badges">
                  <SourceTypeBadge type={source.source_type} />
                  <SourceStatusBadge source={source} />
                </div>
              </div>

              {source.description && <p className="source-description">{source.description}</p>}

              <div className="source-url">
                <a href={source.url} target="_blank" rel="noopener noreferrer">
                  {source.url}
                </a>
              </div>

              {source.last_error && <div className="source-error">{source.last_error}</div>}

              <div className="source-meta">
                {source.last_verified_at && (
                  <span>Verified: {new Date(source.last_verified_at).toLocaleDateString()}</span>
                )}
              </div>

              <div className="source-actions">
                <Button
                  variant="secondary"
                  onClick={() => handleVerify(source.id)}
                  loading={verifying === source.id}
                >
                  {verifying === source.id ? 'Verifying...' : 'Verify'}
                </Button>
                <Button
                  variant={source.is_active ? 'secondary' : 'primary'}
                  onClick={() => handleToggleActive(source)}
                >
                  {source.is_active ? 'Disable' : 'Enable'}
                </Button>
                <Button variant="danger" onClick={() => handleDelete(source.id)}>
                  Delete
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      {showCreateModal && (
        <CreateSourceModal
          onClose={() => setShowCreateModal(false)}
          onCreated={handleSourceCreated}
        />
      )}
    </div>
  );
}
