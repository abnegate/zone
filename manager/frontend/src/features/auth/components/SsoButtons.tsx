import { Button } from '@zone/ui';
import { useState } from 'react';
import { authorize } from '../../../api/sso';
import { saveAttempt } from '../ssoAttempt';
import { messageOf } from '../ssoMessages';
import type { SsoPublicProvider, SsoPublicProviders } from '../types';

interface SsoButtonsProps {
  sso: SsoPublicProviders | null;
  next: string;
}

const STORAGE_BLOCKED =
  'Your browser blocked session storage, which single sign-on needs. Allow it for this site and try again.';

const LOGIN_PATH = '/login';
const WEB_PROTOCOLS = new Set(['https:', 'http:']);

function loginElsewhere(callbackOrigin: string): string | null {
  try {
    const login = new URL(LOGIN_PATH, callbackOrigin);
    return WEB_PROTOCOLS.has(login.protocol) && login.origin !== window.location.origin
      ? login.href
      : null;
  } catch {
    return null;
  }
}

export default function SsoButtons({ sso, next }: SsoButtonsProps) {
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState('');

  if (!sso || sso.providers.length === 0) {
    return null;
  }

  const elsewhere = loginElsewhere(sso.callback_origin);

  const start = async (provider: SsoPublicProvider) => {
    if (elsewhere) {
      window.location.assign(elsewhere);
      return;
    }
    setPending(provider.id);
    setError('');
    try {
      const authorization = await authorize(provider.id);
      try {
        saveAttempt({ state: authorization.state, next });
      } catch {
        setError(STORAGE_BLOCKED);
        setPending(null);
        return;
      }
      window.location.assign(authorization.url);
    } catch (caught) {
      setError(messageOf(caught, 'Single sign-on could not start'));
      setPending(null);
    }
  };

  return (
    <div className="auth-sso">
      <div className="auth-sso-buttons">
        {sso.providers.map((provider) => (
          <Button
            key={provider.id}
            type="button"
            variant="outline"
            loading={pending === provider.id}
            disabled={pending !== null}
            onClick={() => void start(provider)}
          >
            {`Sign in with ${provider.display_name}`}
          </Button>
        ))}
      </div>
      {error && (
        <div className="auth-error" role="alert">
          {error}
        </div>
      )}
      <div className="auth-divider">
        <span>or</span>
      </div>
    </div>
  );
}
