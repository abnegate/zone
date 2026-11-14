import { Badge } from '@zone/ui';
import type { GitHubInstallation } from '../../../github';

interface InstallationStateProps {
  installations: GitHubInstallation[];
}

export function InstallationState({ installations }: InstallationStateProps) {
  const [latest] = installations;
  if (latest === undefined) {
    return <span className="github-account-pending">Installation not finished</span>;
  }
  if (latest.revoked_at !== null) {
    return <Badge variant="destructive">Revoked</Badge>;
  }
  if (latest.suspended_at !== null) {
    return <Badge variant="warning">Suspended</Badge>;
  }
  return null;
}
