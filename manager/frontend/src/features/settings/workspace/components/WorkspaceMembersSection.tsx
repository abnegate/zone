import { Button, Modal, Select } from '@zone/ui';
import { type FormEvent, useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import { useAuth } from '../../../auth';
import type {
  AddWorkspaceMemberRequest,
  OrganizationMember,
  WorkspaceMember,
  WorkspaceRole,
} from '../types';
import '../../organization/components/OrgMembersSection.css'; // Reuse the same CSS

interface WorkspaceMembersSectionProps {
  workspaceId: string;
  orgId: string;
}

const roleOptions: Array<{ value: WorkspaceRole; label: string }> = [
  { value: 'viewer', label: 'Viewer' },
  { value: 'member', label: 'Member' },
  { value: 'admin', label: 'Admin' },
  { value: 'owner', label: 'Owner' },
];

const memberLabel = (member: { display_name: string | null; email: string }) =>
  member.display_name || member.email || 'Member';

const toUserMessage = (err: unknown, fallback: string) => {
  const message = err instanceof Error ? err.message : fallback;
  return message.startsWith('Validation failed') ? fallback : message;
};

export default function WorkspaceMembersSection({
  workspaceId,
  orgId,
}: WorkspaceMembersSectionProps) {
  const { user } = useAuth();

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | undefined>(undefined);
  const [success, setSuccess] = useState<string | undefined>(undefined);
  const [members, setMembers] = useState<WorkspaceMember[]>([]);
  const [orgMembers, setOrgMembers] = useState<OrganizationMember[]>([]);
  const [currentUserRole, setCurrentUserRole] = useState<WorkspaceRole>('viewer');

  // Track which member is being modified to prevent race conditions
  const [updatingMemberId, setUpdatingMemberId] = useState<string | null>(null);

  // Add member modal state
  const [showAddModal, setShowAddModal] = useState(false);
  const [addUserId, setAddUserId] = useState('');
  const [addUserIdError, setAddUserIdError] = useState<string | undefined>(undefined);
  const [addRole, setAddRole] = useState<WorkspaceRole>('member');
  const [adding, setAdding] = useState(false);

  // Remove member modal state
  const [showRemoveModal, setShowRemoveModal] = useState(false);
  const [memberToRemove, setMemberToRemove] = useState<WorkspaceMember | null>(null);
  const [removing, setRemoving] = useState(false);

  // Role elevation confirmation modal state
  const [showRoleConfirmModal, setShowRoleConfirmModal] = useState(false);
  const [pendingRoleChange, setPendingRoleChange] = useState<{
    member: WorkspaceMember;
    newRole: WorkspaceRole;
  } | null>(null);

  const loadMembers = useCallback(async () => {
    setLoading(true);
    setError(undefined);
    try {
      const [workspaceMembersResponse, orgMembersResponse] = await Promise.all([
        client.getWorkspaceMembers(workspaceId),
        client.getOrgMembers(orgId),
      ]);

      setMembers(workspaceMembersResponse.members);
      setOrgMembers(orgMembersResponse.members);

      // Find current user's role
      const currentMember = workspaceMembersResponse.members.find((m) => m.user_id === user?.id);
      if (currentMember) {
        setCurrentUserRole(currentMember.role);
      }
    } catch (err) {
      setError(toUserMessage(err, 'Couldn’t load members'));
    } finally {
      setLoading(false);
    }
  }, [workspaceId, orgId, user?.id]);

  useEffect(() => {
    loadMembers();
  }, [loadMembers]);

  const showSuccessMessage = (message: string) => {
    setSuccess(message);
    setTimeout(() => setSuccess(undefined), 3000);
  };

  // Get org members who are NOT already in the workspace
  const getAvailableOrgMembers = (): OrganizationMember[] => {
    const workspaceMemberUserIds = new Set(members.map((m) => m.user_id));
    return orgMembers.filter((orgMember) => !workspaceMemberUserIds.has(orgMember.user_id));
  };

  const handleAddMember = async (e: FormEvent) => {
    e.preventDefault();
    setAddUserIdError(undefined);

    // Validate user selection
    if (!addUserId.trim()) {
      setAddUserIdError('User is required');
      return;
    }

    setAdding(true);
    setError(undefined);

    try {
      const request: AddWorkspaceMemberRequest = {
        user_id: addUserId,
        role: addRole,
      };
      await client.addWorkspaceMember(workspaceId, request);
      showSuccessMessage('Member added successfully');
      setShowAddModal(false);
      setAddUserId('');
      setAddRole('member');
      setAddUserIdError(undefined);
      await loadMembers();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to add member');
    } finally {
      setAdding(false);
    }
  };

  const handleRoleChangeRequest = (member: WorkspaceMember, newRole: WorkspaceRole) => {
    // Prevent demoting last owner
    if (member.role === 'owner' && countOwners() === 1 && newRole !== 'owner') {
      setError('Cannot change role of the last owner');
      return;
    }

    // Check if this is an elevation to admin or owner
    if ((newRole === 'admin' || newRole === 'owner') && member.role === 'member') {
      setPendingRoleChange({ member, newRole });
      setShowRoleConfirmModal(true);
    } else if ((newRole === 'admin' || newRole === 'owner') && member.role === 'viewer') {
      setPendingRoleChange({ member, newRole });
      setShowRoleConfirmModal(true);
    } else if (newRole === 'owner' && member.role === 'admin') {
      setPendingRoleChange({ member, newRole });
      setShowRoleConfirmModal(true);
    } else {
      // No confirmation needed for demotions
      handleRoleChange(member.user_id, newRole);
    }
  };

  const handleRoleChange = async (userId: string, newRole: WorkspaceRole) => {
    // Find the member
    const member = members.find((m) => m.user_id === userId);
    if (!member) return;

    setUpdatingMemberId(member.id);
    setError(undefined);

    try {
      await client.updateWorkspaceMemberRole(workspaceId, userId, { role: newRole });
      showSuccessMessage('Role updated successfully');
      await loadMembers();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to update role');
    } finally {
      setUpdatingMemberId(null);
    }
  };

  const confirmRoleChange = async () => {
    if (!pendingRoleChange) return;

    // Clear pending state BEFORE async call to prevent race conditions
    const { member, newRole } = pendingRoleChange;
    setPendingRoleChange(null);
    setShowRoleConfirmModal(false);
    await handleRoleChange(member.user_id, newRole);
  };

  const handleRemoveMember = async () => {
    if (!memberToRemove) return;

    setRemoving(true);
    setError(undefined);

    try {
      await client.removeWorkspaceMember(workspaceId, memberToRemove.user_id);
      showSuccessMessage('Member removed successfully');
      setShowRemoveModal(false);
      setMemberToRemove(null);
      await loadMembers();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to remove member');
    } finally {
      setRemoving(false);
    }
  };

  const getRoleBadgeClass = (role: WorkspaceRole): string => {
    return `role-badge role-badge-${role}`;
  };

  const formatDate = (dateString: string): string => {
    if (!dateString) return '—';
    const date = new Date(dateString);
    if (Number.isNaN(date.getTime())) return '—';
    return date.toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  };

  const countOwners = (): number => {
    return members.filter((m) => m.role === 'owner').length;
  };

  const canModifyMember = (member: WorkspaceMember): boolean => {
    // Cannot modify yourself
    if (member.user_id === user?.id) return false;
    // Cannot modify last owner
    if (member.role === 'owner' && countOwners() === 1) return false;
    return true;
  };

  const getAvailableRoles = (
    member: WorkspaceMember
  ): Array<{ value: WorkspaceRole; label: string }> => {
    // Viewers and members can't change roles (already prevented by canModifyMember, but extra safety)
    if (currentUserRole === 'viewer' || currentUserRole === 'member') {
      return roleOptions.filter((r) => r.value === member.role);
    }

    // Admins can only assign viewer/member/admin roles (not owner)
    if (currentUserRole === 'admin') {
      return roleOptions.filter((r) => r.value !== 'owner');
    }

    // Owners can assign any role
    return roleOptions;
  };

  if (loading) {
    return <div className="loading-state">Loading members...</div>;
  }

  const availableOrgMembers = getAvailableOrgMembers();

  return (
    <div className="org-members-section">
      <div className="section-header">
        <div>
          <h2 className="section-title">Workspace Members</h2>
          <p className="section-description">Manage members and their roles in this workspace.</p>
        </div>
        {(currentUserRole === 'admin' || currentUserRole === 'owner') && (
          <Button onClick={() => setShowAddModal(true)} variant="primary">
            Add Member
          </Button>
        )}
      </div>

      {error && (
        <div className="alert alert-error" role="alert">
          {error}
        </div>
      )}
      {success && (
        <div className="alert alert-success" role="alert">
          {success}
        </div>
      )}

      {members.length === 0 ? (
        <div className="empty-state">
          <p>No members found</p>
        </div>
      ) : (
        <div className="members-table-container">
          <table className="members-table">
            <thead>
              <tr>
                <th>Member</th>
                <th>Email</th>
                <th>Role</th>
                <th>Joined</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {members.map((member) => {
                const isUpdating = updatingMemberId === member.id;
                const canModify = canModifyMember(member);
                const availableRoles = getAvailableRoles(member);

                return (
                  <tr key={member.id}>
                    <td>
                      <div className="member-info">
                        <div className="member-avatar">{memberLabel(member)[0].toUpperCase()}</div>
                        <div className="member-name">{memberLabel(member)}</div>
                      </div>
                    </td>
                    <td>{member.email || '—'}</td>
                    <td>
                      <span className={getRoleBadgeClass(member.role)}>
                        {member.role.charAt(0).toUpperCase() + member.role.slice(1)}
                      </span>
                    </td>
                    <td>{formatDate(member.joined_at)}</td>
                    <td>
                      <div className="member-actions">
                        <select
                          value={member.role}
                          onChange={(e) =>
                            handleRoleChangeRequest(member, e.target.value as WorkspaceRole)
                          }
                          disabled={!canModify || isUpdating}
                          className="role-select"
                          aria-label={`Change role for ${memberLabel(member)}`}
                        >
                          {availableRoles.map((option) => (
                            <option key={option.value} value={option.value}>
                              {option.label}
                            </option>
                          ))}
                        </select>
                        {isUpdating && (
                          <span className="loading-spinner" aria-live="polite">
                            Updating...
                          </span>
                        )}
                        <Button
                          onClick={() => {
                            setMemberToRemove(member);
                            setShowRemoveModal(true);
                          }}
                          disabled={!canModify || isUpdating}
                          variant="secondary"
                          size="sm"
                        >
                          Remove
                        </Button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {/* Add Member Modal */}
      <Modal
        isOpen={showAddModal}
        onClose={() => {
          setShowAddModal(false);
          setAddUserIdError(undefined);
        }}
        title="Add Workspace Member"
      >
        <form onSubmit={handleAddMember} className="add-member-form">
          <Select
            label="User"
            value={addUserId}
            onChange={(e) => {
              setAddUserId(e.target.value);
              setAddUserIdError(undefined);
            }}
            options={[
              { value: '', label: 'Select a user...' },
              ...availableOrgMembers.map((orgMember) => ({
                value: orgMember.user_id,
                label: `${memberLabel(orgMember)}${orgMember.email ? ` (${orgMember.email})` : ''}`,
              })),
            ]}
            error={addUserIdError}
          />
          <Select
            label="Role"
            value={addRole}
            onChange={(e) => setAddRole(e.target.value as WorkspaceRole)}
            options={
              currentUserRole === 'admin'
                ? roleOptions.filter((r) => r.value !== 'owner')
                : roleOptions
            }
          />
          <div className="modal-actions">
            <Button
              type="button"
              onClick={() => {
                setShowAddModal(false);
                setAddUserIdError(undefined);
              }}
              variant="secondary"
            >
              Cancel
            </Button>
            <Button type="submit" loading={adding} variant="primary">
              Add
            </Button>
          </div>
        </form>
      </Modal>

      {/* Remove Member Confirmation Modal */}
      <Modal
        isOpen={showRemoveModal}
        onClose={() => setShowRemoveModal(false)}
        title="Remove Member"
      >
        <div className="remove-member-modal">
          <p>
            Are you sure you want to remove{' '}
            <strong>{memberToRemove ? memberLabel(memberToRemove) : ''}</strong> from this
            workspace?
          </p>
          <div className="modal-actions">
            <Button type="button" onClick={() => setShowRemoveModal(false)} variant="secondary">
              Cancel
            </Button>
            <Button onClick={handleRemoveMember} loading={removing} variant="danger">
              Confirm
            </Button>
          </div>
        </div>
      </Modal>

      {/* Role Elevation Confirmation Modal */}
      <Modal
        isOpen={showRoleConfirmModal}
        onClose={() => {
          setShowRoleConfirmModal(false);
          setPendingRoleChange(null);
        }}
        title="Confirm Role Change"
      >
        <div className="role-confirm-modal">
          <p>
            Are you sure you want to promote{' '}
            <strong>{pendingRoleChange ? memberLabel(pendingRoleChange.member) : ''}</strong> to{' '}
            <strong>{pendingRoleChange?.newRole}</strong>?
          </p>
          {pendingRoleChange?.newRole === 'owner' && (
            <div className="alert alert-warning">
              Warning: Owners have full control over the workspace, including the ability to remove
              other owners.
            </div>
          )}
          <div className="modal-actions">
            <Button
              type="button"
              onClick={() => {
                setShowRoleConfirmModal(false);
                setPendingRoleChange(null);
              }}
              variant="secondary"
            >
              Cancel
            </Button>
            <Button onClick={confirmRoleChange} variant="primary">
              Confirm
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}
