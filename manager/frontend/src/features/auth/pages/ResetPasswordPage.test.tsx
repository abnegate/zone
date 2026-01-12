import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock, jest } from 'bun:test';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { client } from '../../../api/client';
import ResetPasswordPage from './ResetPasswordPage';

const validToken = 'valid-token-1234567890abcdef';

const mockResetPassword = mock();
const originalResetPassword = client.resetPassword;

beforeAll(() => {
  client.resetPassword = mockResetPassword as typeof client.resetPassword;
});

afterAll(() => {
  client.resetPassword = originalResetPassword;
  mock.restore();
});

beforeEach(() => {
  mockResetPassword.mockReset();
});

const LocationDisplay = () => {
  const location = useLocation();
  return (
    <div data-testid="location">
      {location.pathname}
      {location.search}
    </div>
  );
};

const renderPage = (token?: string) => {
  const path = token ? `/reset-password?token=${token}` : '/reset-password';
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/reset-password" element={<ResetPasswordPage />} />
        <Route path="/login" element={<div>Login Page</div>} />
      </Routes>
      <LocationDisplay />
    </MemoryRouter>
  );
};

describe('ResetPasswordPage', () => {
  describe('Rendering', () => {
    it('renders reset password form when token is present', () => {
      renderPage(validToken);

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
      renderPage('invalid');

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
      renderPage(validToken);

      const passwordInput = screen.getByLabelText(/^new password$/i);
      const confirmInput = screen.getByLabelText(/confirm password/i);

      expect(passwordInput).toHaveAttribute('type', 'password');
      expect(confirmInput).toHaveAttribute('type', 'password');
      expect(passwordInput).toHaveAttribute('autocomplete', 'new-password');
    });
  });

  describe('Validation', () => {
    it('shows error when submitting empty passwords', async () => {
      renderPage(validToken);

      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      // Empty password will fail one of the validation rules
      expect(screen.getByText(/password must/i)).toBeInTheDocument();
      expect(mockResetPassword).not.toHaveBeenCalled();
    });

    it('shows error when password is too short', async () => {
      renderPage(validToken);

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'Short1');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'Short1');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(screen.getByText(/password must be at least 8 characters/i)).toBeInTheDocument();
      expect(mockResetPassword).not.toHaveBeenCalled();
    });

    it('shows error when password lacks uppercase letter', async () => {
      renderPage(validToken);

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(
        screen.getByText(/password must contain at least one uppercase letter/i)
      ).toBeInTheDocument();
      expect(mockResetPassword).not.toHaveBeenCalled();
    });

    it('shows error when password lacks lowercase letter', async () => {
      renderPage(validToken);

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'PASSWORD123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'PASSWORD123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(
        screen.getByText(/password must contain at least one lowercase letter/i)
      ).toBeInTheDocument();
      expect(mockResetPassword).not.toHaveBeenCalled();
    });

    it('shows error when password lacks number', async () => {
      renderPage(validToken);

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'PasswordABC');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'PasswordABC');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(screen.getByText(/password must contain at least one number/i)).toBeInTheDocument();
      expect(mockResetPassword).not.toHaveBeenCalled();
    });

    it('shows error when passwords do not match', async () => {
      renderPage(validToken);

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'Password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'Different123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(screen.getByText(/passwords do not match/i)).toBeInTheDocument();
      expect(mockResetPassword).not.toHaveBeenCalled();
    });
  });

  describe('Form Submission', () => {
    it('calls resetPassword with token and new password', async () => {
      mockResetPassword.mockResolvedValue({
        success: true,
        message: 'Password reset successfully',
      });

      renderPage(validToken);

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'NewPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'NewPassword123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(mockResetPassword).toHaveBeenCalledWith(validToken, 'NewPassword123');
    });

    it('shows loading state during submission', async () => {
      mockResetPassword.mockImplementation(
        () => new Promise((resolve) => setTimeout(resolve, 1000))
      );

      renderPage(validToken);

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'NewPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'NewPassword123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      expect(screen.getByRole('button', { name: /resetting/i })).toBeDisabled();
      expect(screen.getByLabelText(/^new password$/i)).toBeDisabled();
      expect(screen.getByLabelText(/confirm password/i)).toBeDisabled();
    });

    it('shows success message after successful reset', async () => {
      mockResetPassword.mockResolvedValue({
        success: true,
        message: 'Password reset successfully',
      });

      renderPage(validToken);

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'NewPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'NewPassword123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      await waitFor(() => {
        expect(screen.getByText(/password reset successful/i)).toBeInTheDocument();
      });
    });

    it('redirects to login after 3 seconds on success', async () => {
      mockResetPassword.mockResolvedValue({
        success: true,
        message: 'Password reset successfully',
      });

      renderPage(validToken);

      fireEvent.change(screen.getByLabelText(/^new password$/i), {
        target: { value: 'newPassword123' },
      });
      fireEvent.change(screen.getByLabelText(/confirm password/i), {
        target: { value: 'newPassword123' },
      });
      fireEvent.click(screen.getByRole('button', { name: /reset password/i }));

      await waitFor(() => {
        expect(screen.getByText(/password reset successful/i)).toBeInTheDocument();
      });

      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 3100));
      });

      expect(screen.getByTestId('location')).toHaveTextContent('/login');
    });

    it('shows error message on reset failure', async () => {
      mockResetPassword.mockRejectedValue(new Error('Invalid or expired token'));

      renderPage(validToken);

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'newPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'newPassword123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      await waitFor(() => {
        expect(screen.getByText(/invalid or expired token/i)).toBeInTheDocument();
      });
    });

    it('re-enables form after error', async () => {
      mockResetPassword.mockRejectedValue(new Error('Server error'));

      renderPage(validToken);

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
      mockResetPassword.mockRejectedValue('String error');

      renderPage(validToken);

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'newPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'newPassword123');
      await userEvent.click(screen.getByRole('button', { name: /reset password/i }));

      await waitFor(() => {
        expect(screen.getByText(/failed to reset password/i)).toBeInTheDocument();
      });
    });
  });

  describe('Keyboard Navigation', () => {
    it('submits form on Enter key in confirm password field', async () => {
      mockResetPassword.mockResolvedValue({
        success: true,
        message: 'Reset',
      });

      renderPage(validToken);

      await userEvent.type(screen.getByLabelText(/^new password$/i), 'NewPassword123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'NewPassword123{enter}');

      expect(mockResetPassword).toHaveBeenCalled();
    });
  });
});
