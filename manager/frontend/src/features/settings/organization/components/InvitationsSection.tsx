import { type FormEvent, useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import type { CreateInvitationRequest, Invitation, OrgRole } from '../types';
import type { Workspace, WorkspaceRole } from '../../workspace/types';
import { Button } from '@zone/ui';
import './InvitationsSection.css';

interface InvitationsSectionProps {
  orgId: string;
  workspaces: Workspace[];
}

export function InvitationsSection({ orgId, workspaces }: InvitationsSectionProps) {
  const [invitations, setInvitations] = useState<Invitation[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showModal, setShowModal] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  // Form state
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
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load invitations');
    } finally {
      setLoading(false);
    }
  }, [orgId]);

  useEffect(() => {
    loadInvitations();
  }, [loadInvitations]);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
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
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create invitation');
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
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to revoke invitation');
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
      <div className="section-header">
        <h2>Pending Invitations</h2>
        <Button onClick={() => setShowModal(true)} variant="primary">
          Invite Member
        </Button>
      </div>

      {error && <div className="alert alert-error">{error}</div>}

      {invitations.length === 0 ? (
        <div className="empty-state">
          <p>No pending invitations</p>
          <p className="text-muted">Invite team members to join this organization</p>
        </div>
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
                <th>Actions</th>
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
                  <td>{invitation.workspace_name || '-'}</td>
                  <td>
                    {invitation.workspace_role ? (
                      <span className={`role-badge role-${invitation.workspace_role}`}>
                        {invitation.workspace_role}
                      </span>
                    ) : (
                      '-'
                    )}
                  </td>
                  <td>{invitation.invited_by_email}</td>
                  <td className={isExpired(invitation.expires_at) ? 'text-danger' : ''}>
                    {formatDate(invitation.expires_at)}
                    {isExpired(invitation.expires_at) && ' (Expired)'}
                  </td>
                  <td>
                    <Button onClick={() => handleRevoke(invitation.id)} variant="danger" size="sm">
                      Revoke
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {showModal && (
        <div
          className="modal-overlay"
          onClick={() => setShowModal(false)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              setShowModal(false);
            }
          }}
          role="button"
          tabIndex={0}
          aria-label="Close modal"
        >
          <div
            className="modal-content"
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => e.stopPropagation()}
            role="dialog"
          >
            <div className="modal-header">
              <h3>Invite Member</h3>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowModal(false)}
                aria-label="Close"
              >
                ×
              </button>
            </div>

            <form onSubmit={handleSubmit} className="invitation-form">
              <div className="form-group">
                <label htmlFor="email">Email Address</label>
                <input
                  type="email"
                  id="email"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  required
                  placeholder="member@example.com"
                  className="form-input"
                />
              </div>

              <div className="form-group">
                <label htmlFor="org-role">Organization Role</label>
                <select
                  id="org-role"
                  value={orgRole}
                  onChange={(e) => setOrgRole(e.target.value as OrgRole)}
                  className="form-select"
                >
                  <option value="member">Member</option>
                  <option value="admin">Admin</option>
                  <option value="owner">Owner</option>
                </select>
              </div>

              <div className="form-group">
                <label htmlFor="workspace">Workspace (Optional)</label>
                <select
                  id="workspace"
                  value={workspaceId}
                  onChange={(e) => setWorkspaceId(e.target.value)}
                  className="form-select"
                >
                  <option value="">None - Org only</option>
                  {workspaces.map((ws) => (
                    <option key={ws.id} value={ws.id}>
                      {ws.name}
                    </option>
                  ))}
                </select>
              </div>

              {workspaceId && (
                <div className="form-group">
                  <label htmlFor="workspace-role">Workspace Role</label>
                  <select
                    id="workspace-role"
                    value={workspaceRole}
                    onChange={(e) => setWorkspaceRole(e.target.value as WorkspaceRole)}
                    className="form-select"
                  >
                    <option value="viewer">Viewer</option>
                    <option value="member">Member</option>
                    <option value="admin">Admin</option>
                    <option value="owner">Owner</option>
                  </select>
                </div>
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
          </div>
        </div>
      )}
    </div>
  );
}
