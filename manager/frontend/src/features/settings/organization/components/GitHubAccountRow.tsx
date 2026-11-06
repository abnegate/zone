import { Badge, Button, buttonVariants } from '@zone/ui';
import type { GitHubApp, GitHubAppOwnerType, GitHubInstallation } from '../../../github';

const OWNER_TYPE_LABELS: Readonly<Record<GitHubAppOwnerType, string>> = {
  User: 'Personal account',
  Organization: 'Organization',
};

interface GitHubAccountRowProps {
  app: GitHubApp;
  installations: GitHubInstallation[];
  canManage: boolean;
  installing: boolean;
  onFinishInstalling: () => void;
  onRemove: () => void;
}

function InstallationState({ installations }: { installations: GitHubInstallation[] }) {
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

export function GitHubAccountRow({
  app,
  installations,
  canManage,
  installing,
  onFinishInstalling,
  onRemove,
}: GitHubAccountRowProps) {
  const [latest] = installations;
  const configurable = latest !== undefined && latest.revoked_at === null;

  return (
    <li className="github-account" aria-label={app.owner_login}>
      <div className="github-account-identity">
        <span className="github-account-login">{app.owner_login}</span>
        <span className="github-account-type">{OWNER_TYPE_LABELS[app.owner_type]}</span>
      </div>
      <div className="github-account-state">
        <InstallationState installations={installations} />
      </div>
      {canManage && (
        <div className="github-account-actions">
          {configurable ? (
            <a
              className={buttonVariants({ variant: 'ghost', size: 'sm' })}
              href={latest.html_url}
              target="_blank"
              rel="noopener noreferrer"
            >
              Configure on GitHub
            </a>
          ) : (
            <Button variant="ghost" size="sm" loading={installing} onClick={onFinishInstalling}>
              Finish installing
            </Button>
          )}
          <Button variant="ghost" size="sm" className="github-account-remove" onClick={onRemove}>
            Remove
          </Button>
        </div>
      )}
    </li>
  );
}
