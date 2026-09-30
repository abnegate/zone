import { Button } from '@zone/ui';
import { type FormEvent, useState } from 'react';
import { WebhookSecretSchema } from '../schemas';
import type { SyncConfig, SyncProvider } from '../types';
import { formatDate } from '../utils/formatters';
import { SyncSecretCallout } from './SyncSecretCallout';

const SETUP_HINTS: Record<SyncProvider, string> = {
  github:
    'Add a webhook to the repository with the Payload URL above, content type application/json, this secret, and events: Issues.',
  linear:
    'Create an Issues webhook in Linear with the URL above and paste its signing secret below.',
};

interface SyncConfigCardProps {
  config: SyncConfig;
  secret: string | null;
  onDismissSecret: () => void;
  onSetSecret: (secret?: string) => Promise<boolean>;
  onRemove: () => void;
}

export function SyncConfigCard({
  config,
  secret,
  onDismissSecret,
  onSetSecret,
  onRemove,
}: SyncConfigCardProps) {
  const [draft, setDraft] = useState('');
  const [draftError, setDraftError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  const issuesOwnSecret = config.provider === 'linear';
  const missing = config.webhook_secret_configured === false;
  const draftId = `sync-secret-${config.id}`;

  const change = async (next?: string) => {
    setSaving(true);
    setSaved(false);
    const succeeded = await onSetSecret(next);
    setSaving(false);
    return succeeded;
  };

  const save = async (event: FormEvent) => {
    event.preventDefault();
    const result = WebhookSecretSchema.safeParse(draft);
    if (!result.success) {
      setDraftError(result.error.issues[0]?.message ?? 'Enter the signing secret');
      return;
    }
    setDraftError(null);
    if (await change(result.data)) {
      setDraft('');
      setSaved(true);
    }
  };

  return (
    <div className="sync-config-item" data-testid={`sync-config-${config.id}`}>
      <div className="sync-config-info">
        <span className={`sync-provider-badge ${config.provider}`}>{config.provider}</span>
        <span className="sync-direction">{config.direction}</span>
        {config.external_repo_url && (
          <a
            href={config.external_repo_url}
            target="_blank"
            rel="noopener noreferrer"
            className="sync-external-link"
            onClick={(event) => event.stopPropagation()}
          >
            {config.external_repo_url}
          </a>
        )}
        {config.external_project_id && (
          <span className="sync-external-link">{config.external_project_id}</span>
        )}
        <div className="sync-config-actions">
          {!issuesOwnSecret && !missing && (
            <Button
              variant="ghost"
              size="sm"
              disabled={saving}
              loading={saving}
              onClick={() => change()}
            >
              Rotate secret
            </Button>
          )}
          <Button variant="ghost" size="sm" className="sync-config-remove" onClick={onRemove}>
            Remove
          </Button>
        </div>
      </div>
      <div className="sync-config-state">
        <span className={`sync-status ${config.last_synced_at ? 'synced' : ''}`}>
          {config.last_synced_at
            ? `Synced ${formatDate(config.last_synced_at)}`
            : 'Configured, not yet synced'}
        </span>
        {config.webhook_path && (
          <code className="sync-webhook" title="Register this webhook URL with the provider">
            {`${window.location.origin}${config.webhook_path}`}
          </code>
        )}
      </div>
      {missing && (
        <div className="sync-secret-missing" role="alert">
          <span>No webhook secret: deliveries are refused until one is set</span>
          {!issuesOwnSecret && (
            <Button
              variant="secondary"
              size="sm"
              disabled={saving}
              loading={saving}
              onClick={() => change()}
            >
              Generate secret
            </Button>
          )}
        </div>
      )}
      {secret && (
        <SyncSecretCallout
          key={secret}
          secret={secret}
          hint={SETUP_HINTS[config.provider]}
          onDismiss={onDismissSecret}
        />
      )}
      {issuesOwnSecret && (
        <form className="sync-secret-form" onSubmit={save} noValidate>
          <label htmlFor={draftId}>Set signing secret</label>
          <p className="sync-secret-hint">{SETUP_HINTS.linear}</p>
          <div className="sync-secret-row">
            <input
              id={draftId}
              type="password"
              autoComplete="off"
              value={draft}
              onChange={(event) => {
                setDraft(event.target.value);
                setDraftError(null);
              }}
              placeholder="Linear signing secret"
              className={draftError ? 'input-error' : ''}
              aria-invalid={draftError !== null}
            />
            <Button type="submit" size="sm" disabled={saving || !draft.trim()} loading={saving}>
              Save
            </Button>
          </div>
          {draftError && <span className="field-error">{draftError}</span>}
          {saved && (
            <span className="sync-secret-saved" role="status">
              Signing secret saved
            </span>
          )}
        </form>
      )}
    </div>
  );
}
