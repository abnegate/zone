import { useLayoutEffect } from 'react';
import { useAuth } from '../../features/auth';
import { useTheme } from './ThemeContext';
import { useWorkspace } from './WorkspaceContext';

export function WorkspaceTheme({
  useAuthHook = useAuth,
  useWorkspaceHook = useWorkspace,
}: {
  useAuthHook?: typeof useAuth;
  useWorkspaceHook?: typeof useWorkspace;
}) {
  const { isAuthenticated, isLoading } = useAuthHook();
  const { currentOrganization, currentWorkspace } = useWorkspaceHook();
  const { loadWorkspaceTheme } = useTheme();
  const organization = isAuthenticated && !isLoading ? (currentOrganization?.id ?? null) : null;
  const workspace = isAuthenticated && !isLoading ? (currentWorkspace?.id ?? null) : null;

  useLayoutEffect(
    () => loadWorkspaceTheme(organization, workspace),
    [loadWorkspaceTheme, organization, workspace]
  );
  return null;
}
