import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

// Mock client
const mockResendVerification = mock();
const mockClient = {
  resendVerification: mockResendVerification,
};

mock.module('../../../api/client', () => ({
  client: mockClient,
}));

let VerificationPendingBanner: typeof import('./VerificationPendingBanner').default;

beforeAll(async () => {
  VerificationPendingBanner = (await import('./VerificationPendingBanner')).default;
});

afterAll(() => {
  mock.restore();
});

/// Hold every callback scheduled at exactly `delay` so a test can fire it,
/// while everything scheduled at any other delay keeps its real timer.
function captureTimer(delay: number) {
  const callbacks: Array<() => void> = [];
  const real = globalThis.setTimeout;
  globalThis.setTimeout = ((callback: () => void, ms?: number, ...rest: unknown[]) => {
    if (ms === delay) {
      callbacks.push(callback);
      return 0 as unknown as ReturnType<typeof real>;
    }
    return (real as (...args: unknown[]) => unknown)(callback, ms, ...rest);
  }) as typeof globalThis.setTimeout;

  return {
    callbacks,
    fire: () => {
      for (const callback of callbacks) {
        callback();
      }
    },
    restore: () => {
      globalThis.setTimeout = real;
    },
  };
}

function captureInterval(delay: number) {
  const callbacks: Array<() => void> = [];
  const real = globalThis.setInterval;
  globalThis.setInterval = ((callback: () => void, ms?: number, ...rest: unknown[]) => {
    if (ms === delay) {
      callbacks.push(callback);
      return 0 as unknown as ReturnType<typeof real>;
    }
    return (real as (...args: unknown[]) => unknown)(callback, ms, ...rest);
  }) as typeof globalThis.setInterval;

  return {
    callbacks,
    fire: () => {
      for (const callback of callbacks) {
        callback();
      }
    },
    restore: () => {
      globalThis.setInterval = real;
    },
  };
}

describe('VerificationPendingBanner', () => {
  const mockEmail = 'test@example.com';

  beforeEach(() => {
    mock.clearAllMocks();
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
      mockResendVerification.mockResolvedValue({
        success: true,
        message: 'Verification email sent',
      });

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      expect(mockResendVerification).toHaveBeenCalledWith(mockEmail);
    });

    it('shows loading state during resend', async () => {
      mockResendVerification.mockImplementation(
        () => new Promise((resolve) => setTimeout(resolve, 1000))
      );

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      expect(screen.getByRole('button', { name: /sending/i })).toBeDisabled();
    });

    it('shows success message after successful resend', async () => {
      mockResendVerification.mockResolvedValue({
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
      mockResendVerification.mockResolvedValue({
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
      mockResendVerification.mockResolvedValue({
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
      mockResendVerification.mockRejectedValue(new Error('Rate limit exceeded'));

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      await waitFor(() => {
        expect(screen.getByText(/rate limit exceeded/i)).toBeInTheDocument();
      });
    });

    it('re-enables button after error', async () => {
      mockResendVerification.mockRejectedValue(new Error('Server error'));

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      await waitFor(() => {
        expect(
          screen.getByRole('button', { name: /resend verification email/i })
        ).not.toBeDisabled();
      });
    });

    it('handles non-Error objects in catch block', async () => {
      mockResendVerification.mockRejectedValue('String error');

      render(<VerificationPendingBanner email={mockEmail} />);

      await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));

      await waitFor(() => {
        expect(screen.getByText(/failed to send verification email/i)).toBeInTheDocument();
      });
    });

    // Fake timers fight bun:test's async handling, so rather than skip the two
    // behaviours that are only reachable through a timer, hold the scheduled
    // callback and fire it. Anything scheduled at another delay -- userEvent's
    // own waiting among it -- still runs for real.
    it('clears the success message once its timer fires', async () => {
      const scheduled = captureTimer(5000);
      try {
        mockResendVerification.mockResolvedValue(undefined);
        render(<VerificationPendingBanner email={mockEmail} />);
        await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));
        await waitFor(() => {
          expect(screen.getByText(/verification email sent/i)).toBeInTheDocument();
        });

        expect(scheduled.callbacks).toHaveLength(1);
        act(() => {
          scheduled.fire();
        });

        await waitFor(() => {
          expect(screen.queryByText(/verification email sent/i)).not.toBeInTheDocument();
        });
      } finally {
        scheduled.restore();
      }
    });

    it('brings the resend button back under its cooldown when the message clears', async () => {
      const message = captureTimer(5000);
      const cooldown = captureInterval(1000);
      try {
        mockResendVerification.mockResolvedValue(undefined);
        render(<VerificationPendingBanner email={mockEmail} />);
        await userEvent.click(screen.getByRole('button', { name: /resend verification email/i }));
        await waitFor(() => {
          expect(screen.getByText(/verification email sent/i)).toBeInTheDocument();
        });

        act(() => {
          cooldown.fire();
          message.fire();
        });

        const button = await screen.findByRole('button', { name: /resend \(\d+s\)/i });
        expect(button).toBeDisabled();
      } finally {
        message.restore();
        cooldown.restore();
      }
    });
  });

  describe('Dismissal', () => {
    it('renders dismiss button', () => {
      render(<VerificationPendingBanner email={mockEmail} />);

      expect(screen.getByRole('button', { name: /dismiss/i })).toBeInTheDocument();
    });

    it('calls onDismiss when dismiss button is clicked', async () => {
      const onDismiss = mock();
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
