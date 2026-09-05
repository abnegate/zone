import { BrowserRouter, Route, Routes } from 'react-router-dom';
import {
  AuthProvider,
  EmailVerificationPage,
  ForgotPasswordPage,
  InvitationAcceptPage,
  LoginPage,
  RegisterPage,
  ResetPasswordPage,
  SessionsPage,
} from './features/auth';
import { ChatsPage } from './features/chats';
import { ContextSearchPage, WikiPage } from './features/knowledge';
import { ModelsPage, PullDownloadIndicator, PullProvider } from './features/models';
import { ProjectsPage } from './features/projects';
import { OrgSettingsPage, WorkspaceSettingsPage } from './features/settings';
import { SourcesPage } from './features/sources';
import { TasksPage } from './features/tasks';
import UnauthorizedPage from './pages/UnauthorizedPage';
import { Layout, ProtectedRoute } from './shared/components';
import { ThemeProvider, WorkspaceProvider } from './shared/context';
import { WorkspaceTheme } from './shared/context/WorkspaceTheme';
import { PERMISSIONS } from './shared/types/permissions';
import './App.css';

function App() {
  return (
    <ThemeProvider>
      <AuthProvider>
        <PullProvider>
          <WorkspaceProvider>
            <WorkspaceTheme />
            <BrowserRouter>
              <PullDownloadIndicator />
              <Routes>
                {/* Public routes */}
                <Route path="/login" element={<LoginPage />} />
                <Route path="/register" element={<RegisterPage />} />
                <Route path="/verify-email" element={<EmailVerificationPage />} />
                <Route path="/forgot-password" element={<ForgotPasswordPage />} />
                <Route path="/reset-password" element={<ResetPasswordPage />} />
                <Route path="/invitations" element={<InvitationAcceptPage />} />
                <Route path="/unauthorized" element={<UnauthorizedPage />} />

                {/* Protected routes */}
                <Route
                  path="/"
                  element={
                    <ProtectedRoute>
                      <Layout />
                    </ProtectedRoute>
                  }
                >
                  <Route
                    index
                    element={
                      <ProtectedRoute requiredPermission={PERMISSIONS.CHATS.READ}>
                        <ChatsPage />
                      </ProtectedRoute>
                    }
                  />
                  <Route
                    path="chats"
                    element={
                      <ProtectedRoute requiredPermission={PERMISSIONS.CHATS.READ}>
                        <ChatsPage />
                      </ProtectedRoute>
                    }
                  />
                  <Route
                    path="models"
                    element={
                      <ProtectedRoute requiredPermission={PERMISSIONS.MODELS.READ}>
                        <ModelsPage />
                      </ProtectedRoute>
                    }
                  />
                  <Route
                    path="projects"
                    element={
                      <ProtectedRoute requiredPermission={PERMISSIONS.PROJECTS.READ}>
                        <ProjectsPage />
                      </ProtectedRoute>
                    }
                  />
                  <Route
                    path="tasks"
                    element={
                      <ProtectedRoute requiredPermission={PERMISSIONS.TASKS.READ}>
                        <TasksPage />
                      </ProtectedRoute>
                    }
                  />
                  <Route
                    path="sources"
                    element={
                      <ProtectedRoute requiredPermission={PERMISSIONS.SOURCES.READ}>
                        <SourcesPage />
                      </ProtectedRoute>
                    }
                  />
                  <Route
                    path="search"
                    element={
                      <ProtectedRoute requiredPermission={PERMISSIONS.SOURCES.READ}>
                        <ContextSearchPage />
                      </ProtectedRoute>
                    }
                  />
                  <Route
                    path="wiki"
                    element={
                      <ProtectedRoute requiredPermission={PERMISSIONS.WIKI.READ}>
                        <WikiPage />
                      </ProtectedRoute>
                    }
                  />
                  <Route
                    path="org-settings"
                    element={
                      <ProtectedRoute requiredPermission={PERMISSIONS.ORGANIZATIONS.UPDATE}>
                        <OrgSettingsPage />
                      </ProtectedRoute>
                    }
                  />
                  <Route
                    path="settings"
                    element={
                      <ProtectedRoute requiredPermission={PERMISSIONS.WORKSPACES.UPDATE}>
                        <WorkspaceSettingsPage />
                      </ProtectedRoute>
                    }
                  />
                  <Route
                    path="sessions"
                    element={
                      <ProtectedRoute>
                        <SessionsPage />
                      </ProtectedRoute>
                    }
                  />
                </Route>
              </Routes>
            </BrowserRouter>
          </WorkspaceProvider>
        </PullProvider>
      </AuthProvider>
    </ThemeProvider>
  );
}

export default App;
