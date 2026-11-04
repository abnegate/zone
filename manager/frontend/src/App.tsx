import { Suspense, useEffect, useLayoutEffect, useState } from 'react';
import { BrowserRouter, Navigate, Route, Routes, useLocation, useNavigate } from 'react-router-dom';
import { AuthProvider } from './features/auth/context';
import ForgotPasswordPage from './features/auth/pages/ForgotPasswordPage';
import LoginPage from './features/auth/pages/LoginPage';
import RegisterPage from './features/auth/pages/RegisterPage';
import { loaders } from './routes';
import ProtectedRoute from './shared/components/ProtectedRoute';
import { ThemeProvider, WorkspaceProvider } from './shared/context';
import { WorkspaceTheme } from './shared/context/WorkspaceTheme';
import { lastPathToRestore, persistLastPath } from './shared/lastPath';
import { lazyRoute } from './shared/lazyRoute';
import { PERMISSIONS } from './shared/types/permissions';

const AuthenticatedProviders = lazyRoute(loaders.authenticatedProviders);
const AgentSignInPage = lazyRoute(loaders.agentSignIn);
const ChatsPage = lazyRoute(loaders.chats);
const EmailVerificationPage = lazyRoute(loaders.emailVerification);
const InvitationAcceptPage = lazyRoute(loaders.invitationAccept);
const Layout = lazyRoute(loaders.layout);
const ModelsPage = lazyRoute(loaders.models);
const OrgSettingsPage = lazyRoute(loaders.orgSettings);
const ProjectsPage = lazyRoute(loaders.projects);
const ResetPasswordPage = lazyRoute(loaders.resetPassword);
const SessionsPage = lazyRoute(loaders.sessions);
const SetupGate = lazyRoute(loaders.setupGate);
const SetupPage = lazyRoute(loaders.setup);
const SourcesPage = lazyRoute(loaders.sources);
const TasksPage = lazyRoute(loaders.tasks);
const UnauthorizedPage = lazyRoute(loaders.unauthorized);
const WikiPage = lazyRoute(loaders.wiki);
const WorkspaceSettingsPage = lazyRoute(loaders.workspaceSettings);

function LocationSync() {
  const { pathname, search, hash } = useLocation();
  const navigate = useNavigate();
  const href = `${pathname}${search}${hash}`;
  const [bootRestore] = useState(() => lastPathToRestore(href));
  const [didRestore, setDidRestore] = useState(() => bootRestore === null);

  useLayoutEffect(() => {
    if (!bootRestore || didRestore) {
      return;
    }
    navigate(bootRestore, { replace: true });
    setDidRestore(true);
  }, [bootRestore, didRestore, navigate]);

  useEffect(() => {
    if (!didRestore) {
      return;
    }
    persistLastPath(href);
  }, [didRestore, href]);

  return null;
}

function RouteLoading() {
  return (
    <div className="initial-loader" role="status" aria-label="Loading">
      <h1 className="loader-title">Zone</h1>
      <span className="loader-spinner" />
      <p className="loader-text">Loading...</p>
    </div>
  );
}

function App() {
  return (
    <ThemeProvider>
      <AuthProvider>
        <WorkspaceProvider>
          <WorkspaceTheme />
          <BrowserRouter>
            <LocationSync />
            <Suspense fallback={<RouteLoading />}>
              <Routes>
                <Route path="/login" element={<LoginPage />} />
                <Route path="/register" element={<RegisterPage />} />
                <Route path="/verify-email" element={<EmailVerificationPage />} />
                <Route path="/verify" element={<EmailVerificationPage />} />
                <Route path="/forgot-password" element={<ForgotPasswordPage />} />
                <Route path="/reset-password" element={<ResetPasswordPage />} />
                <Route path="/invitations" element={<InvitationAcceptPage />} />
                <Route path="/unauthorized" element={<UnauthorizedPage />} />
                <Route
                  element={
                    <ProtectedRoute>
                      <AuthenticatedProviders />
                    </ProtectedRoute>
                  }
                >
                  <Route
                    path="/agent-sign-in"
                    element={
                      <ProtectedRoute>
                        <AgentSignInPage />
                      </ProtectedRoute>
                    }
                  />
                  <Route
                    path="/setup"
                    element={
                      <ProtectedRoute requiredPermission={PERMISSIONS.MODELS.READ}>
                        <SetupPage />
                      </ProtectedRoute>
                    }
                  />
                  <Route
                    path="/"
                    element={
                      <ProtectedRoute>
                        <SetupGate>
                          <Layout />
                        </SetupGate>
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
                    <Route path="search" element={<Navigate to="/wiki" replace />} />
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
                        <ProtectedRoute
                          requiredPermission={PERMISSIONS.ORGANIZATIONS.UPDATE}
                          requiredOrganizationRole="admin"
                        >
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
                </Route>
              </Routes>
            </Suspense>
          </BrowserRouter>
        </WorkspaceProvider>
      </AuthProvider>
    </ThemeProvider>
  );
}

export default App;
