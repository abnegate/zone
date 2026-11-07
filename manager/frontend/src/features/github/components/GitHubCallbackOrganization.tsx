import { type ReactNode, useEffect } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { GitHubSearchParameter } from '../callback';

interface GitHubCallbackOrganizationProps {
  children: ReactNode;
  useWorkspaceHook?: typeof useWorkspace;
}

export default function GitHubCallbackOrganization({
  children,
  useWorkspaceHook = useWorkspace,
}: GitHubCallbackOrganizationProps) {
  const { organizations, currentOrganization, loading, setCurrentOrganization } =
    useWorkspaceHook();
  const [searchParams, setSearchParams] = useSearchParams();
  const organizationId = searchParams.get(GitHubSearchParameter.Organization);

  useEffect(() => {
    if (organizationId === null || loading) {
      return;
    }
    const next = new URLSearchParams(searchParams);
    next.delete(GitHubSearchParameter.Organization);
    const match = organizations.find((organization) => organization.id === organizationId);
    if (match === undefined) {
      next.delete(GitHubSearchParameter.Outcome);
    } else if (match.id !== currentOrganization?.id) {
      setCurrentOrganization(match);
    }
    setSearchParams(next, { replace: true });
  }, [
    organizationId,
    loading,
    organizations,
    currentOrganization,
    searchParams,
    setSearchParams,
    setCurrentOrganization,
  ]);

  if (organizationId !== null) {
    return (
      <div className="loading-container" role="status">
        Switching organization…
      </div>
    );
  }

  return <>{children}</>;
}
