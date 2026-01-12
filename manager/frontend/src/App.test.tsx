import { render } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterAll, beforeAll, describe, expect, it, mock } from 'bun:test';

mock.module('./features/auth', () => ({
  AuthProvider: ({ children }: { children: ReactNode }) => children,
  useAuth: () => ({ isAuthenticated: false }),
  LoginPage: () => <div>Login Page</div>,
  RegisterPage: () => <div>Register Page</div>,
  EmailVerificationPage: () => <div>Email Verification Page</div>,
  ForgotPasswordPage: () => <div>Forgot Password Page</div>,
  ResetPasswordPage: () => <div>Reset Password Page</div>,
  InvitationAcceptPage: () => <div>Invitation Accept Page</div>,
  SessionsPage: () => <div>Sessions Page</div>,
  VerificationPendingBanner: () => <div>Verification Pending Banner</div>,
  ResendVerificationButton: () => <button type="button">Resend Verification</button>,
}));

mock.module('./shared/context', () => ({
  ThemeProvider: ({ children }: { children: ReactNode }) => children,
  WorkspaceProvider: ({ children }: { children: ReactNode }) => children,
}));

mock.module('./shared/components', () => ({
  Layout: () => <div data-testid="layout">Layout</div>,
  Sidebar: () => <div data-testid="sidebar">Sidebar</div>,
  ContextSwitcher: () => <div data-testid="context-switcher">ContextSwitcher</div>,
  ProtectedRoute: ({ children }: { children: ReactNode }) => <>{children}</>,
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

mock.module('./features/models', () => ({
  ModelsPage: () => <div data-testid="models-page">Models</div>,
}));

mock.module('./features/chats', () => ({
  ChatsPage: () => <div data-testid="chats-page">Chats</div>,
}));

mock.module('./features/projects', () => ({
  ProjectsPage: () => <div data-testid="projects-page">Projects</div>,
}));

mock.module('./features/tasks', () => ({
  TasksPage: () => <div data-testid="tasks-page">Tasks</div>,
}));

mock.module('./features/sources', () => ({
  SourcesPage: () => <div data-testid="sources-page">Sources</div>,
}));

mock.module('./features/knowledge', () => ({
  WikiPage: () => <div data-testid="wiki-page">Wiki</div>,
  ContextSearchPage: () => <div data-testid="context-search-page">ContextSearch</div>,
}));

mock.module('./features/settings', () => ({
  OrgSettingsPage: () => <div data-testid="org-settings-page">OrgSettings</div>,
  WorkspaceSettingsPage: () => <div data-testid="settings-page">Settings</div>,
}));

let App: typeof import('./App').default;

beforeAll(async () => {
  App = (await import('./App')).default;
});

afterAll(() => {
  mock.restore();
});

describe('App', () => {
  it('renders without crashing', () => {
    render(<App />);
    expect(document.body).toBeInTheDocument();
  });
});
