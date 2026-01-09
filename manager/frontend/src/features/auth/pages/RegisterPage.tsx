import type React from 'react';
import { type FormEvent, useEffect, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { Button, Input } from '@zone/ui';
import { useAuth } from '../hooks';
import { RegisterRequestSchema } from '../schemas';
import { getErrors } from '../../../validation';
import './AuthPage.css';

export default function RegisterPage() {
  const navigate = useNavigate();
  const { register, isAuthenticated, isLoading: authLoading } = useAuth();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [error, setError] = useState('');
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(false);

  // Redirect if already authenticated
  useEffect(() => {
    if (isAuthenticated && !authLoading) {
      navigate('/', { replace: true });
    }
  }, [isAuthenticated, authLoading, navigate]);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();

    const formData = {
      email: email.trim(),
      password,
      display_name: displayName.trim() || undefined,
    };
    const errors = getErrors(RegisterRequestSchema, formData);

    if (password !== confirmPassword) {
      errors.confirmPassword = 'Passwords do not match';
    }

    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setLoading(true);
    setError('');

    try {
      await register({
        email: email.trim(),
        password,
        display_name: displayName.trim() || undefined,
      });
      navigate('/');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Registration failed');
    } finally {
      setLoading(false);
    }
  };

  if (authLoading) {
    return (
      <div className="auth-page">
        <div className="auth-loading">
          <span className="spinner" />
          <span>Loading...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="auth-page">
      <div className="auth-container">
        <div className="auth-header">
          <h1>Zone</h1>
          <p>Create your account</p>
        </div>

        <form className="auth-form" onSubmit={handleSubmit}>
          <Input
            label="Display Name"
            type="text"
            placeholder="Your name (optional)"
            value={displayName}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) => setDisplayName(e.target.value)}
            disabled={loading}
            autoComplete="name"
          />

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

          <Input
            label="Password"
            type="password"
            placeholder="At least 8 characters"
            value={password}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) => setPassword(e.target.value)}
            disabled={loading}
            autoComplete="new-password"
            error={fieldErrors.password}
          />

          <Input
            label="Confirm Password"
            type="password"
            placeholder="Repeat your password"
            value={confirmPassword}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              setConfirmPassword(e.target.value)
            }
            disabled={loading}
            autoComplete="new-password"
            error={fieldErrors.confirmPassword}
          />

          {error && <div className="auth-error">{error}</div>}

          <Button type="submit" variant="primary" loading={loading} className="btn-block">
            {loading ? 'Creating account...' : 'Create Account'}
          </Button>
        </form>

        <div className="auth-footer">
          <p>
            Already have an account? <Link to="/login">Sign in</Link>
          </p>
        </div>

        <div className="auth-info">
          <p>The first registered user automatically becomes an administrator.</p>
        </div>
      </div>
    </div>
  );
}
