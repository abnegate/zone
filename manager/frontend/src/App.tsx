import {
  type ComponentType,
  type ReactNode,
  Suspense,
  useEffect,
  useLayoutEffect,
  useState,
} from 'react';
import { BrowserRouter, Navigate, Route, Routes, useLocation, useNavigate } from 'react-router-dom';
import { AuthProvider } from './features/auth/context';
import EmailVerificationPage from './features/auth/pages/EmailVerificationPage';
import ForgotPasswordPage from './features/auth/pages/ForgotPasswordPage';
import InvitationAcceptPage from './features/auth/pages/InvitationAcceptPage';
import LoginPage from './features/auth/pages/LoginPage';
import RegisterPage from './features/auth/pages/RegisterPage';
import ResetPasswordPage from './features/auth/pages/ResetPasswordPage';
import GitHubCallbackOrganization from './features/github/components/GitHubCallbackOrganization';
import SettingsRedirect, { LegacySettings } from './features/settings/navigation/SettingsRedirect';
import UnauthorizedPage from './pages/UnauthorizedPage';
import { loaders } from './routes';
import ProtectedRoute from './shared/components/ProtectedRoute';
import RouteErrorBoundary from './shared/components/RouteErrorBoundary';
import { ThemeProvider, WorkspaceProvider } from './shared/context';
import { WorkspaceTheme } from './shared/context/WorkspaceTheme';
import { lastPathToRestore, persistLastPath } from './shared/lastPath';
import { lazyRoute } from './shared/lazyRoute';
import { PERMISSIONS } from './shared/types/permissions';
import { releaseShell } from './shell/release';

const AuthenticatedProviders = lazyRoute(loaders.authenticatedProviders);
const AgentSignInPage = lazyRoute(loaders.agentSignIn);
const ChatsPage = lazyRoute(loaders.chats);
const Layout = lazyRoute(loaders.layout);
const ModelsPage = lazyRoute(loaders.models);
const ProjectsPage = lazyRoute(loaders.projects);
const SettingsPage = lazyRoute(loaders.settings);
const SetupGate = lazyRoute(loaders.setupGate);
const SetupPage = lazyRoute(loaders.setup);
const SourcesPage = lazyRoute(loaders.sources);
const TasksPage = lazyRoute(loaders.tasks);
const WikiPage = lazyRoute(loaders.wiki);

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

function Unmatched() {
  useLayoutEffect(() => releaseShell(), []);
  return null;
}

function App({
  router: Router = BrowserRouter,
}: {
  router?: ComponentType<{ children: ReactNode }>;
}) {
  return (
    <RouteErrorBoundary>
      <ThemeProvider>
        <AuthProvider>
          <WorkspaceProvider>
            <WorkspaceTheme />
            <Router>
              <LocationSync />
              <RouteErrorBoundary>
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
                          path="settings/*"
                          element={
                            <GitHubCallbackOrganization>
                              <ProtectedRoute>
                                <SettingsPage />
                              </ProtectedRoute>
                            </GitHubCallbackOrganization>
                          }
                        />
                        <Route
                          path="org-settings"
                          element={<SettingsRedirect from={LegacySettings.Organization} />}
                        />
                        <Route
                          path="sessions"
                          element={<SettingsRedirect from={LegacySettings.Sessions} />}
                        />
                      </Route>
                    </Route>
                    <Route path="*" element={<Unmatched />} />
                  </Routes>
                </Suspense>
              </RouteErrorBoundary>
            </Router>
          </WorkspaceProvider>
        </AuthProvider>
      </ThemeProvider>
    </RouteErrorBoundary>
  );
}

export default App;
