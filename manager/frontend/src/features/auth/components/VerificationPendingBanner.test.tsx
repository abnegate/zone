import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { client } from '../../../api/client';
import VerificationPendingBanner from './VerificationPendingBanner';

// Mock client
jest.mock('../../../api/client', () => ({
  client: {
    resendVerification: jest.fn(),
  },
}));

describe('VerificationPendingBanner', () => {
  const mockEmail = 'test@example.com';

  beforeEach(() => {
    jest.clearAllMocks();
  });

  describe('Rendering', () => {
    it('renders warning banner with email verification message', () => {
      render(<VerificationPendingBanner email={mockEmail} />);

      expect(screen.getByText(/email not verified/i)).toBeInTheDocument();
      expect(
        screen.getByText(/please verify your email address to access all features/i)
      ).toBeInTheDocument();
    });

    it('renders resend verification button', () => {
      render(<VerificationPendingBanner email={mockEmail} />);

      expect(
        screen.getByRole('button', { name: /resend verification email/i })
      ).toBeInTheDocument();
    });

    it('has warning styling', () => {
      const { container } = render(<VerificationPendingBanner email={mockEmail} />);

      expect(container.querySelector('.verification-banner')).toBeInTheDocument();
      expect(container.querySelector('.verification-banner--warning')).toBeInTheDocument();
    });
  });

  describe('Resend Verification', () => {
    it('calls resendVerification with email when button is clicked', async () => {
      (client.resendVerification as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Verification email sent',
      });

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      expect(client.resendVerification).toHaveBeenCalledWith(mockEmail);
    });

    it('shows loading state during resend', async () => {
      (client.resendVerification as jest.Mock).mockImplementation(
        () => new Promise((resolve) => setTimeout(resolve, 1000))
      );

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      expect(screen.getByRole('button', { name: /sending/i })).toBeDisabled();
    });

    it('shows success message after successful resend', async () => {
      (client.resendVerification as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Verification email sent',
      });

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      await waitFor(() => {
        expect(screen.getByText(/verification email sent/i)).toBeInTheDocument();
      });
    });

    it('shows success icon after successful resend', async () => {
      (client.resendVerification as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Verification email sent',
      });

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      await waitFor(() => {
        expect(screen.getByTestId('success-icon')).toBeInTheDocument();
      });
    });

    it('hides resend button after successful resend', async () => {
      (client.resendVerification as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Verification email sent',
      });

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      await waitFor(() => {
        expect(
          screen.queryByRole('button', { name: /resend verification email/i })
        ).not.toBeInTheDocument();
      });
    });

    it('shows error message on resend failure', async () => {
      (client.resendVerification as jest.Mock).mockRejectedValue(new Error('Rate limit exceeded'));

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      await waitFor(() => {
        expect(screen.getByText(/rate limit exceeded/i)).toBeInTheDocument();
      });
    });

    it('re-enables button after error', async () => {
      (client.resendVerification as jest.Mock).mockRejectedValue(new Error('Server error'));

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      await waitFor(() => {
        expect(
          screen.getByRole('button', { name: /resend verification email/i })
        ).not.toBeDisabled();
      });
    });

    it('handles non-Error objects in catch block', async () => {
      (client.resendVerification as jest.Mock).mockRejectedValue('String error');

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      await waitFor(() => {
        expect(screen.getByText(/failed to send verification email/i)).toBeInTheDocument();
      });
    });

    it('clears success message after 5 seconds', async () => {
      jest.useFakeTimers();
      (client.resendVerification as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Verification email sent',
      });

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      await waitFor(() => {
        expect(screen.getByText(/verification email sent/i)).toBeInTheDocument();
      });

      jest.advanceTimersByTime(5000);

      await waitFor(() => {
        expect(screen.queryByText(/verification email sent/i)).not.toBeInTheDocument();
      });

      jest.useRealTimers();
    });

    it('shows resend button again after success message clears with cooldown', async () => {
      jest.useFakeTimers();
      (client.resendVerification as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Verification email sent',
      });

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      await waitFor(() => {
        expect(screen.getByText(/verification email sent/i)).toBeInTheDocument();
      });

      // Success message clears after 5 seconds
      jest.advanceTimersByTime(5000);

      await waitFor(() => {
        expect(screen.queryByText(/verification email sent/i)).not.toBeInTheDocument();
      });

      // Button should be visible but with cooldown countdown
      expect(screen.getByRole('button', { name: /resend \(5\ds\)/i })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /resend \(5\ds\)/i })).toBeDisabled();

      // After 60 seconds total, button should be enabled
      jest.advanceTimersByTime(60000);

      await waitFor(() => {
        expect(
          screen.getByRole('button', { name: /resend verification email/i })
        ).toBeInTheDocument();
        expect(screen.getByRole('button', { name: /resend verification email/i })).toBeEnabled();
      });

      jest.useRealTimers();
    });
  });

  describe('Dismissal', () => {
    it('renders dismiss button', () => {
      render(<VerificationPendingBanner email={mockEmail} />);

      expect(screen.getByRole('button', { name: /dismiss/i })).toBeInTheDocument();
    });

    it('calls onDismiss when dismiss button is clicked', async () => {
      const onDismiss = jest.fn();
      render(<VerificationPendingBanner email={mockEmail} onDismiss={onDismiss} />);

      await userEvent.click(screen.getByRole('button', { name: /dismiss/i }));

      expect(onDismiss).toHaveBeenCalled();
    });

    it('does not crash when onDismiss is not provided', async () => {
      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /dismiss/i }));

      // Should not throw
      expect(true).toBe(true);
    });
  });
});
