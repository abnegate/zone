import type React from 'react';
import { type FormEvent, useState } from 'react';
import { Link } from 'react-router-dom';
import { client } from '../../../api/client';
import { Button, Input } from '@zone/ui';
import { ForgotPasswordSchema } from '../schemas';
import { getErrors } from '../../../validation';
import ZoneLogo from '../../../shared/components/ZoneLogo';
import './AuthPage.css';

export default function ForgotPasswordPage() {
  const [email, setEmail] = useState('');
  const [error, setError] = useState('');
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(false);
  const [submitted, setSubmitted] = useState(false);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const formData = { email: email.trim() };
    const errors = getErrors(ForgotPasswordSchema, formData);

    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setLoading(true);
    setError('');

    try {
      await client.forgotPassword(email.trim());
      setSubmitted(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to send reset email');
    } finally {
      setLoading(false);
    }
  };

  if (submitted) {
    return (
      <div className="auth-page">
        <div className="auth-container">
          <div className="auth-header">
            <ZoneLogo size="xl" />
            <p>Check your email</p>
          </div>

          <div className="auth-success">
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
                <path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z" />
                <polyline points="22,6 12,13 2,6" />
              </svg>
            </div>
            <p className="success-message">
              We've sent a password reset link to <strong>{email}</strong>
            </p>
            <p className="info-message">
              If you don't see the email, check your spam folder or request a new link.
            </p>
          </div>

          <div className="auth-footer">
            <p>
              <Link to="/login">Back to Login</Link>
            </p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="auth-page">
      <div className="auth-container">
        <div className="auth-header">
          <ZoneLogo size="xl" />
          <p>Reset your password</p>
        </div>

        <form className="auth-form" onSubmit={handleSubmit}>
          <Input
            label="Email"
            type="email"
            placeholder="you@example.com"
            value={email}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) => setEmail(e.target.value)}
            disabled={loading}
            autoFocus
            autoComplete="email"
            error={fieldErrors.email}
          />

          {error && <div className="auth-error">{error}</div>}

          <Button type="submit" variant="primary" loading={loading} className="btn-block">
            {loading ? 'Sending...' : 'Send Reset Link'}
          </Button>
        </form>

        <div className="auth-footer">
          <p>
            Remember your password? <Link to="/login">Sign in</Link>
          </p>
        </div>
      </div>
    </div>
  );
}
