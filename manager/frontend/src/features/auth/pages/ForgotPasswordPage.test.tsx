import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { BrowserRouter } from 'react-router-dom';
import { client } from '../../../api/client';
import ForgotPasswordPage from './ForgotPasswordPage';

// Mock client
jest.mock('../../../api/client', () => ({
  client: {
    forgotPassword: jest.fn(),
  },
}));

// Mock useNavigate
const mockNavigate = jest.fn();
jest.mock('react-router-dom', () => ({
  BrowserRouter: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => (
    <a href={to}>{children}</a>
  ),
  useNavigate: () => mockNavigate,
}));

const renderPage = () => {
  return render(
    <BrowserRouter>
      <ForgotPasswordPage />
    </BrowserRouter>
  );
};

describe('ForgotPasswordPage', () => {
  beforeEach(() => {
    jest.clearAllMocks();
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
      expect(client.forgotPassword).not.toHaveBeenCalled();
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
      expect(client.forgotPassword).not.toHaveBeenCalled();
    });
  });

  describe('Form Submission', () => {
    it('calls forgotPassword with correct email', async () => {
      (client.forgotPassword as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Password reset email sent',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      expect(client.forgotPassword).toHaveBeenCalledWith('test@example.com');
    });

    it('trims email before submission', async () => {
      (client.forgotPassword as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Password reset email sent',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), '  test@example.com  ');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      expect(client.forgotPassword).toHaveBeenCalledWith('test@example.com');
    });

    it('shows loading state during submission', async () => {
      (client.forgotPassword as jest.Mock).mockImplementation(
        () => new Promise((resolve) => setTimeout(resolve, 1000))
      );

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      expect(screen.getByRole('button', { name: /sending/i })).toBeDisabled();
      expect(screen.getByLabelText(/email/i)).toBeDisabled();
    });

    it('shows success message after successful submission', async () => {
      (client.forgotPassword as jest.Mock).mockResolvedValue({
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
      (client.forgotPassword as jest.Mock).mockResolvedValue({
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
      (client.forgotPassword as jest.Mock).mockResolvedValue({
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
      (client.forgotPassword as jest.Mock).mockRejectedValue(new Error('User not found'));

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      await waitFor(() => {
        expect(screen.getByText(/user not found/i)).toBeInTheDocument();
      });
    });

    it('re-enables form after error', async () => {
      (client.forgotPassword as jest.Mock).mockRejectedValue(new Error('Server error'));

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.click(screen.getByRole('button', { name: /send reset link/i }));

      await waitFor(() => {
        expect(screen.getByLabelText(/email/i)).not.toBeDisabled();
        expect(screen.getByRole('button', { name: /send reset link/i })).not.toBeDisabled();
      });
    });

    it('handles non-Error objects in catch block', async () => {
      (client.forgotPassword as jest.Mock).mockRejectedValue('String error');

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
      (client.forgotPassword as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Sent',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com{enter}');

      expect(client.forgotPassword).toHaveBeenCalled();
    });
  });
});
