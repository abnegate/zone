import { Button, EmptyState, Modal, Select } from '@zone/ui';
import { type FormEvent, useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import { Permission } from '../../../enterprise';
import { ORGANIZATION_ROLES, type RoleOption, WORKSPACE_ROLES } from '../../assignment';
import { allowsOrganization, grantsOrganization } from '../../navigation/access';
import type { Workspace, WorkspaceRole } from '../../workspace/types';
import type { CreateInvitationRequest, Invitation, InvitationAccess, OrgRole } from '../types';
import './InvitationsSection.css';

interface InvitationsSectionProps {
  orgId: string;
  workspaces: Workspace[];
  access: Omit<InvitationAccess, 'read'>;
}

const PRIVILEGED_WORKSPACE_ROLES: readonly WorkspaceRole[] = ['admin', 'owner'];

function organizationRoles(access: Omit<InvitationAccess, 'read'>): RoleOption<OrgRole>[] {
  const administers = grantsOrganization(
    access.permissions,
    access.role,
    Permission.AdministratorsUpdate
  );
  const offered: Readonly<Record<OrgRole, boolean>> = {
    member: true,
    admin: allowsOrganization(access.role, 'admin'),
    owner: administers,
  };
  return ORGANIZATION_ROLES.filter(({ value }) => offered[value]);
}

function workspaceRoles(access: Omit<InvitationAccess, 'read'>): RoleOption<WorkspaceRole>[] {
  const administers = grantsOrganization(
    access.permissions,
    access.role,
    Permission.AdministratorsUpdate
  );
  return WORKSPACE_ROLES.filter(
    ({ value }) => administers || !PRIVILEGED_WORKSPACE_ROLES.includes(value)
  );
}

export function InvitationsSection({ orgId, workspaces, access }: InvitationsSectionProps) {
  const [invitations, setInvitations] = useState<Invitation[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showModal, setShowModal] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  const [email, setEmail] = useState('');
  const [orgRole, setOrgRole] = useState<OrgRole>('member');
  const [workspaceId, setWorkspaceId] = useState('');
  const [workspaceRole, setWorkspaceRole] = useState<WorkspaceRole>('member');

  const loadInvitations = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await client.getInvitations(orgId);
      setInvitations(response.invitations);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to load invitations';
      setError(message.startsWith('Validation failed') ? 'Couldn’t load invitations' : message);
    } finally {
      setLoading(false);
    }
  }, [orgId]);

  useEffect(() => {
    loadInvitations();
  }, [loadInvitations]);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);

    try {
      const request: CreateInvitationRequest = {
        email,
        org_role: orgRole,
      };

      if (workspaceId) {
        request.workspace_id = workspaceId;
        request.workspace_role = workspaceRole;
      }

      await client.createInvitation(orgId, request);
      setShowModal(false);
      setEmail('');
      setOrgRole('member');
      setWorkspaceId('');
      setWorkspaceRole('member');
      loadInvitations();
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Failed to create invitation');
    } finally {
      setSubmitting(false);
    }
  };

  const handleRevoke = async (invitationId: string) => {
    if (!window.confirm('Are you sure you want to revoke this invitation?')) {
      return;
    }

    try {
      await client.revokeInvitation(orgId, invitationId);
      loadInvitations();
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Failed to revoke invitation');
    }
  };

  const formatDate = (dateString: string) => {
    return new Date(dateString).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  };

  const isExpired = (expiresAt: string) => {
    return new Date(expiresAt) < new Date();
  };

  if (loading) {
    return <div className="loading-state">Loading invitations...</div>;
  }

  return (
    <div className="invitations-section">
      <div className="section-row">
        <div className="section-row-copy">
          <h2 className="section-title">Pending Invitations</h2>
          <p className="section-description">Invitations expire seven days after they are sent.</p>
        </div>
        {access.create && (
          <div className="section-row-actions">
            <Button size="sm" onClick={() => setShowModal(true)}>
              Invite Member
            </Button>
          </div>
        )}
      </div>
      {error && (
        <div className="alert alert-error" role="alert">
          {error}
        </div>
      )}
      {invitations.length === 0 ? (
        <EmptyState
          icon={
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M3 8l9 6 9-6M5 5h14a2 2 0 012 2v10a2 2 0 01-2 2H5a2 2 0 01-2-2V7a2 2 0 012-2z" />
            </svg>
          }
          title="No pending invitations"
          description="Invite team members to join this organization"
          action={
            access.create ? (
              <Button variant="secondary" onClick={() => setShowModal(true)}>
                Send an invitation
              </Button>
            ) : undefined
          }
        />
      ) : (
        <div className="table-container">
          <table className="invitations-table">
            <thead>
              <tr>
                <th>Email</th>
                <th>Org Role</th>
                <th>Workspace</th>
                <th>WS Role</th>
                <th>Invited By</th>
                <th>Expires</th>
                {access.revoke && <th>Actions</th>}
              </tr>
            </thead>
            <tbody>
              {invitations.map((invitation) => (
                <tr
                  key={invitation.id}
                  className={isExpired(invitation.expires_at) ? 'expired' : ''}
                >
                  <td>{invitation.email}</td>
                  <td>
                    <span className={`role-badge role-${invitation.org_role}`}>
                      {invitation.org_role}
                    </span>
                  </td>
                  <td>
                    {invitation.workspace_name ||
                      workspaces.find((workspace) => workspace.id === invitation.workspace_id)
                        ?.name ||
                      '-'}
                  </td>
                  <td>
                    {invitation.workspace_role ? (
                      <span className={`role-badge role-${invitation.workspace_role}`}>
                        {invitation.workspace_role}
                      </span>
                    ) : (
                      '-'
                    )}
                  </td>
                  <td>{invitation.invited_by_email || '—'}</td>
                  <td className={isExpired(invitation.expires_at) ? 'text-danger' : ''}>
                    {formatDate(invitation.expires_at)}
                    {isExpired(invitation.expires_at) && ' (Expired)'}
                  </td>
                  {access.revoke && (
                    <td className="invitation-actions">
                      <Button
                        className="invitation-revoke"
                        onClick={() => handleRevoke(invitation.id)}
                        variant="ghost"
                        size="sm"
                      >
                        Revoke
                      </Button>
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <Modal isOpen={showModal} onClose={() => setShowModal(false)} title="Invite Member">
        <form onSubmit={handleSubmit} className="invitation-form">
          <div className="form-group">
            <label htmlFor="email">Email Address</label>
            <input
              type="email"
              id="email"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              required
              placeholder="member@example.com"
              className="form-input"
            />
          </div>

          <Select
            id="org-role"
            label="Organization Role"
            value={orgRole}
            onValueChange={(next) => setOrgRole(next as OrgRole)}
            wrapperClassName="form-group"
            options={organizationRoles(access)}
          />

          <div className="form-group">
            <label htmlFor="workspace">
              Workspace <span className="label-optional">optional</span>
            </label>
            <Select
              compact
              id="workspace"
              value={workspaceId}
              onValueChange={setWorkspaceId}
              options={[
                { value: '', label: 'None - Org only' },
                ...workspaces.map((workspace) => ({
                  value: workspace.id,
                  label: workspace.name,
                })),
              ]}
            />
          </div>

          {workspaceId && (
            <Select
              id="workspace-role"
              label="Workspace Role"
              value={workspaceRole}
              onValueChange={(next) => setWorkspaceRole(next as WorkspaceRole)}
              wrapperClassName="form-group"
              options={workspaceRoles(access)}
            />
          )}

          <div className="modal-actions">
            <Button
              type="button"
              onClick={() => setShowModal(false)}
              variant="secondary"
              disabled={submitting}
            >
              Cancel
            </Button>
            <Button type="submit" variant="primary" loading={submitting}>
              Send Invitation
            </Button>
          </div>
        </form>
      </Modal>
    </div>
  );
}
