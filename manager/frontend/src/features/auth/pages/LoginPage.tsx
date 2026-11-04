import './AuthPage.css';
import { Button, Input } from '@zone/ui';
import { type FormEvent, useEffect, useRef, useState } from 'react';
import { flushSync } from 'react-dom';
import { Link, useLocation, useNavigate } from 'react-router-dom';
import ZoneLogo from '../../../shared/components/ZoneLogo';
import { type PathLike, pathAfterAuth } from '../../../shared/lastPath';
import { useAuth } from '../context';
import type { LoginRequest } from '../types';

const FIELDS = ['email', 'password'] as const;

function valuesOf(form: HTMLFormElement): LoginRequest {
  const data = new FormData(form);
  return { email: String(data.get('email') ?? ''), password: String(data.get('password') ?? '') };
}

async function validate(values: LoginRequest): Promise<Record<string, string>> {
  const [{ getErrors }, { LoginRequestSchema }] = await Promise.all([
    import('../../../validation'),
    import('../schemas'),
  ]);
  return getErrors(LoginRequestSchema, values);
}

async function notify(kind: 'success' | 'error', message: string) {
  const { toast } = await import('sonner');
  toast[kind](message);
}

function fromLocation(state: unknown): PathLike | null {
  if (!state || typeof state !== 'object' || !('from' in state)) {
    return null;
  }
  const from = (state as { from?: PathLike }).from;
  if (!from || typeof from.pathname !== 'string') {
    return null;
  }
  return from;
}

export default function LoginPage() {
  const navigate = useNavigate();
  const location = useLocation();
  const { login, isAuthenticated, isLoading: authLoading } = useAuth();
  const next = pathAfterAuth(fromLocation(location.state));

  const [errors, setErrors] = useState<Record<string, string>>({});
  const [rootError, setRootError] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const attempted = useRef(false);

  useEffect(() => {
    if (isAuthenticated && !authLoading) {
      navigate(next, { replace: true });
    }
  }, [isAuthenticated, authLoading, navigate, next]);

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const values = valuesOf(form);
    attempted.current = true;
    setRootError('');
    setIsSubmitting(true);

    try {
      const invalid = await validate(values);
      const firstInvalid = FIELDS.find((field) => invalid[field]);
      if (firstInvalid) {
        flushSync(() => {
          setErrors(invalid);
          setIsSubmitting(false);
        });
        form.querySelector<HTMLInputElement>(`[name="${firstInvalid}"]`)?.focus();
        return;
      }
      setErrors({});

      await login(values);
      navigate(next, { replace: true });
      void notify('success', 'Successfully logged in');
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Login failed';
      void notify('error', message);
      setRootError(message);
    } finally {
      setIsSubmitting(false);
    }
  };

  const revalidate = async (event: FormEvent<HTMLFormElement>) => {
    if (attempted.current) {
      const values = valuesOf(event.currentTarget);
      setErrors(await validate(values).catch(() => ({})));
    }
  };

  if (authLoading) {
    return (
      <div className="auth-page">
        <div className="auth-loading">
          <span className="loading-spinner" aria-hidden="true" />
          <span>Loading...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="auth-page">
      <div className="auth-container">
        <div className="auth-header">
          <ZoneLogo size="md" />
          <p>Sign in to your account</p>
        </div>

        <form className="auth-form" onSubmit={handleSubmit} onChange={revalidate}>
          <Input
            label="Email"
            type="email"
            placeholder="you@example.com"
            autoFocus
            autoComplete="email"
            error={errors.email}
            disabled={isSubmitting}
            name="email"
          />

          <Input
            label="Password"
            type="password"
            placeholder="Enter your password"
            autoComplete="current-password"
            error={errors.password}
            disabled={isSubmitting}
            name="password"
          />

          <div className="auth-form-links">
            <Link to="/forgot-password">Forgot password?</Link>
          </div>

          {rootError && (
            <div className="auth-error" role="alert">
              {rootError}
            </div>
          )}

          <Button type="submit" variant="primary" loading={isSubmitting}>
            {isSubmitting ? 'Signing in...' : 'Sign In'}
          </Button>
        </form>

        <div className="auth-footer">
          <p>
            Don't have an account? <Link to="/register">Create one</Link>
          </p>
        </div>
      </div>
    </div>
  );
}
