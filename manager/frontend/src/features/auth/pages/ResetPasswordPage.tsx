import { Button, Input } from '@zone/ui';
import type React from 'react';
import { type FormEvent, useEffect, useState } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';
import { client } from '../../../api/client';
import ZoneLogo from '../../../shared/components/ZoneLogo';
import { getErrors } from '../../../validation';
import { AuthCard, AuthStatus } from '../components';
import { ResetPasswordSchema } from '../schemas';
import { isValidTokenFormat } from '../utils';
import './AuthPage.css';

export default function ResetPasswordPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const token = searchParams.get('token');

  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [error, setError] = useState('');
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(false);
  const [success, setSuccess] = useState(false);

  useEffect(() => {
    if (success) {
      const timer = setTimeout(() => {
        navigate('/login');
      }, 3000);
      return () => clearTimeout(timer);
    }
  }, [success, navigate]);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();

    if (!token || !isValidTokenFormat(token)) {
      setError('Invalid reset token');
      return;
    }

    const formData = { password, confirmPassword };
    const errors = getErrors(ResetPasswordSchema, formData);

    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setLoading(true);
    setError('');

    try {
      await client.resetPassword(token, password);
      setSuccess(true);
      // Clear password fields from state after success
      setPassword('');
      setConfirmPassword('');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to reset password');
    } finally {
      setLoading(false);
    }
  };

  if (!token || !isValidTokenFormat(token)) {
    return (
      <AuthCard subtitle="Reset your password">
        <AuthStatus
          title="Invalid reset link"
          description={!token ? 'No token provided' : 'Invalid token format'}
          action={
            <Link to="/forgot-password" className="btn btn-primary">
              Request New Reset Link
            </Link>
          }
        />
      </AuthCard>
    );
  }

  if (success) {
    return (
      <div className="auth-page">
        <div className="auth-container">
          <div className="auth-header">
            <ZoneLogo size="md" />
            <p>Password Reset Successful</p>
          </div>

          <div className="auth-success" role="alert" aria-live="polite">
            <div className="success-icon">
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
            <p className="success-message">Your password has been reset successfully</p>
            <p className="redirect-message">Redirecting to login...</p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="auth-page">
      <div className="auth-container">
        <div className="auth-header">
          <ZoneLogo size="md" />
          <p>Set new password</p>
        </div>

        <form className="auth-form" onSubmit={handleSubmit}>
          <Input
            label="New Password"
            type="password"
            placeholder="Enter new password"
            value={password}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) => setPassword(e.target.value)}
            disabled={loading}
            autoFocus
            autoComplete="new-password"
            error={fieldErrors.password}
          />

          <Input
            label="Confirm Password"
            type="password"
            placeholder="Confirm new password"
            value={confirmPassword}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              setConfirmPassword(e.target.value)
            }
            disabled={loading}
            autoComplete="new-password"
            error={fieldErrors.confirmPassword}
          />

          {error && (
            <div className="auth-error" role="alert" aria-live="assertive">
              {error}
            </div>
          )}

          <Button type="submit" variant="primary" loading={loading} className="btn-block">
            {loading ? 'Resetting...' : 'Reset Password'}
          </Button>
        </form>

        <div className="auth-footer">
          <p>
            <Link to="/login">Back to Login</Link>
          </p>
        </div>
      </div>
    </div>
  );
}
