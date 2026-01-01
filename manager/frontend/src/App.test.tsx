import { render, screen } from '@testing-library/react';
import App from './App';

// Mock all the contexts and providers
jest.mock('./context/AuthContext', () => ({
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

jest.mock('./context/ThemeContext', () => ({
  ThemeProvider: ({ children }: { children: React.ReactNode }) => children,
}));

jest.mock('./context/WorkspaceContext', () => ({
  WorkspaceProvider: ({ children }: { children: React.ReactNode }) => children,
}));

// Mock the Layout component to avoid complex rendering
jest.mock('./components/Layout', () => {
  return function MockLayout() {
    return <div data-testid="layout">Layout</div>;
  };
});

// Mock ProtectedRoute to just render children
jest.mock('./components/ProtectedRoute', () => {
  return function MockProtectedRoute({ children }: { children: React.ReactNode }) {
    return <>{children}</>;
  };
});

// Mock pages
jest.mock('./pages/LoginPage', () => () => <div data-testid="login-page">Login</div>);
jest.mock('./pages/RegisterPage', () => () => <div data-testid="register-page">Register</div>);
jest.mock('./pages/UnauthorizedPage', () => () => <div data-testid="unauthorized-page">Unauthorized</div>);
jest.mock('./pages/ModelsPage', () => () => <div data-testid="models-page">Models</div>);
jest.mock('./pages/ChatsPage', () => () => <div data-testid="chats-page">Chats</div>);
jest.mock('./pages/ProjectsPage', () => () => <div data-testid="projects-page">Projects</div>);
jest.mock('./pages/TasksPage', () => () => <div data-testid="tasks-page">Tasks</div>);
jest.mock('./pages/SourcesPage', () => () => <div data-testid="sources-page">Sources</div>);
jest.mock('./pages/WikiPage', () => () => <div data-testid="wiki-page">Wiki</div>);
jest.mock('./pages/WorkspaceSettingsPage', () => () => <div data-testid="settings-page">Settings</div>);

describe('App', () => {
  it('renders without crashing', () => {
    render(<App />);
    // Just verify it rendered without errors
    expect(document.body).toBeInTheDocument();
  });
});
