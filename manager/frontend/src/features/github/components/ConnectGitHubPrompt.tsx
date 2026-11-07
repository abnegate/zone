import { Link } from 'react-router-dom';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { administersOrganization } from '../../settings/navigation/sections';
import { GITHUB_SETTINGS_PATH } from '../callback';

interface ConnectGitHubPromptProps {
  className?: string;
}

export function ConnectGitHubPrompt({ className }: ConnectGitHubPromptProps) {
  const { currentOrganization, loading, resolvingRole } = useWorkspace();

  if (loading || resolvingRole) {
    return null;
  }

  const administers = administersOrganization({
    organization: currentOrganization !== null,
    organizationRole: currentOrganization?.role,
  });

  return administers ? (
    <Link className={className} to={GITHUB_SETTINGS_PATH}>
      Connect GitHub
    </Link>
  ) : (
    <span>Ask an organization admin to connect GitHub.</span>
  );
}
