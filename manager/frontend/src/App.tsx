import { type ComponentType, lazy, Suspense, useEffect, useLayoutEffect, useState } from 'react';
import { BrowserRouter, Navigate, Route, Routes, useLocation, useNavigate } from 'react-router-dom';
import { AuthProvider } from './features/auth/context';
import ForgotPasswordPage from './features/auth/pages/ForgotPasswordPage';
import LoginPage from './features/auth/pages/LoginPage';
import RegisterPage from './features/auth/pages/RegisterPage';
import ProtectedRoute from './shared/components/ProtectedRoute';
import { ThemeProvider, WorkspaceProvider } from './shared/context';
import { WorkspaceTheme } from './shared/context/WorkspaceTheme';
import { lastPathToRestore, persistLastPath } from './shared/lastPath';
import { loadFeatureStylesheet } from './shared/stylesheet';
import { PERMISSIONS } from './shared/types/permissions';

function lazyRoute<Props>(load: () => Promise<{ default: ComponentType<Props> }>) {
  return lazy(async () => {
    const [module] = await Promise.all([load(), loadFeatureStylesheet()]);
    return module;
  });
}

const AuthenticatedProviders = lazyRoute(
  () => import('./shared/components/AuthenticatedProviders')
);
const AgentSignInPage = lazyRoute(() => import('./features/settings/ai/AgentSignInPage'));
const ChatsPage = lazyRoute(() => import('./features/chats/pages/ChatsPage'));
const EmailVerificationPage = lazyRoute(
  () => import('./features/auth/pages/EmailVerificationPage')
);
const InvitationAcceptPage = lazyRoute(() => import('./features/auth/pages/InvitationAcceptPage'));
const Layout = lazyRoute(() => import('./shared/components/Layout/Layout'));
const ModelsPage = lazyRoute(() => import('./features/models/pages/ModelsPage'));
const OrgSettingsPage = lazyRoute(
  () => import('./features/settings/organization/pages/OrgSettingsPage')
);
const ProjectsPage = lazyRoute(() => import('./features/projects/pages/ProjectsPage'));
const ResetPasswordPage = lazyRoute(() => import('./features/auth/pages/ResetPasswordPage'));
const SessionsPage = lazyRoute(() => import('./features/auth/pages/SessionsPage'));
const SetupGate = lazyRoute(() => import('./features/models/components/SetupGate'));
const SetupPage = lazyRoute(() => import('./features/models/pages/SetupPage'));
const SourcesPage = lazyRoute(() => import('./features/sources/pages/SourcesPage'));
const TasksPage = lazyRoute(() => import('./features/tasks/pages/TasksPage'));
const UnauthorizedPage = lazyRoute(() => import('./pages/UnauthorizedPage'));
const WikiPage = lazyRoute(() => import('./features/knowledge/pages/WikiPage'));
const WorkspaceSettingsPage = lazyRoute(
  () => import('./features/settings/workspace/pages/WorkspaceSettingsPage')
);

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
