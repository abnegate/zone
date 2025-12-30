import { BrowserRouter, Route, Routes } from 'react-router-dom';
import Layout from './components/Layout';
import LoginOverlay from './components/LoginOverlay';
import { AuthProvider } from './context/AuthContext';
import ChatsPage from './pages/ChatsPage';
import ModelsPage from './pages/ModelsPage';
import ProjectsPage from './pages/ProjectsPage';
import TasksPage from './pages/TasksPage';
import WikiPage from './pages/WikiPage';
import './App.css';

function App() {
  return (
    <AuthProvider>
      <BrowserRouter>
        <LoginOverlay />
        <Routes>
          <Route path="/" element={<Layout />}>
            <Route index element={<ModelsPage />} />
            <Route path="chats" element={<ChatsPage />} />
            <Route path="projects" element={<ProjectsPage />} />
            <Route path="tasks" element={<TasksPage />} />
            <Route path="wiki" element={<WikiPage />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </AuthProvider>
  );
}

export default App;
