import { zodResolver } from '@hookform/resolvers/zod';
import { Button, Card, CardContent, CardDescription, CardHeader, Input } from '@zone/ui';
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
      <div className="flex min-h-screen items-center justify-center bg-background p-4">
        <div className="flex flex-col items-center gap-3 text-muted-foreground">
          <span className="inline-block h-5 w-5 animate-spin rounded-full border-2 border-current border-t-transparent" />
          <span>Loading...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-4">
      <Card className="w-full max-w-md">
        <CardHeader className="text-center">
          <div className="flex justify-center mb-2">
            <ZoneLogo size="xl" />
          </div>
          <CardDescription>Sign in to your account</CardDescription>
        </CardHeader>
        <CardContent>
          <form className="grid gap-4" onSubmit={handleSubmit(onSubmit)}>
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

            {errors.root && (
              <div className="rounded-md bg-destructive/10 border border-destructive/30 p-3 text-sm text-destructive">
                {errors.root.message}
              </div>
            )}

            <Button type="submit" loading={isSubmitting} className="w-full mt-2">
              {isSubmitting ? 'Signing in...' : 'Sign In'}
            </Button>

            <div className="text-center text-sm">
              <Link to="/forgot-password" className="text-primary hover:underline">
                Forgot password?
              </Link>
            </div>
          </form>

          <div className="mt-6 pt-6 border-t text-center text-sm text-muted-foreground">
            Don't have an account?{' '}
            <Link to="/register" className="text-primary font-medium hover:underline">
              Create one
            </Link>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
