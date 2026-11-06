import { Button } from '@zone/ui';
import { type ReactNode, useId } from 'react';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { canManageOrganization } from '../../../shared/roles';
import { BranchSelect, type GitHubRepository, RepositoryPicker } from '../../github';
import { GitHubConnectionMode } from './GitHubConnectionMode';

interface GitHubConfigurationProps {
  mode: GitHubConnectionMode;
  onModeChange: (mode: GitHubConnectionMode) => void;
  repository: GitHubRepository | null;
  onRepositoryChange: (repository: GitHubRepository | null) => void;
  branch: string;
  onBranchChange: (branch: string) => void;
  children: ReactNode;
}

export function GitHubConfiguration({
  mode,
  onModeChange,
  repository,
  onRepositoryChange,
  branch,
  onBranchChange,
  children,
}: GitHubConfigurationProps) {
  const tokenFieldsId = useId();
  const { currentOrganization } = useWorkspace();
  const organizationId = currentOrganization?.id ?? null;
  const canManage = canManageOrganization(currentOrganization?.role);
  const tokenMode = mode === GitHubConnectionMode.Token;

  return (
    <div className="github-configuration">
      {!tokenMode && (
        <>
          <RepositoryPicker
            organizationId={organizationId}
            canManage={canManage}
            value={repository}
            onChange={onRepositoryChange}
          />
          <BranchSelect
            organizationId={organizationId}
            repository={repository}
            value={branch}
            onChange={onBranchChange}
          />
        </>
      )}
      <Button
        variant="link"
        size="sm"
        aria-expanded={tokenMode}
        aria-controls={tokenFieldsId}
        onClick={() =>
          onModeChange(tokenMode ? GitHubConnectionMode.App : GitHubConnectionMode.Token)
        }
      >
        Use an access token instead
      </Button>
      <div id={tokenFieldsId} hidden={!tokenMode}>
        {tokenMode && children}
      </div>
    </div>
  );
}
