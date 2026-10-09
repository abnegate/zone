import { Button } from '@zone/ui';
import { useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import { isPhoneReachableHost } from '../phoneOrigin';
import './ConnectDevicesSection.css';

const EMPTY_HINT =
  "Set ZONE_CONNECT_URL in .env to this computer's LAN address, for example http://192.168.0.10, or open this console at that address.";

const EMPTY_HINT_REACHABLE =
  'Add more addresses with ZONE_CONNECT_URL in .env, for example a Tailscale IP.';

export default function ConnectDevicesSection() {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [urls, setUrls] = useState<string[]>([]);
  const [copied, setCopied] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await client.getConnectUrls();
      setUrls(response.urls);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Failed to load connect URLs');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (!copied) {
      return;
    }
    const timer = window.setTimeout(() => setCopied(null), 2000);
    return () => window.clearTimeout(timer);
  }, [copied]);

  const copy = async (url: string) => {
    try {
      await navigator.clipboard.writeText(url);
      setCopied(url);
      setError(null);
    } catch {
      setError('Could not copy to the clipboard');
    }
  };

  if (loading) {
    return <div className="loading-state">Loading connect URLs...</div>;
  }

  const hint = isPhoneReachableHost(window.location.hostname) ? EMPTY_HINT_REACHABLE : EMPTY_HINT;

  return (
    <div className="connect-devices">
      <div className="section-row">
        <div className="section-row-copy">
          <h2 className="section-title">Connect a device</h2>
          <p className="section-description">Paste a URL into the Zone app.</p>
        </div>
      </div>

      {error && <div className="alert alert-error">{error}</div>}

      <div className="settings-card">
        <p className="connect-devices-steps">
          Install or open the Zone app on your phone, enter one of these addresses, then sign in
          with the same account.
        </p>
        {urls.length > 0 ? (
          <div className="connect-url-list">
            {urls.map((url) => (
              <div key={url} className="connect-url-row">
                <code className="connect-url">{url}</code>
                <div className="section-row-actions">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => void copy(url)}
                    aria-label={copied === url ? `Copied ${url}` : `Copy ${url}`}
                  >
                    {copied === url ? 'Copied' : 'Copy'}
                  </Button>
                </div>
              </div>
            ))}
          </div>
        ) : (
          !error && <p className="connect-devices-hint">{hint}</p>
        )}
      </div>
    </div>
  );
}
