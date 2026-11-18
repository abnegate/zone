import { Button } from '@zone/ui';
import { useEffect, useState } from 'react';

const COPIED_FOR_MS = 2000;

interface RedirectUriProps {
  uri: string;
}

export function RedirectUri({ uri }: RedirectUriProps) {
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
      await navigator.clipboard.writeText(uri);
      setCopied(true);
      setError(null);
    } catch {
      setError('Could not copy to the clipboard. Select the address and copy it instead.');
    }
  };

  return (
    <div className="settings-card sso-redirect">
      <h3 className="sso-card-title">Redirect URI</h3>
      <div className="sso-redirect-row">
        <code className="sso-redirect-uri">{uri}</code>
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={copied ? 'Copied redirect URI' : 'Copy redirect URI'}
          onClick={() => void copy()}
        >
          {copied ? 'Copied' : 'Copy'}
        </Button>
      </div>
      <p className="sso-hint">
        Add this as the sign-in redirect URI when you register Zone with your identity provider.
      </p>
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
