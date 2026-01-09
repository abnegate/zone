import { useEffect, useState } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';
import { client } from '../../../api/client';
import { isValidTokenFormat } from '../utils';
import './AuthPage.css';

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
        const result = await client.verifyEmail(token);
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

  return (
    <div className="auth-page">
      <div className="auth-container">
        <div className="auth-header">
          <h1>Zone</h1>
          {status === 'loading' && <p>Verifying your email...</p>}
          {status === 'success' && <p>Email Verified</p>}
          {status === 'error' && <p>Verification Failed</p>}
        </div>

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
                  width="64"
                  height="64"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
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
            <div className="auth-error-state" role="alert" aria-live="assertive">
              <div className="error-icon" data-testid="error-icon">
                <svg
                  width="64"
                  height="64"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <circle cx="12" cy="12" r="10" />
                  <line x1="15" y1="9" x2="9" y2="15" />
                  <line x1="9" y1="9" x2="15" y2="15" />
                </svg>
              </div>
              <p className="error-title">
                {!token ? 'Invalid verification link' : 'Verification failed'}
              </p>
              <p className="error-message">{message}</p>
              <Link to="/login" className="btn btn-primary">
                Go to Login
              </Link>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
