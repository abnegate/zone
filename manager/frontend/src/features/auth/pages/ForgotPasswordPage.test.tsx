import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { BrowserRouter } from 'react-router-dom';

// Mock client
const mockForgotPassword = mock();
const mockClient = {
  forgotPassword: mockForgotPassword,
};

mock.module('../../../api/client', () => ({
  client: mockClient,
}));

// Mock useNavigate
const mockNavigate = mock();
mock.module('react-router-dom', () => ({
  BrowserRouter: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => (
    <a href={to}>{children}</a>
  ),
  useNavigate: () => mockNavigate,
  useSearchParams: () => [new URLSearchParams(), mock()],
}));

let ForgotPasswordPage: typeof import('./ForgotPasswordPage').default;

beforeAll(async () => {
  ForgotPasswordPage = (await import('./ForgotPasswordPage')).default;
});

afterAll(() => {
  mock.restore();
});

const renderPage = () => {
  return render(
    <BrowserRouter>
      <ForgotPasswordPage />
    </BrowserRouter>
  );
};

describe('ForgotPasswordPage', () => {
  beforeEach(() => {
    mock.clearAllMocks();
  });

  describe('Rendering', () => {
    it('renders forgot password form', () => {
      renderPage();

      expect(screen.getByText('Zone')).toBeInTheDocument();
      expect(screen.getByText(/reset your password/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/email/i)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /send reset link/i })).toBeInTheDocument();
    });

    it('renders link to login page', () => {
      renderPage();

      expect(screen.getByText(/remember your password/i)).toBeInTheDocument();
      expect(screen.getByRole('link', { name: /sign in/i })).toHaveAttribute('href', '/login');
    });

    it('has email input with correct attributes', () => {
      renderPage();

      const emailInput = screen.getByLabelText(/email/i);
      expect(emailInput).toHaveAttribute('type', 'email');
      expect(emailInput).toHaveAttribute('autocomplete', 'email');
    });
  });

  describe('Validation', () => {
    it('shows error when submitting empty email', async () => {
      renderPage();

      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      expect(screen.getByText(/invalid email address/i)).toBeInTheDocument();
      expect(mockForgotPassword).not.toHaveBeenCalled();
    });

    it('shows error when submitting invalid email format', async () => {
      renderPage();

      const emailInput = screen.getByLabelText(/email/i);
      fireEvent.change(emailInput, { target: { value: 'not-an-email' } });

      // Submit the form
      const form = emailInput.closest('form');
      fireEvent.submit(form!);

      // Wait for the validation error to appear
      await waitFor(() => {
        expect(screen.getByText(/invalid email address/i)).toBeInTheDocument();
      });
      expect(mockForgotPassword).not.toHaveBeenCalled();
    });
  });

  describe('Form Submission', () => {
    it('calls forgotPassword with correct email', async () => {
      mockForgotPassword.mockResolvedValue({
        success: true,
        message: 'Password reset email sent',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      expect(mockForgotPassword).toHaveBeenCalledWith('test@example.com');
    });

    it('trims email before submission', async () => {
      mockForgotPassword.mockResolvedValue({
        success: true,
        message: 'Password reset email sent',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), '  test@example.com  ');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      expect(mockForgotPassword).toHaveBeenCalledWith('test@example.com');
    });

    it('shows loading state during submission', async () => {
      mockForgotPassword.mockImplementation(
        () => new Promise((resolve) => setTimeout(resolve, 1000))
      );

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      expect(screen.getByRole('button', { name: /sending/i })).toBeDisabled();
      expect(screen.getByLabelText(/email/i)).toBeDisabled();
    });

    it('shows success message after successful submission', async () => {
      mockForgotPassword.mockResolvedValue({
        success: true,
        message: 'Password reset email sent',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      await waitFor(() => {
        expect(screen.getByText(/check your email/i)).toBeInTheDocument();
        expect(screen.getByText(/we've sent a password reset link to/i)).toBeInTheDocument();
        expect(screen.getByText('test@example.com')).toBeInTheDocument();
      });
    });

    it('hides form and shows success state', async () => {
      mockForgotPassword.mockResolvedValue({
        success: true,
        message: 'Password reset email sent',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      await waitFor(() => {
        expect(screen.queryByLabelText(/email/i)).not.toBeInTheDocument();
        expect(screen.getByText(/check your email/i)).toBeInTheDocument();
      });
    });

    it('provides link to login after success', async () => {
      mockForgotPassword.mockResolvedValue({
        success: true,
        message: 'Password reset email sent',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      await waitFor(() => {
        const loginLink = screen.getByRole('link', { name: /back to login/i });
        expect(loginLink).toHaveAttribute('href', '/login');
      });
    });

    it('shows error message on submission failure', async () => {
      mockForgotPassword.mockRejectedValue(new Error('User not found'));

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      await waitFor(() => {
        expect(screen.getByText(/user not found/i)).toBeInTheDocument();
      });
    });

    it('re-enables form after error', async () => {
      mockForgotPassword.mockRejectedValue(new Error('Server error'));

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      await waitFor(() => {
        expect(screen.getByLabelText(/email/i)).not.toBeDisabled();
        expect(screen.getByRole('button', { name: /send reset link/i })).not.toBeDisabled();
      });
    });

    it('handles non-Error objects in catch block', async () => {
      mockForgotPassword.mockRejectedValue('String error');

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      await waitFor(() => {
        expect(screen.getByText(/failed to send reset email/i)).toBeInTheDocument();
      });
    });
  });

  describe('Keyboard Navigation', () => {
    it('submits form on Enter key', async () => {
      mockForgotPassword.mockResolvedValue({
        success: true,
        message: 'Sent',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com{enter}');

      expect(mockForgotPassword).toHaveBeenCalled();
    });
  });
});
