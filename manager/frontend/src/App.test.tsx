import { render } from '@testing-library/react';
import App from './App';

// Mock all the contexts and providers
jest.mock('./features/auth', () => ({
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
  LoginPage: () => <div>Login Page</div>,
  RegisterPage: () => <div>Register Page</div>,
  EmailVerificationPage: () => <div>Email Verification Page</div>,
  ForgotPasswordPage: () => <div>Forgot Password Page</div>,
  ResetPasswordPage: () => <div>Reset Password Page</div>,
  InvitationAcceptPage: () => <div>Invitation Accept Page</div>,
  SessionsPage: () => <div>Sessions Page</div>,
}));

jest.mock('./shared/context/ThemeContext', () => ({
  ThemeProvider: ({ children }: { children: React.ReactNode }) => children,
}));

jest.mock('./shared/context/WorkspaceContext', () => ({
  WorkspaceProvider: ({ children }: { children: React.ReactNode }) => children,
}));

// Mock the Layout component to avoid complex rendering
jest.mock('./shared/components/Layout/Layout', () => {
  return function MockLayout() {
    return <div data-testid="layout">Layout</div>;
  };
});

// Mock ProtectedRoute to just render children
jest.mock('./shared/components/ProtectedRoute', () => {
  return function MockProtectedRoute({ children }: { children: React.ReactNode }) {
    return <>{children}</>;
  };
});

// Mock pages
jest.mock('./pages/UnauthorizedPage', () => () => (
  <div data-testid="unauthorized-page">Unauthorized</div>
));
jest.mock('./features/models', () => ({
  ModelsPage: () => <div data-testid="models-page">Models</div>,
}));
jest.mock('./features/chats', () => ({
  ChatsPage: () => <div data-testid="chats-page">Chats</div>,
}));
jest.mock('./features/projects', () => ({
  ProjectsPage: () => <div data-testid="projects-page">Projects</div>,
}));
jest.mock('./features/tasks', () => ({
  TasksPage: () => <div data-testid="tasks-page">Tasks</div>,
}));
jest.mock('./features/sources', () => ({
  SourcesPage: () => <div data-testid="sources-page">Sources</div>,
}));
jest.mock('./features/knowledge', () => ({
  WikiPage: () => <div data-testid="wiki-page">Wiki</div>,
  ContextSearchPage: () => <div data-testid="context-search-page">ContextSearch</div>,
}));
jest.mock('./features/settings/workspace/pages/WorkspaceSettingsPage', () => ({
  __esModule: true,
  default: () => <div data-testid="settings-page">Settings</div>,
}));
jest.mock('./features/settings/organization/pages/OrgSettingsPage', () => ({
  __esModule: true,
  default: () => <div data-testid="org-settings-page">OrgSettings</div>,
}));

describe('App', () => {
  it('renders without crashing', () => {
    render(<App />);
    // Just verify it rendered without errors
    expect(document.body).toBeInTheDocument();
  });
});
