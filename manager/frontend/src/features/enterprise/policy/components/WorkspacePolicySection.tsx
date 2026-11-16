import { Button } from '@zone/ui';
import { useId } from 'react';
import { Link } from 'react-router-dom';
import { useUpdateWorkspacePolicy, useWorkspacePolicy } from '../hooks';
import { community } from '../license';
import { PolicyScope } from '../source';
import type { WorkspacePolicyResponse } from '../types';
import { PolicyEditor } from './PolicyEditor';
import { PolicyStatus } from './PolicyStatus';
import { PolicyUpgradeNotice } from './PolicyUpgradeNotice';
import './PolicySection.css';

export const WORKSPACE_TITLE = 'Agent policy';

export const WORKSPACE_DESCRIPTION = "Narrow the organization's agent policy for this workspace.";

export const WORKSPACE_READ_ONLY =
  "Only workspace admins and organization admins can change this workspace's agent policy.";

export const ORGANIZATION_POLICY_LINK = 'Edit organization policy';

interface WorkspacePolicyProps {
  organizationId: string;
  workspaceId: string;
  policy: WorkspacePolicyResponse;
}

function WorkspacePolicy({ organizationId, workspaceId, policy }: WorkspacePolicyProps) {
  const update = useUpdateWorkspacePolicy(organizationId, workspaceId);

  if (community(policy)) {
    return <PolicyUpgradeNotice />;
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
    </>
  );
}

interface WorkspacePolicySectionProps {
  organizationId: string;
  workspaceId: string;
  organizationPolicyPath?: string;
}

export function WorkspacePolicySection({
  organizationId,
  workspaceId,
  organizationPolicyPath,
}: WorkspacePolicySectionProps) {
  const headingId = useId();
  const { policy, loading, error, refresh } = useWorkspacePolicy(organizationId, workspaceId);

  if (loading) {
    return <div className="loading-state">Loading the agent policy...</div>;
  }

  return (
    <section className="settings-section policy-section" aria-labelledby={headingId}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id={headingId} className="section-title">
            {WORKSPACE_TITLE}
          </h2>
          <p className="section-description">{WORKSPACE_DESCRIPTION}</p>
        </div>
        {organizationPolicyPath && (
          <div className="section-row-actions">
            <Link to={organizationPolicyPath} className="ui-btn ui-btn-ghost ui-btn-sm">
              {ORGANIZATION_POLICY_LINK}
            </Link>
          </div>
        )}
      </div>
      {error && (
        <div className="alert alert-error alert-inline" role="alert">
          <span>{error}</span>
          <Button type="button" variant="ghost" size="sm" onClick={() => void refresh()}>
            Retry
          </Button>
        </div>
      )}
      {policy && (
        <WorkspacePolicy
          organizationId={organizationId}
          workspaceId={workspaceId}
          policy={policy}
        />
      )}
    </section>
  );
}
