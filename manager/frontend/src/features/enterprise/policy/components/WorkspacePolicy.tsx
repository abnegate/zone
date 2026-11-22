import { UpgradeNotice } from '../../components/UpgradeNotice';
import { RunnerPlacementCard } from '../../fleet/components/RunnerPlacementCard';
import { workspacePlacement } from '../../fleet/placement';
import { Permission } from '../../roles/schemas';
import { useUpdateWorkspacePolicy } from '../hooks';
import { UPGRADE_NOTICE } from '../labels';
import { community } from '../license';
import { PolicyScope } from '../source';
import type { WorkspacePolicyResponse } from '../types';
import { PolicyEditor } from './PolicyEditor';
import { PolicyStatus } from './PolicyStatus';

export const WORKSPACE_READ_ONLY = `Changing this workspace's agent policy needs the ${Permission.WorkspacePoliciesUpdate} permission in this workspace.`;

interface WorkspacePolicyProps {
  organizationId: string;
  workspaceId: string;
  policy: WorkspacePolicyResponse;
}

export function WorkspacePolicy({ organizationId, workspaceId, policy }: WorkspacePolicyProps) {
  const update = useUpdateWorkspacePolicy(organizationId, workspaceId);

  if (community(policy)) {
    return <UpgradeNotice message={UPGRADE_NOTICE} className="policy-notice" />;
  }

  return (
    <>
      <PolicyStatus policy={policy} readOnlyHint={WORKSPACE_READ_ONLY} />
      <PolicyEditor
        key={`${organizationId}/${workspaceId}`}
        scope={PolicyScope.Workspace}
        saved={policy.workspace}
        organization={policy.organization}
        effective={policy.effective}
        editable={policy.editable}
        pending={update.pending}
        refusal={update.refusal}
        error={update.error}
        onSave={(request, onSaved) => update.update(request, onSaved)}
        onReset={update.reset}
      />
      <RunnerPlacementCard scope={workspacePlacement(organizationId, workspaceId)} />
    </>
  );
}
