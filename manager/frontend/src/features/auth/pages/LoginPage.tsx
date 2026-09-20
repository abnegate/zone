import './AuthPage.css';
import { zodResolver } from '@hookform/resolvers/zod';
import { Button, Input } from '@zone/ui';
import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { Link, useNavigate } from 'react-router-dom';
import { toast } from 'sonner';
import type { z } from 'zod';
import ZoneLogo from '../../../shared/components/ZoneLogo';
import { useAuth } from '../hooks';
import { LoginRequestSchema } from '../schemas';

type LoginForm = z.infer<typeof LoginRequestSchema>;

export default function LoginPage() {
  const navigate = useNavigate();
  const { login, isAuthenticated, isLoading: authLoading } = useAuth();

  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
    setError,
  } = useForm<LoginForm>({
    resolver: zodResolver(LoginRequestSchema),
    defaultValues: {
      email: '',
      password: '',
    },
  });

  useEffect(() => {
    if (isAuthenticated && !authLoading) {
      navigate('/', { replace: true });
    }
  }, [isAuthenticated, authLoading, navigate]);

  const onSubmit = async (data: LoginForm) => {
    try {
      await login(data);
      navigate('/');
      toast.success('Successfully logged in');
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Login failed';
      toast.error(message);
      setError('root', { message });
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

        <form className="auth-form" onSubmit={handleSubmit(onSubmit)}>
          <Input
            label="Email"
            type="email"
            placeholder="you@example.com"
            autoFocus
            autoComplete="email"
            error={errors.email?.message}
            disabled={isSubmitting}
            {...register('email')}
          />

          <Input
            label="Password"
            type="password"
            placeholder="Enter your password"
            autoComplete="current-password"
            error={errors.password?.message}
            disabled={isSubmitting}
            {...register('password')}
          />

          <div className="auth-form-links">
            <Link to="/forgot-password">Forgot password?</Link>
          </div>

          {errors.root && (
            <div className="auth-error" role="alert">
              {errors.root.message}
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
