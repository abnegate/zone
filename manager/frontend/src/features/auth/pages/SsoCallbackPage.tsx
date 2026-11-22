import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import AuthCard from '../components/AuthCard';
import AuthStatus from '../components/AuthStatus';
import { useAuth } from '../context';
import { SsoCode } from '../SsoCode';
import { clearAttempt, readAttempt, type SsoAttempt } from '../ssoAttempt';
import { messageOf, SSO_INCOMPLETE, SSO_MESSAGES, ssoMessage } from '../ssoMessages';
import './AuthPage.css';

interface Callback {
  code: string | null;
  state: string | null;
  error: string | null;
  description: string | null;
}

type Outcome =
  | { kind: 'complete'; next: string; code: string; state: string }
  | { kind: 'failed'; message: string };

function callbackOf(search: string): Callback {
  const parameters = new URLSearchParams(search);
  return {
    code: parameters.get('code'),
    state: parameters.get('state'),
    error: parameters.get('error'),
    description: parameters.get('error_description'),
  };
}

function refusalOf(callback: Callback, error: string): string {
  return (
    ssoMessage(error) ??
    (callback.description?.trim() ||
      `Your identity provider didn't complete the sign-in (${error}).`)
  );
}

function throttled(callback: Callback, attempt: SsoAttempt | null): boolean {
  return attempt !== null && callback.state === null && callback.error === SsoCode.RateLimited;
}

function outcomeOf(callback: Callback, attempt: SsoAttempt | null): Outcome {
  if (throttled(callback, attempt)) {
    return { kind: 'failed', message: SSO_MESSAGES[SsoCode.RateLimited] };
  }
  if (attempt === null || !callback.state || callback.state !== attempt.state) {
    return { kind: 'failed', message: SSO_INCOMPLETE };
  }
  if (callback.error !== null) {
    return { kind: 'failed', message: refusalOf(callback, callback.error) };
  }
  if (!callback.code) {
    return { kind: 'failed', message: SSO_INCOMPLETE };
  }
  return { kind: 'complete', next: attempt.next, code: callback.code, state: callback.state };
}

export default function SsoCallbackPage() {
  const navigate = useNavigate();
  const { completeSso } = useAuth();
  const [outcome] = useState<Outcome>(() =>
    outcomeOf(callbackOf(window.location.search), readAttempt())
  );
  const [failure, setFailure] = useState<string | null>(
    outcome.kind === 'failed' ? outcome.message : null
  );
  const started = useRef(false);

  useLayoutEffect(() => {
    window.history.replaceState(window.history.state, '', window.location.pathname);
    clearAttempt();
  }, []);

  useEffect(() => {
    if (outcome.kind !== 'complete' || started.current) {
      return;
    }
    started.current = true;
    completeSso({ state: outcome.state, code: outcome.code }).then(
      () => navigate(outcome.next, { replace: true }),
      (error: unknown) => setFailure(messageOf(error, 'Single sign-on failed'))
    );
  }, [completeSso, navigate, outcome]);

  if (failure !== null) {
    return (
      <AuthCard subtitle="Single sign-on">
        <AuthStatus
          title="Sign-in didn't complete"
          description={failure}
          action={
            <Link to="/login" className="btn btn-primary">
              Back to sign in
            </Link>
          }
        />
      </AuthCard>
    );
  }

  return (
    <AuthCard subtitle="Single sign-on">
      <div className="auth-loading" role="status">
        <span className="loading-spinner" aria-hidden="true" />
        <span>Signing you in...</span>
      </div>
    </AuthCard>
  );
}
