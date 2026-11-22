import { Button } from '@zone/ui';
import { useEffect, useId, useState } from 'react';

const COPIED_FOR_MS = 2000;

export const LICENSE_ID_HINT = 'Quote this ID when you contact Zone support.';

export const LICENSE_ID_COPY_FAILED =
  'Could not copy to the clipboard. Select the license ID and copy it instead.';

interface LicenseIdProps {
  id: string;
}

export function LicenseId({ id }: LicenseIdProps) {
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
          {LICENSE_ID_HINT}
        </p>
        {error && (
          <p className="field-error" role="alert">
            {error}
          </p>
        )}
      </dd>
    </div>
  );
}
