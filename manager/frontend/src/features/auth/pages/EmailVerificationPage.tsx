import { useEffect, useState } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';
import { client } from '../../../api/client';
import { AuthCard, AuthStatus } from '../components';
import { isValidTokenFormat } from '../utils';
import './AuthPage.css';

// One request per token while it is in flight: StrictMode mounts the page
// twice, and a second call would find the single-use token consumed.
const inFlight = new Map<string, Promise<{ message: string }>>();

function verifyOnce(token: string): Promise<{ message: string }> {
  const pending = inFlight.get(token);
  if (pending) return pending;
  const request = client.verifyEmail(token).finally(() => inFlight.delete(token));
  inFlight.set(token, request);
  return request;
}

export default function EmailVerificationPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const [status, setStatus] = useState<'loading' | 'success' | 'error'>('loading');
  const [message, setMessage] = useState('');
  const token = searchParams.get('token');

  useEffect(() => {
    if (!token) {
      setStatus('error');
      setMessage('No token provided');
      return;
    }

    if (!isValidTokenFormat(token)) {
      setStatus('error');
      setMessage('Invalid token format');
      return;
    }

    const abortController = new AbortController();
    let redirectTimeout: NodeJS.Timeout;

    const verify = async () => {
      try {
        const result = await verifyOnce(token);
        if (abortController.signal.aborted) return;

        setStatus('success');
        setMessage(result.message);

        // Redirect to login after 3 seconds
        redirectTimeout = setTimeout(() => {
          if (!abortController.signal.aborted) {
            navigate('/login');
          }
        }, 3000);
      } catch (err) {
        if (abortController.signal.aborted) return;

        setStatus('error');
        setMessage(err instanceof Error ? err.message : 'An error occurred');
      }
    };

    verify();

    return () => {
      abortController.abort();
      if (redirectTimeout) clearTimeout(redirectTimeout);
    };
  }, [token, navigate]);

  const subtitle =
    status === 'loading'
      ? 'Verifying your email...'
      : status === 'success'
        ? 'Email Verified'
        : undefined;

  return (
    <AuthCard subtitle={subtitle}>
      <div className="auth-content">
        {status === 'loading' && (
          <div className="auth-loading">
            <span className="spinner" />
          </div>
        )}

        {status === 'success' && (
          <div className="auth-success" role="alert" aria-live="polite">
            <div className="success-icon" data-testid="success-icon">
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
                <polyline points="22 4 12 14.01 9 11.01" />
              </svg>
            </div>
            <p className="success-message">{message}</p>
            <p className="redirect-message">Redirecting to login...</p>
          </div>
        )}

        {status === 'error' && (
          <AuthStatus
            title={!token ? 'Invalid verification link' : 'Verification failed'}
            description={message}
            action={
              <Link to="/login" className="btn btn-primary">
                Go to Login
              </Link>
            }
          />
        )}
      </div>
    </AuthCard>
  );
}
