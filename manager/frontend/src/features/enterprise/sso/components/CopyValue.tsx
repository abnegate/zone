import { Button } from '@zone/ui';
import { useEffect, useState } from 'react';

const COPIED_FOR_MS = 2000;

export const COPY_FAILED =
  'Could not copy to the clipboard. Select the address and copy it instead.';

interface CopyValueProps {
  value: string;
  name: string;
  label?: string;
}

export function CopyValue({ value, name, label }: CopyValueProps) {
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
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setError(null);
    } catch {
      setError(COPY_FAILED);
    }
  };

  return (
    <div className="sso-copy">
      {label && <span className="sso-copy-label">{label}</span>}
      <div className="sso-copy-row">
        <code className="sso-copy-value">{value}</code>
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={copied ? `Copied ${name}` : `Copy ${name}`}
          onClick={() => void copy()}
        >
          {copied ? 'Copied' : 'Copy'}
        </Button>
      </div>
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
