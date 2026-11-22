import { Button } from '@zone/ui';
import { useEffect, useId, useState } from 'react';
import { LicenseStatus } from '../schemas';
import type { LicenseStatus as LicenseStatusValue } from '../types';

const COPIED_FOR_MS = 2000;

export const LICENSE_ID_HINT = 'Quote this ID when you contact Zone support.';

export const LICENSE_ID_LAPSED_HINT =
  'This license has lapsed, so this server gets Community support on GitHub.';

export const LICENSE_ID_COPIED = 'Copied the license ID to the clipboard.';

export const LICENSE_ID_COPY_FAILED =
  'Could not copy to the clipboard. Select the license ID and copy it instead.';

interface LicenseIdProps {
  id: string;
  status: LicenseStatusValue;
}

export function LicenseId({ id, status }: LicenseIdProps) {
  const hintId = useId();
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!copied) {
      return;
    }
    const timer = window.setTimeout(() => setCopied(false), COPIED_FOR_MS);
    return () => window.clearTimeout(timer);
  }, [copied]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(id);
      setCopied(true);
      setError(null);
    } catch {
      setCopied(false);
      setError(LICENSE_ID_COPY_FAILED);
    }
  };

  const hint = status === LicenseStatus.Lapsed ? LICENSE_ID_LAPSED_HINT : LICENSE_ID_HINT;

  return (
    <div className="license-detail license-id">
      <dt>License ID</dt>
      <dd>
        <div className="license-id-row">
          <code className="license-id-value">{id}</code>
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={copied ? 'Copied license ID' : 'Copy license ID'}
            aria-describedby={hintId}
            onClick={() => void copy()}
          >
            {copied ? 'Copied' : 'Copy'}
          </Button>
        </div>
        <p id={hintId} className="license-id-hint">
          {hint}
        </p>
        {error && <p className="field-error">{error}</p>}
        <span className="sr-only license-id-announcement" aria-live="polite" aria-atomic="true">
          {copied ? LICENSE_ID_COPIED : (error ?? '')}
        </span>
      </dd>
    </div>
  );
}
