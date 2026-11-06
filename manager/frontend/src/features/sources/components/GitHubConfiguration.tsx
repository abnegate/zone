import { Button } from '@zone/ui';
import { type KeyboardEvent, type ReactNode, useId } from 'react';
import { meetsOrganizationRole } from '../../../shared/components/ProtectedRoute';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
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

function keepHandledEscapeInside(event: KeyboardEvent<HTMLDivElement>) {
  if (event.key === 'Escape' && event.nativeEvent.defaultPrevented) {
    event.stopPropagation();
  }
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
  const role = currentOrganization?.role;
  const canManage = role !== undefined && meetsOrganizationRole(role, 'admin');
  const tokenMode = mode === GitHubConnectionMode.Token;

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: stops an Escape the picker already handled from reaching the dialog's document listener; adds no interaction
    <div className="github-configuration" onKeyDown={keepHandledEscapeInside}>
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
