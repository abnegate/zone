import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { BrowserRouter } from 'react-router-dom';
import { client } from '../../../api/client';
import ResetPasswordPage from './ResetPasswordPage';

// Mock client
jest.mock('../../../api/client', () => ({
  client: {
    resetPassword: jest.fn(),
  },
}));

// Mock useNavigate and useSearchParams
const mockNavigate = jest.fn();
const mockSearchParams = new URLSearchParams();

jest.mock('react-router-dom', () => ({
  BrowserRouter: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => (
    <a href={to}>{children}</a>
  ),
  useNavigate: () => mockNavigate,
  useSearchParams: () => [mockSearchParams],
}));

const renderPage = () => {
  return render(
    <BrowserRouter>
      <ResetPasswordPage />
    </BrowserRouter>
  );
};

describe('ResetPasswordPage', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    mockSearchParams.delete('token');
  });

  describe('Rendering', () => {
    it('renders reset password form when token is present', () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      renderPage();

      expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Zone');
      expect(screen.getByText(/set new password/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/^new password$/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/confirm password/i)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /reset password/i })).toBeInTheDocument();
    });

    it('shows error when token is missing', () => {
      renderPage();

      expect(screen.getByText(/invalid reset link/i)).toBeInTheDocument();
      expect(screen.getByText(/no token provided/i)).toBeInTheDocument();
      expect(screen.queryByLabelText(/password/i)).not.toBeInTheDocument();
    });

    it('shows error when token has invalid format', () => {
      mockSearchParams.set('token', 'invalid');
      renderPage();

      expect(screen.getByText(/invalid reset link/i)).toBeInTheDocument();
      expect(screen.getByText(/invalid token format/i)).toBeInTheDocument();
      expect(screen.queryByLabelText(/password/i)).not.toBeInTheDocument();
    });

    it('provides link to forgot password page when token missing', () => {
      renderPage();

      const link = screen.getByRole('link', { name: /request new reset link/i });
      expect(link).toHaveAttribute('href', '/forgot-password');
    });

    it('has password inputs with correct attributes', () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      renderPage();

      const passwordInput = screen.getByLabelText(/^new password$/i);
      const confirmInput = screen.getByLabelText(/confirm password/i);

      expect(passwordInput).toHaveAttribute('type', 'password');
      expect(confirmInput).toHaveAttribute('type', 'password');
      expect(passwordInput).toHaveAttribute('autocomplete', 'new-password');
    });
  });

  describe('Validation', () => {
    beforeEach(() => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
    });

    it('shows error when submitting empty passwords', async () => {
      renderPage();

      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      // Empty password will fail one of the validation rules
      expect(screen.getByText(/password must/i)).toBeInTheDocument();
      expect(client.resetPassword).not.toHaveBeenCalled();
    });

    it('shows error when password is too short', async () => {
      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'Short1');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'Short1');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(screen.getByText(/password must be at least 8 characters/i)).toBeInTheDocument();
      expect(client.resetPassword).not.toHaveBeenCalled();
    });

    it('shows error when password lacks uppercase letter', async () => {
      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(
        screen.getByText(/password must contain at least one uppercase letter/i)
      ).toBeInTheDocument();
      expect(client.resetPassword).not.toHaveBeenCalled();
    });

    it('shows error when password lacks lowercase letter', async () => {
      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'PASSWORD123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'PASSWORD123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(
        screen.getByText(/password must contain at least one lowercase letter/i)
      ).toBeInTheDocument();
      expect(client.resetPassword).not.toHaveBeenCalled();
    });

    it('shows error when password lacks number', async () => {
      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'PasswordABC');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'PasswordABC');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(screen.getByText(/password must contain at least one number/i)).toBeInTheDocument();
      expect(client.resetPassword).not.toHaveBeenCalled();
    });

    it('shows error when passwords do not match', async () => {
      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'Password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'Different123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(screen.getByText(/passwords do not match/i)).toBeInTheDocument();
      expect(client.resetPassword).not.toHaveBeenCalled();
    });
  });

  describe('Form Submission', () => {
    beforeEach(() => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
    });

    it('calls resetPassword with token and new password', async () => {
      (client.resetPassword as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Password reset successfully',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'NewPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'NewPassword123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(client.resetPassword).toHaveBeenCalledWith(
        'valid-token-1234567890abcdef',
        'NewPassword123'
      );
    });

    it('shows loading state during submission', async () => {
      (client.resetPassword as jest.Mock).mockImplementation(
        () => new Promise((resolve) => setTimeout(resolve, 1000))
      );

      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'NewPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'NewPassword123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(screen.getByRole('button', { name: /resetting/i })).toBeDisabled();
      expect(screen.getByLabelText(/^new password$/i)).toBeDisabled();
      expect(screen.getByLabelText(/confirm password/i)).toBeDisabled();
    });

    it('shows success message after successful reset', async () => {
      (client.resetPassword as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Password reset successfully',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'NewPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'NewPassword123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      await waitFor(() => {
        expect(screen.getByText(/password reset successful/i)).toBeInTheDocument();
      });
    });

    it('redirects to login after 3 seconds on success', async () => {
      jest.useFakeTimers();
      (client.resetPassword as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Password reset successfully',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'newPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'newPassword123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      await waitFor(() => {
        expect(screen.getByText(/password reset successful/i)).toBeInTheDocument();
      });

      jest.advanceTimersByTime(3000);

      expect(mockNavigate).toHaveBeenCalledWith('/login');

      jest.useRealTimers();
    });

    it('shows error message on reset failure', async () => {
      (client.resetPassword as jest.Mock).mockRejectedValue(new Error('Invalid or expired token'));

      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'newPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'newPassword123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      await waitFor(() => {
        expect(screen.getByText(/invalid or expired token/i)).toBeInTheDocument();
      });
    });

    it('re-enables form after error', async () => {
      (client.resetPassword as jest.Mock).mockRejectedValue(new Error('Server error'));

      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'newPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'newPassword123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      await waitFor(() => {
        expect(screen.getByLabelText(/^new password$/i)).not.toBeDisabled();
        expect(screen.getByLabelText(/confirm password/i)).not.toBeDisabled();
        expect(screen.getByRole('button', { name: /reset password/i })).not.toBeDisabled();
      });
    });

    it('handles non-Error objects in catch block', async () => {
      (client.resetPassword as jest.Mock).mockRejectedValue('String error');

      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'newPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'newPassword123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      await waitFor(() => {
        expect(screen.getByText(/failed to reset password/i)).toBeInTheDocument();
      });
    });
  });

  describe('Keyboard Navigation', () => {
    beforeEach(() => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
    });

    it('submits form on Enter key in confirm password field', async () => {
      (client.resetPassword as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Reset',
      });

      renderPage();

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'NewPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'NewPassword123{enter}');

      expect(client.resetPassword).toHaveBeenCalled();
    });
  });
});
