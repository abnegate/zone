import { render, screen, waitFor } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import { client } from '../../../api/client';
import EmailVerificationPage from './EmailVerificationPage';

// Mock client
jest.mock('../../../api/client', () => ({
  client: {
    verifyEmail: jest.fn(),
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
      <EmailVerificationPage />
    </BrowserRouter>
  );
};

// TODO: Fix timing issues with async useEffect in happy-dom
describe.skip('EmailVerificationPage', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    mockSearchParams.delete('token');
  });

  describe('Rendering', () => {
    it('shows loading state initially when token is present', () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      renderPage();

      expect(screen.getByText(/verifying your email/i)).toBeInTheDocument();
    });

    it('shows error when token is missing', () => {
      renderPage();

      expect(screen.getByText(/invalid verification link/i)).toBeInTheDocument();
      expect(screen.getByText(/no token provided/i)).toBeInTheDocument();
    });

    it('shows error when token has invalid format', () => {
      mockSearchParams.set('token', 'invalid');
      renderPage();

      expect(screen.getAllByText(/verification failed/i).length).toBeGreaterThan(0);
      expect(screen.getByText(/invalid token format/i)).toBeInTheDocument();
    });
  });

  describe('Verification Process', () => {
    it('calls verifyEmail with token from URL', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      (client.verifyEmail as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Email verified successfully',
      });

      renderPage();

      await waitFor(() => {
        expect(client.verifyEmail).toHaveBeenCalledWith('valid-token-1234567890abcdef');
      });
    });

    it('shows success message on successful verification', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      (client.verifyEmail as jest.Mock).mockResolvedValue({
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
      (client.verifyEmail as jest.Mock).mockResolvedValue({
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
      jest.useFakeTimers();
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      (client.verifyEmail as jest.Mock).mockResolvedValue({
        success: true,
        message: 'Email verified successfully',
      });

      renderPage();

      await waitFor(() => {
        expect(screen.getByText('Email Verified')).toBeInTheDocument();
      });

      jest.advanceTimersByTime(3000);

      expect(mockNavigate).toHaveBeenCalledWith('/login');

      jest.useRealTimers();
    });

    it('shows error message on verification failure', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      (client.verifyEmail as jest.Mock).mockRejectedValue(new Error('Invalid or expired token'));

      renderPage();

      await waitFor(() => {
        expect(screen.getByText('Verification Failed')).toBeInTheDocument();
        expect(screen.getByText('Invalid or expired token')).toBeInTheDocument();
      });
    });

    it('shows error icon on verification failure', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      (client.verifyEmail as jest.Mock).mockRejectedValue(new Error('Invalid token'));

      renderPage();

      await waitFor(() => {
        const errorIcon = screen.getByTestId('error-icon');
        expect(errorIcon).toBeInTheDocument();
      });
    });

    it('provides link to login page on error', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      (client.verifyEmail as jest.Mock).mockRejectedValue(new Error('Invalid token'));

      renderPage();

      await waitFor(() => {
        const loginLink = screen.getByRole('link', { name: /go to login/i });
        expect(loginLink).toHaveAttribute('href', '/login');
      });
    });

    it('handles non-Error objects in catch block', async () => {
      mockSearchParams.set('token', 'valid-token-1234567890abcdef');
      (client.verifyEmail as jest.Mock).mockRejectedValue('String error');

      renderPage();

      await waitFor(() => {
        expect(screen.getByText('Verification Failed')).toBeInTheDocument();
        expect(screen.getByText('An error occurred')).toBeInTheDocument();
      });
    });
  });

  describe('UI Elements', () => {
    it('displays Zone branding', () => {
      renderPage();

      expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Zone');
    });

    it('has proper page structure with auth-page class', () => {
      const { container } = renderPage();

      expect(container.querySelector('.auth-page')).toBeInTheDocument();
    });
  });
});
