import { Link } from 'react-router-dom';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { Permission } from '../../enterprise/roles/schemas';
import { grantsOrganization } from '../../settings/navigation/access';
import { GITHUB_SETTINGS_PATH } from '../callback';

interface ConnectGitHubPromptProps {
  className?: string;
}

export function ConnectGitHubPrompt({ className }: ConnectGitHubPromptProps) {
  const { currentOrganization, loading, resolvingRole } = useWorkspace();

  if (loading || resolvingRole) {
    return null;
  }

  const connects =
    currentOrganization !== null &&
    grantsOrganization(
      currentOrganization?.permissions,
      currentOrganization?.role,
      Permission.GitHubUpdate
    );

  return connects ? (
    <Link className={className} to={GITHUB_SETTINGS_PATH}>
      Connect GitHub
    </Link>
  ) : (
    <span>Ask an organization admin to connect GitHub.</span>
  );
}
