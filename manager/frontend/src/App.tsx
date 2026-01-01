import { BrowserRouter, Route, Routes } from 'react-router-dom';
import Layout from './components/Layout';
import ProtectedRoute from './components/ProtectedRoute';
import { AuthProvider } from './context/AuthContext';
import { ThemeProvider } from './context/ThemeContext';
import { WorkspaceProvider } from './context/WorkspaceContext';
import ChatsPage from './pages/ChatsPage';
import LoginPage from './pages/LoginPage';
import ModelsPage from './pages/ModelsPage';
import ProjectsPage from './pages/ProjectsPage';
import RegisterPage from './pages/RegisterPage';
import SourcesPage from './pages/SourcesPage';
import TasksPage from './pages/TasksPage';
import UnauthorizedPage from './pages/UnauthorizedPage';
import WikiPage from './pages/WikiPage';
import WorkspaceSettingsPage from './pages/WorkspaceSettingsPage';
import { PERMISSIONS } from './types';
import './App.css';

function App() {
  return (
    <ThemeProvider>
      <AuthProvider>
        <WorkspaceProvider>
          <BrowserRouter>
            <Routes>
              {/* Public routes */}
              <Route path="/login" element={<LoginPage />} />
              <Route path="/register" element={<RegisterPage />} />
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
                    <ProtectedRoute requiredPermission={PERMISSIONS.MODELS.READ}>
                      <ModelsPage />
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
                  path="wiki"
                  element={
                    <ProtectedRoute requiredPermission={PERMISSIONS.WIKI.READ}>
                      <WikiPage />
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
              </Route>
            </Routes>
          </BrowserRouter>
        </WorkspaceProvider>
      </AuthProvider>
    </ThemeProvider>
  );
}

export default App;
