import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { client } from '../../../api/client';
import ResendVerificationButton from './ResendVerificationButton';

// Mock client
jest.mock('../../../api/client', () => ({
  client: {
    resendVerification: jest.fn(),
  },
}));

describe('ResendVerificationButton', () => {
  const mockEmail = 'test@example.com';

  beforeEach(() => {
    jest.clearAllMocks();
  });

  describe('Rendering', () => {
    it('renders button with default text', () => {
      render(<ResendVerificationButton email={mockEmail} />);

      expect(screen.getByRole('button', { name: /resend verification/i })).toBeInTheDocument();
    });

    it('renders button as secondary variant by default', () => {
      render(<ResendVerificationButton email={mockEmail} />);

      const button = screen.getByRole('button', { name: /resend verification/i });
      expect(button).toHaveClass('ui-btn--secondary');
    });
  });

  describe('Resend Verification', () => {
    it('calls resendVerification with email when clicked', async () => {
      (client.resendVerification as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Verification email sent',
      });

      render(<ResendVerificationButton email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification/i }));

      expect(client.resendVerification).toHaveBeenCalledWith(mockEmail);
    });

    it('shows loading state during resend', async () => {
      (client.resendVerification as jest.Mock).mockImplementation(
        () => new Promise((resolve) => setTimeout(resolve, 1000))
      );

      render(<ResendVerificationButton email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification/i }));

      expect(screen.getByRole('button', { name: /sending/i })).toBeDisabled();
    });

    it('shows success message after successful resend', async () => {
      (client.resendVerification as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Verification email sent',
      });

      render(<ResendVerificationButton email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification/i }));

      await waitFor(() => {
        expect(screen.getByText(/sent! check your email/i)).toBeInTheDocument();
      });
    });

    it('disables button after successful resend', async () => {
      (client.resendVerification as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Verification email sent',
      });

      render(<ResendVerificationButton email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification/i }));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: /sent! check your email/i })).toBeDisabled();
      });
    });

    it('shows error message on resend failure', async () => {
      (client.resendVerification as jest.Mock).mockRejectedValue(new Error('Rate limit exceeded'));

      render(<ResendVerificationButton email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification/i }));

      await waitFor(() => {
        expect(screen.getByText(/rate limit exceeded/i)).toBeInTheDocument();
      });
    });

    it('re-enables button after error', async () => {
      (client.resendVerification as jest.Mock).mockRejectedValue(new Error('Server error'));

      render(<ResendVerificationButton email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification/i }));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: /resend verification/i })).not.toBeDisabled();
      });
    });

    it('handles non-Error objects in catch block', async () => {
      (client.resendVerification as jest.Mock).mockRejectedValue('String error');

      render(<ResendVerificationButton email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification/i }));

      await waitFor(() => {
        expect(screen.getByText(/failed to send/i)).toBeInTheDocument();
      });
    });
  });

  describe('Callback', () => {
    it('calls onSuccess callback after successful resend', async () => {
      const onSuccess = jest.fn();
      (client.resendVerification as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Sent',
      });

      render(<ResendVerificationButton email={mockEmail} onSuccess={onSuccess} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification/i }));

      await waitFor(() => {
        expect(onSuccess).toHaveBeenCalled();
      });
    });

    it('does not call onSuccess on error', async () => {
      const onSuccess = jest.fn();
      (client.resendVerification as jest.Mock).mockRejectedValue(new Error('Failed'));

      render(<ResendVerificationButton email={mockEmail} onSuccess={onSuccess} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification/i }));

      await waitFor(() => {
        expect(screen.getByText(/failed/i)).toBeInTheDocument();
      });

      expect(onSuccess).not.toHaveBeenCalled();
    });

    it('does not crash when onSuccess is not provided', async () => {
      (client.resendVerification as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Sent',
      });

      render(<ResendVerificationButton email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification/i }));

      await waitFor(() => {
        expect(screen.getByText(/sent! check your email/i)).toBeInTheDocument();
      });
    });
  });

  describe('Custom Props', () => {
    it('accepts custom variant prop', () => {
      render(<ResendVerificationButton email={mockEmail} variant="primary" />);

      const button = screen.getByRole('button', { name: /resend verification/i });
      expect(button).toHaveClass('ui-btn--primary');
    });

    it('accepts custom className prop', () => {
      const { container } = render(
        <ResendVerificationButton email={mockEmail} className="custom-class" />
      );

      expect(container.querySelector('.custom-class')).toBeInTheDocument();
    });
  });
});
