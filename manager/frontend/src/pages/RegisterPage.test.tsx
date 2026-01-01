import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { BrowserRouter } from 'react-router-dom';
import { useAuth } from '../context/AuthContext';
import RegisterPage from './RegisterPage';

// Mock useAuth
jest.mock('../context/AuthContext');
const mockUseAuth = useAuth as jest.MockedFunction<typeof useAuth>;

// Mock useNavigate
const mockNavigate = jest.fn();
jest.mock('react-router-dom', () => ({
  BrowserRouter: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => (
    <a href={to}>{children}</a>
  ),
  useNavigate: () => mockNavigate,
}));

const renderRegisterPage = () => {
  return render(
    <BrowserRouter>
      <RegisterPage />
    </BrowserRouter>
  );
};

describe('RegisterPage', () => {
  const mockRegister = jest.fn();

  beforeEach(() => {
    jest.clearAllMocks();
    mockUseAuth.mockReturnValue({
      isAuthenticated: false,
      isLoading: false,
      user: null,
      accessToken: null,
      refreshToken: null,
      roles: [],
      permissions: [],
      login: jest.fn(),
      register: mockRegister,
      logout: jest.fn(),
      hasPermission: jest.fn(),
      hasAnyPermission: jest.fn(),
      hasAllPermissions: jest.fn(),
      hasRole: jest.fn(),
    });
  });

  describe('Rendering', () => {
    it('renders registration form elements', () => {
      renderRegisterPage();

      expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Zone');
      expect(screen.getByLabelText(/email/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/^password$/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/confirm password/i)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /create account/i })).toBeInTheDocument();
    });

    it('renders optional display name field', () => {
      renderRegisterPage();

      expect(screen.getByLabelText(/display name/i)).toBeInTheDocument();
    });

    it('renders link to login page', () => {
      renderRegisterPage();

      expect(screen.getByText(/already have an account/i)).toBeInTheDocument();
      expect(screen.getByRole('link', { name: /sign in/i })).toHaveAttribute('href', '/login');
    });
  });

  describe('Validation', () => {
    // Note: These tests are skipped because the component uses HTML5 native validation
    // (required, minLength attributes) which triggers before our custom JS validation.
    // The validation logic is tested through E2E tests instead.

    it('shows error when passwords do not match', async () => {
      renderRegisterPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/^password$/i), 'password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'different123');
      await userEvent.click(screen.getByRole('button', { name: /create account/i }));

      expect(screen.getByText(/passwords do not match/i)).toBeInTheDocument();
      expect(mockRegister).not.toHaveBeenCalled();
    });
  });

  describe('Form Submission', () => {
    it('calls register with correct data', async () => {
      mockRegister.mockResolvedValue(undefined);
      renderRegisterPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'new@example.com');
      await userEvent.type(screen.getByLabelText(/display name/i), 'New User');
      await userEvent.type(screen.getByLabelText(/^password$/i), 'password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /create account/i }));

      expect(mockRegister).toHaveBeenCalledWith({
        email: 'new@example.com',
        password: 'password123',
        display_name: 'New User',
      });
    });

    it('calls register without display_name when not provided', async () => {
      mockRegister.mockResolvedValue(undefined);
      renderRegisterPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'new@example.com');
      await userEvent.type(screen.getByLabelText(/^password$/i), 'password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /create account/i }));

      expect(mockRegister).toHaveBeenCalledWith({
        email: 'new@example.com',
        password: 'password123',
        display_name: undefined,
      });
    });

    it('shows loading state during submission', async () => {
      mockRegister.mockImplementation(() => new Promise((resolve) => setTimeout(resolve, 1000)));
      renderRegisterPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'new@example.com');
      await userEvent.type(screen.getByLabelText(/^password$/i), 'password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /create account/i }));

      expect(screen.getByRole('button', { name: /creating/i })).toBeDisabled();
    });

    it('disables all inputs during submission', async () => {
      mockRegister.mockImplementation(() => new Promise((resolve) => setTimeout(resolve, 1000)));
      renderRegisterPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'new@example.com');
      await userEvent.type(screen.getByLabelText(/^password$/i), 'password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /create account/i }));

      expect(screen.getByLabelText(/email/i)).toBeDisabled();
      expect(screen.getByLabelText(/display name/i)).toBeDisabled();
      expect(screen.getByLabelText(/^password$/i)).toBeDisabled();
      expect(screen.getByLabelText(/confirm password/i)).toBeDisabled();
    });

    it('navigates to home on successful registration', async () => {
      mockRegister.mockResolvedValue(undefined);
      renderRegisterPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'new@example.com');
      await userEvent.type(screen.getByLabelText(/^password$/i), 'password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /create account/i }));

      await waitFor(() => {
        expect(mockNavigate).toHaveBeenCalledWith('/');
      });
    });

    it('shows error message on registration failure', async () => {
      mockRegister.mockRejectedValue(new Error('Email already exists'));
      renderRegisterPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'existing@example.com');
      await userEvent.type(screen.getByLabelText(/^password$/i), 'password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /create account/i }));

      await waitFor(() => {
        expect(screen.getByText(/email already exists/i)).toBeInTheDocument();
      });
    });

    it('re-enables form after registration failure', async () => {
      mockRegister.mockRejectedValue(new Error('Registration failed'));
      renderRegisterPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'new@example.com');
      await userEvent.type(screen.getByLabelText(/^password$/i), 'password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /create account/i }));

      await waitFor(() => {
        expect(screen.getByLabelText(/email/i)).not.toBeDisabled();
        expect(screen.getByLabelText(/^password$/i)).not.toBeDisabled();
        expect(screen.getByLabelText(/confirm password/i)).not.toBeDisabled();
        expect(screen.getByRole('button', { name: /create account/i })).not.toBeDisabled();
      });
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
        login: jest.fn(),
        register: mockRegister,
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });
      renderRegisterPage();

      expect(screen.getByText('Loading...')).toBeInTheDocument();
    });

    it('redirects to home when already authenticated', async () => {
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        isLoading: false,
        user: { id: '1', email: 'test@test.com', display_name: null, is_active: true, is_admin: false, created_at: '', updated_at: '', last_login_at: null },
        accessToken: 'token',
        refreshToken: 'refresh',
        roles: [],
        permissions: [],
        login: jest.fn(),
        register: mockRegister,
        logout: jest.fn(),
        hasPermission: jest.fn(),
        hasAnyPermission: jest.fn(),
        hasAllPermissions: jest.fn(),
        hasRole: jest.fn(),
      });
      renderRegisterPage();

      await waitFor(() => {
        expect(mockNavigate).toHaveBeenCalledWith('/', { replace: true });
      });
    });
  });

  describe('Edge Cases', () => {
    it('handles non-Error object in catch block', async () => {
      mockRegister.mockRejectedValue('String error');
      renderRegisterPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/^password$/i), 'password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /create account/i }));

      await waitFor(() => {
        expect(screen.getByText(/registration failed/i)).toBeInTheDocument();
      });
    });

    it('trims whitespace from email', async () => {
      mockRegister.mockResolvedValue(undefined);
      renderRegisterPage();

      await userEvent.type(screen.getByLabelText(/email/i), '  spacy@example.com  ');
      await userEvent.type(screen.getByLabelText(/^password$/i), 'password123');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'password123');
      await userEvent.click(screen.getByRole('button', { name: /create account/i }));

      expect(mockRegister).toHaveBeenCalledWith(
        expect.objectContaining({
          email: 'spacy@example.com',
        })
      );
    });

    it('accepts passwords at exactly 8 characters', async () => {
      mockRegister.mockResolvedValue(undefined);
      renderRegisterPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/^password$/i), 'exactly8');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'exactly8');
      await userEvent.click(screen.getByRole('button', { name: /create account/i }));

      expect(mockRegister).toHaveBeenCalled();
    });

    it('handles special characters in password', async () => {
      mockRegister.mockResolvedValue(undefined);
      renderRegisterPage();

      await userEvent.type(screen.getByLabelText(/email/i), 'test@example.com');
      await userEvent.type(screen.getByLabelText(/^password$/i), 'P@$$w0rd!#');
      await userEvent.type(screen.getByLabelText(/confirm password/i), 'P@$$w0rd!#');
      await userEvent.click(screen.getByRole('button', { name: /create account/i }));

      expect(mockRegister).toHaveBeenCalledWith(
        expect.objectContaining({
          password: 'P@$$w0rd!#',
        })
      );
    });
  });
});
