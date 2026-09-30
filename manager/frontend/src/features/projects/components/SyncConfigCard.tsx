import { Button, Modal } from '@zone/ui';
import { type FormEvent, useState } from 'react';
import { WebhookSecretSchema } from '../schemas';
import type { RevealedSecret, SyncConfig, SyncProvider } from '../types';
import { formatDate } from '../utils/formatters';
import { SyncSecretCallout } from './SyncSecretCallout';

const SETUP_HINTS: Record<SyncProvider, string> = {
  github:
    'Add a webhook to the repository with the Payload URL above, content type application/json, this secret, and events: Issues.',
  linear:
    'Create an Issues webhook in Linear with the URL above and paste its signing secret below.',
};

const PROVIDER_NAMES: Record<SyncProvider, string> = {
  github: 'GitHub',
  linear: 'Linear',
};

const PASSWORD_MANAGER_OPT_OUT = {
  'data-1p-ignore': true,
  'data-lpignore': 'true',
  'data-form-type': 'other',
} as const;

interface SyncConfigCardProps {
  config: SyncConfig;
  revealed: RevealedSecret | null;
  onDismissSecret: () => void;
  onSetSecret: (secret?: string) => Promise<boolean>;
  onRemove: () => void;
}

export function SyncConfigCard({
  config,
  revealed,
  onDismissSecret,
  onSetSecret,
  onRemove,
}: SyncConfigCardProps) {
  const [draft, setDraft] = useState('');
  const [draftError, setDraftError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [confirmingRotation, setConfirmingRotation] = useState(false);

  const zoneIssuesSecret = config.webhook_secret_issued_by_zone;
  const missing = config.webhook_secret_configured === false;
  const draftId = `sync-secret-${config.id}`;

  const change = async (next?: string) => {
    setSaving(true);
    setSaved(false);
    const succeeded = await onSetSecret(next);
    setSaving(false);
    return succeeded;
  };

  const rotate = async () => {
    setConfirmingRotation(false);
    await change();
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
          {zoneIssuesSecret && !missing && (
            <Button
              variant="ghost"
              size="sm"
              disabled={saving}
              loading={saving}
              onClick={() => setConfirmingRotation(true)}
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
          {zoneIssuesSecret && (
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
      {revealed && (
        <SyncSecretCallout
          key={`${config.id}-${revealed.revision}`}
          secret={revealed.secret}
          hint={SETUP_HINTS[config.provider]}
          onDismiss={onDismissSecret}
        />
      )}
      {!zoneIssuesSecret && (
        <form className="sync-secret-form" onSubmit={save} noValidate>
          <label htmlFor={draftId}>Set signing secret</label>
          <p className="sync-secret-hint">{SETUP_HINTS.linear}</p>
          <div className="sync-secret-row">
            <input
              id={draftId}
              type="text"
              autoComplete="off"
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
              {...PASSWORD_MANAGER_OPT_OUT}
              value={draft}
              onChange={(event) => {
                setDraft(event.target.value);
                setDraftError(null);
              }}
              placeholder="Linear signing secret"
              className={`sync-secret-input${draftError ? ' input-error' : ''}`}
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
      <Modal
        isOpen={confirmingRotation}
        onClose={() => setConfirmingRotation(false)}
        title="Rotate webhook secret?"
        size="sm"
      >
        <p className="sync-rotate-warning">
          Zone generates a new secret and stops accepting the current one straight away. The{' '}
          {PROVIDER_NAMES[config.provider]} webhook keeps signing with the old secret, so its
          deliveries are refused until you paste the new one into the webhook&apos;s settings.
        </p>
        <div className="modal-actions">
          <Button variant="secondary" type="button" onClick={() => setConfirmingRotation(false)}>
            Cancel
          </Button>
          <Button variant="destructive" type="button" onClick={rotate}>
            Generate new secret
          </Button>
        </div>
      </Modal>
    </div>
  );
}
