import { render, screen, waitFor } from '@testing-library/react';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';

// Mock client
const mockVerifyEmail = mock();
const mockClient = {
  verifyEmail: mockVerifyEmail,
};

mock.module('../../../api/client', () => ({
  client: mockClient,
}));

// Mock useNavigate and useSearchParams
const mockNavigate = mock();
const mockSearchParams = new URLSearchParams();

mock.module('react-router-dom', () => ({
  BrowserRouter: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => (
    <a href={to}>{children}</a>
  ),
  useNavigate: () => mockNavigate,
  useSearchParams: () => [mockSearchParams],
}));

let EmailVerificationPage: typeof import('./EmailVerificationPage').default;

beforeAll(async () => {
  EmailVerificationPage = (await import('./EmailVerificationPage')).default;
});

afterAll(() => {
  mock.restore();
});

const BrowserRouter = ({ children }: { children: React.ReactNode }) => <>{children}</>;

const renderPage = () => {
  return render(
    <BrowserRouter>
      <EmailVerificationPage />
    </BrowserRouter>
  );
};

// TODO: Fix timing issues with async useEffect in happy-dom
describe('EmailVerificationPage', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    mockSearchParams.delete('token');
  });

  describe('Rendering', () => {
    it('shows loading state initially when token is present', async () => {
      let resolveVerify: (value: { success: boolean; message: string }) => void;
      const pendingVerify = new Promise<{ success: boolean; message: string }>((resolve) => {
        resolveVerify = resolve;
      });
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      mockVerifyEmail.mockImplementation(() => pendingVerify);
      renderPage();

      expect(screen.getByText(/verifying your email/i)).toBeInTheDocument();

      resolveVerify({ success: true, message: 'Email verified successfully' });

      await waitFor(() => {
        expect(screen.getByText('Email Verified')).toBeInTheDocument();
      });
    });

    it('shows error when token is missing', async () => {
      renderPage();

      await waitFor(() => {
        expect(screen.getByText(/invalid verification link/i)).toBeInTheDocument();
        expect(screen.getByText(/no token provided/i)).toBeInTheDocument();
      });
    });

    it('shows error when token has invalid format', async () => {
      mockSearchParams.set('token', 'invalid');
      renderPage();

      await waitFor(() => {
        expect(screen.getAllByText(/verification failed/i).length).toBeGreaterThan(0);
        expect(screen.getByText(/invalid token format/i)).toBeInTheDocument();
      });
    });
  });

  describe('Verification Process', () => {
    it('calls verifyEmail with token from URL', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      mockVerifyEmail.mockResolvedValue({
        success: true,
        message: 'Email verified successfully',
      });

      renderPage();

      await waitFor(() => {
        expect(mockVerifyEmail).toHaveBeenCalledWith('valid-token-1234567890abcdef');
      });
    });

    it('shows success message on successful verification', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      mockVerifyEmail.mockResolvedValue({
        success: true,
        message: 'Email verified successfully',
      });

      renderPage();

      await waitFor(() => {
        expect(screen.getByText('Email Verified')).toBeInTheDocument();
        expect(screen.getByText('Email verified successfully')).toBeInTheDocument();
      });
    });

    it('shows success icon on successful verification', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      mockVerifyEmail.mockResolvedValue({
        success: true,
        message: 'Email verified successfully',
      });

      renderPage();

      await waitFor(() => {
        const successIcon = screen.getByTestId('success-icon');
        expect(successIcon).toBeInTheDocument();
      });
    });

    it('redirects to login after 3 seconds on success', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      mockVerifyEmail.mockResolvedValue({
        success: true,
        message: 'Email verified successfully',
      });

      renderPage();

      await waitFor(() => {
        expect(screen.getByText('Email Verified')).toBeInTheDocument();
      });

      await waitFor(
        () => {
          expect(mockNavigate).toHaveBeenCalledWith('/login');
        },
        { timeout: 5000 }
      );
    });

    it('shows error message on verification failure', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      mockVerifyEmail.mockRejectedValue(new Error('Invalid or expired token'));

      renderPage();

      await waitFor(() => {
        expect(screen.getByText('Verification Failed')).toBeInTheDocument();
        expect(screen.getByText('Invalid or expired token')).toBeInTheDocument();
      });
    });

    it('shows error icon on verification failure', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      mockVerifyEmail.mockRejectedValue(new Error('Invalid token'));

      renderPage();

      await waitFor(() => {
        const errorIcon = screen.getByTestId('error-icon');
        expect(errorIcon).toBeInTheDocument();
      });
    });

    it('provides link to login page on error', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      mockVerifyEmail.mockRejectedValue(new Error('Invalid token'));

      renderPage();

      await waitFor(() => {
        const loginLink = screen.getByRole('link', { name: /go to login/i });
        expect(loginLink).toHaveAttribute('href', '/login');
      });
    });

    it('handles non-Error objects in catch block', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      mockVerifyEmail.mockRejectedValue('String error');

      renderPage();

      await waitFor(() => {
        expect(screen.getByText('Verification Failed')).toBeInTheDocument();
        expect(screen.getByText('An error occurred')).toBeInTheDocument();
      });
    });
  });

  describe('UI Elements', () => {
    it('displays Zone branding', async () => {
      renderPage();

      expect(screen.getByText('Zone')).toBeInTheDocument();

      await waitFor(() => {
        expect(screen.getByText(/invalid verification link/i)).toBeInTheDocument();
      });
    });

    it('has proper page structure with auth-page class', async () => {
      const { container } = renderPage();

      expect(container.querySelector('.auth-page')).toBeInTheDocument();

      await waitFor(() => {
        expect(screen.getByText(/invalid verification link/i)).toBeInTheDocument();
      });
    });
  });
});
