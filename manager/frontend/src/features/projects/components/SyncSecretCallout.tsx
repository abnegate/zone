import { Button } from '@zone/ui';
import { useState } from 'react';

type CopyState = 'idle' | 'copied' | 'failed';

const COPY_LABELS: Record<CopyState, string> = {
  idle: 'Copy',
  copied: 'Copied',
  failed: 'Select and copy',
};

interface SyncSecretCalloutProps {
  secret: string;
  hint: string;
  onDismiss: () => void;
}

export function SyncSecretCallout({ secret, hint, onDismiss }: SyncSecretCalloutProps) {
  const [copyState, setCopyState] = useState<CopyState>('idle');

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(secret);
      setCopyState('copied');
    } catch {
      setCopyState('failed');
    }
  };

  return (
    <section className="sync-secret-callout" aria-label="Webhook secret" data-testid="sync-secret">
      <div className="sync-secret-callout-header">
        <strong>Webhook secret, shown once</strong>
        <Button variant="ghost" size="sm" onClick={onDismiss}>
          Dismiss
        </Button>
      </div>
      <div className="sync-secret-value">
        <code data-testid="sync-secret-value">{secret}</code>
        <Button variant="secondary" size="sm" onClick={copy}>
          {COPY_LABELS[copyState]}
        </Button>
      </div>
      <p className="sync-secret-hint">{hint} It will not be shown again.</p>
    </section>
  );
}
