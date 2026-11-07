import type { ReactNode } from 'react';
import ProtectedRoute from '../../../shared/components/ProtectedRoute';
import type { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { useAuth } from '../../auth/context';
import GitHubCallbackOrganization from './GitHubCallbackOrganization';

interface GitHubCallbackRouteProps {
  children: ReactNode;
  useAuthHook?: typeof useAuth;
  useWorkspaceHook?: typeof useWorkspace;
}

export default function GitHubCallbackRoute({
  children,
  useAuthHook,
  useWorkspaceHook,
}: GitHubCallbackRouteProps) {
  return (
    <ProtectedRoute useAuthHook={useAuthHook}>
      <GitHubCallbackOrganization useWorkspaceHook={useWorkspaceHook}>
        {children}
      </GitHubCallbackOrganization>
    </ProtectedRoute>
  );
}
