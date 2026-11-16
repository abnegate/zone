import { Button, buttonVariants } from '@zone/ui';
import { useId } from 'react';
import { Link } from 'react-router-dom';
import { useWorkspacePolicy } from '../hooks';
import { WorkspacePolicy } from './WorkspacePolicy';
import './PolicySection.css';

export const WORKSPACE_TITLE = 'Agent policy';

export const WORKSPACE_DESCRIPTION = "Narrow the organization's agent policy for this workspace.";

export const ORGANIZATION_POLICY_LINK = 'Edit organization policy';

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
            <Link
              to={organizationPolicyPath}
              className={buttonVariants({ variant: 'ghost', size: 'sm' })}
            >
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
