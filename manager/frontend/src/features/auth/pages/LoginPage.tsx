import { useEffect } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { Button, Input } from '@zone/ui';
import { useAuth } from '../hooks';
import { LoginRequestSchema } from '../schemas';
import ZoneLogo from '../../../shared/components/ZoneLogo';
import './AuthPage.css';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import type { z } from 'zod';
import { toast } from 'sonner';

type LoginForm = z.infer<typeof LoginRequestSchema>;

export default function LoginPage() {
  const navigate = useNavigate();
  const { login, isAuthenticated, isLoading: authLoading } = useAuth();
  
  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
    setError
  } = useForm<LoginForm>({
    resolver: zodResolver(LoginRequestSchema),
    defaultValues: {
      email: '',
      password: ''
    }
  });

  // Redirect if already authenticated
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
          <ZoneLogo size="xl" />
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

          {errors.root && <div className="auth-error">{errors.root.message}</div>}

          <Button type="submit" variant="primary" loading={isSubmitting} className="btn-block">
            {isSubmitting ? 'Signing in...' : 'Sign In'}
          </Button>

          <div className="auth-link" style={{ textAlign: 'center', marginTop: '0.5rem' }}>
            <Link to="/forgot-password">Forgot password?</Link>
          </div>
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
