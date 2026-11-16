import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { BrowserRouter } from 'react-router-dom';
import { AuthRequestError } from '../../../api/auth';
import { LAST_PATH_KEY, persistLastPath } from '../../../shared/lastPath';
import { SsoCode } from '../SsoCode';
import { NO_SSO, SSO_OFFERED, SSO_REQUIRED } from '../ssoFixtures';
import { SSO_MESSAGES } from '../ssoMessages';

const mockUseAuth = mock();
const mockListProviders = mock();

mock.module('../../../api/sso', () => ({
  authorize: mock(),
  completeCallback: mock(),
  listProviders: mockListProviders,
}));

mock.module('../context', () => ({
  useAuth: mockUseAuth,
}));

// Mock useNavigate
const mockNavigate = mock();
const mockUseLocation = mock(() => ({
  state: null,
  pathname: '/login',
  search: '',
  hash: '',
  key: '',
}));
mock.module('react-router-dom', () => ({
  BrowserRouter: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => (
    <a href={to}>{children}</a>
  ),
  useNavigate: () => mockNavigate,
  useLocation: () => mockUseLocation(),
  useSearchParams: () => [new URLSearchParams(), mock()],
}));

let LoginPage: typeof import('./LoginPage').default;

beforeAll(async () => {
  LoginPage = (await import('./LoginPage')).default;
});

afterAll(() => {
  mock.restore();
});

const renderLoginPage = () => {
  return render(
    <BrowserRouter>
      <LoginPage />
    </BrowserRouter>
  );
};

describe('LoginPage', () => {
  const mockLogin = mock();

  beforeEach(() => {
    mock.clearAllMocks();
    mockListProviders.mockReset();
    mockListProviders.mockRejectedValue(new Error('offline'));
    persistLastPath('/__reset__');
    localStorage.removeItem(LAST_PATH_KEY);
    mockUseLocation.mockReturnValue({
      state: null,
      pathname: '/login',
      search: '',
      hash: '',
      key: '',
    });
    mockUseAuth.mockReturnValue({
      isAuthenticated: false,
      isLoading: false,
      user: null,
      accessToken: null,
      refreshToken: null,
      roles: [],
      permissions: [],
      login: mockLogin,
      register: mock(),
      logout: mock(),
      hasPermission: mock(),
      hasAnyPermission: mock(),
      hasAllPermissions: mock(),
      hasRole: mock(),
    });
  });

  describe('Rendering', () => {
    it('renders login form elements', () => {
      renderLoginPage();

      expect(screen.getByText('Zone')).toBeInTheDocument();
      expect(screen.getByLabelText(/email/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/password/i)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /sign in/i })).toBeInTheDocument();
    });

    it('renders link to register page', () => {
      renderLoginPage();

      expect(screen.getByText(/don't have an account/i)).toBeInTheDocument();
      expect(screen.getByRole('link', { name: /create one/i })).toHaveAttribute(
        'href',
        '/register'
      );
    });

    it('has correct input types', () => {
      renderLoginPage();

      expect(screen.getByLabelText(/email/i)).toHaveAttribute('type', 'email');
      expect(screen.getByLabelText(/password/i)).toHaveAttribute('type', 'password');
    });
  });

  describe('Validation', () => {
    it('shows error when submitting with empty email', async () => {
      renderLoginPage();

      const passwordInput = screen.getByLabelText(/password/i);
      await userEvent.type(passwordInput, 'password123');
      // Use Enter key to submit form reliably in test environment
      await userEvent.type(passwordInput, '{enter}');

      // Zod validation shows "Invalid email address" for empty email
      await waitFor(() => {
        expect(screen.getByText(/invalid email/i)).toBeInTheDocument();
      });
      expect(mockLogin).not.toHaveBeenCalled();
    });

    it('shows error when submitting with empty password', async () => {
      renderLoginPage();

      const emailInput = screen.getByLabelText(/email/i);
      await userEvent.type(emailInput, 'test@example.com');
      // Use Enter key to submit form reliably in test environment
      await userEvent.type(emailInput, '{enter}');

      // Zod validation shows "Password is required" for empty password
      await waitFor(() => {
        expect(screen.getByText(/password is required/i)).toBeInTheDocument();
      });
      expect(mockLogin).not.toHaveBeenCalled();
    });

    it('shows error when submitting with both fields empty', async () => {
      renderLoginPage();

      const emailInput = screen.getByLabelText(/email/i);
      // Use Enter key to submit form reliably in test environment
      await userEvent.type(emailInput, '{enter}');

      // Zod validation shows errors for both fields
      await waitFor(() => {
        expect(screen.getByText(/invalid email/i)).toBeInTheDocument();
      });
      expect(mockLogin).not.toHaveBeenCalled();
    });

    it('focuses the first invalid field', async () => {
      renderLoginPage();

      const emailInput = screen.getByLabelText(/email/i);
      await userEvent.type(emailInput, 'test@example.com');
      await userEvent.type(emailInput, '{enter}');

      await waitFor(() => {
        expect(document.activeElement).toBe(screen.getByLabelText(/password/i));
      });
    });

    it('clears a field error while typing once a submit has been attempted', async () => {
      renderLoginPage();

      const emailInput = screen.getByLabelText(/email/i);
      await userEvent.type(emailInput, 'test@example.com');
      await userEvent.type(emailInput, '{enter}');
      await waitFor(() => {
        expect(screen.getByText(/password is required/i)).toBeInTheDocument();
      });

      await userEvent.type(screen.getByLabelText(/password/i), 'secret');

      await waitFor(() => {
        expect(screen.queryByText(/password is required/i)).not.toBeInTheDocument();
      });
    });

    it('does not validate while typing before the first submit', async () => {
      renderLoginPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'not-an-email');
      await new Promise((resolve) => setTimeout(resolve, 20));

      expect(screen.queryByText(/invalid email/i)).not.toBeInTheDocument();
    });
  });

  describe('Form Submission', () => {
    it('calls login with correct credentials', async () => {
      mockLogin.mockResolvedValue(undefined);
      renderLoginPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      const passwordInput = screen.getByLabelText(/password/i);
      await userEvent.type(passwordInput, 'password123');
      // Use Enter key to submit form reliably in test environment
      await userEvent.type(passwordInput, '{enter}');

      await waitFor(() => {
        expect(mockLogin).toHaveBeenCalledWith({
          email: 'test@example.com',
          password: 'password123',
        });
      });
    });

    it('shows loading state during submission', async () => {
      mockLogin.mockImplementation(() => new Promise((resolve) => setTimeout(resolve, 1000)));
      renderLoginPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /sign in/i }));

      expect(screen.getByRole('button', { name: /signing in/i })).toBeDisabled();
    });

    it('disables inputs during submission', async () => {
      mockLogin.mockImplementation(() => new Promise((resolve) => setTimeout(resolve, 1000)));
      renderLoginPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /sign in/i }));

      expect(screen.getByLabelText(/email/i)).toBeDisabled();
      expect(screen.getByLabelText(/password/i)).toBeDisabled();
    });

    it('navigates to home on successful login', async () => {
      mockLogin.mockResolvedValue(undefined);
      renderLoginPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /sign in/i }));

      await waitFor(() => {
        expect(mockNavigate).toHaveBeenCalledWith('/', { replace: true });
      });
    });

    it('shows error message on login failure', async () => {
      mockLogin.mockRejectedValue(new Error('Invalid credentials'));
      renderLoginPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/password/i), 'wrongpassword');
      await userEvent.click(screen.getByRole('button', { name: /sign in/i }));

      await waitFor(() => {
        expect(screen.getByText(/invalid credentials/i)).toBeInTheDocument();
      });
    });

    it('re-enables form after login failure', async () => {
      mockLogin.mockRejectedValue(new Error('Invalid credentials'));
      renderLoginPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/password/i), 'wrongpassword');
      await userEvent.click(screen.getByRole('button', { name: /sign in/i }));

      await waitFor(() => {
        expect(screen.getByLabelText(/email/i)).not.toBeDisabled();
        expect(screen.getByLabelText(/password/i)).not.toBeDisabled();
        expect(screen.getByRole('button', { name: /sign in/i })).not.toBeDisabled();
      });
    });
  });

  describe('Keyboard Navigation', () => {
    it('submits form on Enter key in password field', async () => {
      mockLogin.mockResolvedValue(undefined);
      renderLoginPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/password/i), 'password123{enter}');

      await waitFor(() => {
        expect(mockLogin).toHaveBeenCalled();
      });
    });
  });

  describe('Redirect Logic', () => {
    it('redirects to original destination from state', async () => {
      mockUseLocation.mockReturnValue({
        state: { from: { pathname: '/chats', search: '?id=chat-1', hash: '' } },
        pathname: '/login',
        search: '',
        hash: '',
        key: '',
      });
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: null,
          is_active: true,
          email_verified: true,
          is_admin: false,
          created_at: '',
          updated_at: '',
          last_login_at: null,
        },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: [],
        permissions: [],
        login: mockLogin,
        register: mock(),
        logout: mock(),
        hasPermission: mock(),
        hasAnyPermission: mock(),
        hasAllPermissions: mock(),
        hasRole: mock(),
      });
      renderLoginPage();

      await waitFor(() => {
        expect(mockNavigate).toHaveBeenCalledWith('/chats?id=chat-1', { replace: true });
      });
    });

    it('redirects to the saved chat when already authenticated', async () => {
      localStorage.setItem(LAST_PATH_KEY, '/?id=chat-9');
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: null,
          is_active: true,
          email_verified: true,
          is_admin: false,
          created_at: '',
          updated_at: '',
          last_login_at: null,
        },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: [],
        permissions: [],
        login: mockLogin,
        register: mock(),
        logout: mock(),
        hasPermission: mock(),
        hasAnyPermission: mock(),
        hasAllPermissions: mock(),
        hasRole: mock(),
      });
      renderLoginPage();

      await waitFor(() => {
        expect(mockNavigate).toHaveBeenCalledWith('/?id=chat-9', { replace: true });
      });
    });

    it('redirects to home when already authenticated', async () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: {
          id: '1',
          email: 'test@test.com',
          display_name: null,
          is_active: true,
          email_verified: true,
          is_admin: false,
          created_at: '',
          updated_at: '',
          last_login_at: null,
        },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: [],
        permissions: [],
        login: mockLogin,
        register: mock(),
        logout: mock(),
        hasPermission: mock(),
        hasAnyPermission: mock(),
        hasAllPermissions: mock(),
        hasRole: mock(),
      });
      renderLoginPage();

      await waitFor(() => {
        expect(mockNavigate).toHaveBeenCalledWith('/', { replace: true });
      });
    });
  });

  describe('Single sign-on', () => {
    it('offers a button per provider above the form, with the divider', async () => {
      mockListProviders.mockResolvedValue(SSO_OFFERED);
      const { container } = renderLoginPage();

      expect(await screen.findByRole('button', { name: 'Sign in with Okta' })).toBeInTheDocument();
      expect(
        screen.getByRole('button', { name: 'Sign in with Microsoft Entra ID' })
      ).toBeInTheDocument();
      const divider = container.querySelector('.auth-divider');
      expect(divider).toHaveTextContent('or');
      expect(divider?.compareDocumentPosition(container.querySelector('form') as Node) ?? 0).toBe(
        Node.DOCUMENT_POSITION_FOLLOWING
      );
    });

    it('shows no buttons and no divider while the providers load', () => {
      mockListProviders.mockReturnValue(new Promise(() => {}));
      const { container } = renderLoginPage();

      expect(screen.queryByRole('button', { name: /sign in with/i })).toBeNull();
      expect(container.querySelector('.auth-divider')).toBeNull();
    });

    it('shows no buttons and no divider when no provider is enabled', async () => {
      mockListProviders.mockResolvedValue(NO_SSO);
      const { container } = renderLoginPage();

      await waitFor(() => expect(mockListProviders).toHaveBeenCalled());
      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(screen.queryByRole('button', { name: /sign in with/i })).toBeNull();
      expect(container.querySelector('.auth-divider')).toBeNull();
      expect(screen.getByRole('link', { name: /forgot password/i })).toBeInTheDocument();
    });

    it('shows no buttons when the providers fail to load', async () => {
      const { container } = renderLoginPage();

      await waitFor(() => expect(mockListProviders).toHaveBeenCalled());
      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(screen.queryByRole('button', { name: /sign in with/i })).toBeNull();
      expect(container.querySelector('.auth-divider')).toBeNull();
    });

    it('keeps the password links when single sign-on is offered but not required', async () => {
      mockListProviders.mockResolvedValue(SSO_OFFERED);
      renderLoginPage();

      await screen.findByRole('button', { name: 'Sign in with Okta' });
      expect(screen.getByRole('link', { name: /forgot password/i })).toBeInTheDocument();
      expect(screen.getByRole('link', { name: /create one/i })).toBeInTheDocument();
      expect(screen.queryByText('Password sign-in is for server admins.')).toBeNull();
    });

    it('hides the forgot-password and register links when single sign-on is required', async () => {
      mockListProviders.mockResolvedValue(SSO_REQUIRED);
      renderLoginPage();

      expect(await screen.findByText('Password sign-in is for server admins.')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sign in with Okta' })).toBeInTheDocument();
      expect(screen.queryByRole('link', { name: /forgot password/i })).toBeNull();
      expect(screen.queryByRole('link', { name: /create one/i })).toBeNull();
      expect(screen.getByLabelText(/email/i)).toBeInTheDocument();
    });

    it('shows the sso_required message when a password sign-in is refused', async () => {
      mockLogin.mockRejectedValue(
        new AuthRequestError(
          'This server requires single sign-on. Sign in from the Zone console in your browser; only server admins can sign in with a password.',
          SsoCode.Required
        )
      );
      renderLoginPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /^sign in$/i }));

      expect(await screen.findByRole('alert')).toHaveTextContent(SSO_MESSAGES[SsoCode.Required]);
    });

    it('shows the account_disabled message when the account is turned off', async () => {
      mockLogin.mockRejectedValue(
        new AuthRequestError('Account disabled', SsoCode.AccountDisabled)
      );
      renderLoginPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /^sign in$/i }));

      expect(await screen.findByRole('alert')).toHaveTextContent(
        SSO_MESSAGES[SsoCode.AccountDisabled]
      );
    });
  });

  describe('Loading State', () => {
    it('shows loading spinner when auth is loading', () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: false,
        isLoading: true,
        user: null,
        accessToken: null,
        refreshToken: null,
        roles: [],
        permissions: [],
        login: mockLogin,
        register: mock(),
        logout: mock(),
        hasPermission: mock(),
        hasAnyPermission: mock(),
        hasAllPermissions: mock(),
        hasRole: mock(),
      });
      renderLoginPage();

      expect(screen.getByText('Loading...')).toBeInTheDocument();
    });

    it('handles non-Error object in catch block', async () => {
      mockLogin.mockRejectedValue('String error');
      renderLoginPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /sign in/i }));

      await waitFor(() => {
        expect(screen.getByText(/login failed/i)).toBeInTheDocument();
      });
    });
  });
});
