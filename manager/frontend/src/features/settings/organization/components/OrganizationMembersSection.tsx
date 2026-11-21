import { useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import { useWorkspace } from '../../../../shared/context/WorkspaceContext';
import type { Workspace } from '../../workspace/types';
import type { InvitationAccess } from '../types';
import { InvitationsSection } from './InvitationsSection';
import OrgMembersSection from './OrgMembersSection';

interface OrganizationMembersSectionProps {
  invitations: InvitationAccess;
}

export function OrganizationMembersSection({ invitations }: OrganizationMembersSectionProps) {
  const { currentOrganization } = useWorkspace();
  const orgId = currentOrganization?.id ?? null;
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!invitations.read || !orgId) return;
    let cancelled = false;
    setError(null);
    client
      .getWorkspaces(orgId)
      .then((loaded) => {
        if (!cancelled) setWorkspaces(loaded);
      })
      .catch(() => {
        if (!cancelled) setError('Couldn’t load workspaces');
      });
    return () => {
      cancelled = true;
    };
  }, [invitations.read, orgId]);

  if (!orgId) return null;

  return (
    <>
      <OrgMembersSection orgId={orgId} />
      {invitations.read && (
        <>
          {error && <div className="alert alert-error">{error}</div>}
          <InvitationsSection orgId={orgId} workspaces={workspaces} access={invitations} />
        </>
      )}
    </>
  );
}
