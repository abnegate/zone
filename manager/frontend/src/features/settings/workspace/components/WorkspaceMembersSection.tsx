import { Button, EmptyState, Modal, Select, type SelectOption } from '@zone/ui';
import { type FormEvent, useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import { useAuth } from '../../../auth';
import { useRoles } from '../../../enterprise';
import {
  type Assignment,
  assignmentFailure,
  assignmentOf,
  assignsCustomRole,
  customRoleOptions,
  grantableRoles,
  heldRole,
  reaches,
  roleBadgeClass,
  roleLabel,
  type Standing,
  WORKSPACE_LADDER,
} from '../../assignment';
import { memberLabel } from '../../memberLabel';
import type {
  AddWorkspaceMemberRequest,
  OrganizationMember,
  WorkspaceMember,
  WorkspaceRole,
} from '../types';
import '../../organization/components/OrgMembersSection.css';

interface WorkspaceMembersSectionProps {
  workspaceId: string;
  orgId: string;
}

const UNSEATED: Standing<WorkspaceRole> = { role: 'viewer', permissions: undefined };

const DEFAULT_ROLE: WorkspaceRole = 'member';

const toUserMessage = (error: unknown, fallback: string) => {
  const message = error instanceof Error ? error.message : fallback;
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
  const [standing, setStanding] = useState<Standing<WorkspaceRole>>(UNSEATED);

  const [updatingMemberId, setUpdatingMemberId] = useState<string | null>(null);

  const [showAddModal, setShowAddModal] = useState(false);
  const [addUserId, setAddUserId] = useState('');
  const [addUserIdError, setAddUserIdError] = useState<string | undefined>(undefined);
  const [addRole, setAddRole] = useState<WorkspaceRole>(DEFAULT_ROLE);
  const [adding, setAdding] = useState(false);

  const [showRemoveModal, setShowRemoveModal] = useState(false);
  const [memberToRemove, setMemberToRemove] = useState<WorkspaceMember | null>(null);
  const [removing, setRemoving] = useState(false);

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

      const caller = workspaceMembersResponse.members.find((member) => member.user_id === user?.id);
      if (caller) {
        setStanding({ role: caller.role, permissions: caller.permissions });
      }
    } catch (error) {
      setError(toUserMessage(error, 'Couldn’t load members'));
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

  const getAvailableOrgMembers = (): OrganizationMember[] => {
    const workspaceMemberUserIds = new Set(members.map((member) => member.user_id));
    return orgMembers.filter((orgMember) => !workspaceMemberUserIds.has(orgMember.user_id));
  };

  const handleAddMember = async (event: FormEvent) => {
    event.preventDefault();
    setAddUserIdError(undefined);

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
      setAddRole(DEFAULT_ROLE);
      setAddUserIdError(undefined);
      await loadMembers();
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Failed to add member');
    } finally {
      setAdding(false);
    }
  };

  const handleRoleChangeRequest = (member: WorkspaceMember, value: string) => {
    const assignment = assignmentOf<WorkspaceRole>(value);
    if (member.role === 'owner' && countOwners() === 1 && heldRole(member) !== value) {
      setError('Cannot change role of the last owner');
      return;
    }
    if (assignsCustomRole(assignment)) {
      handleRoleChange(member.user_id, assignment);
      return;
    }
    const newRole = assignment.role;

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
      handleRoleChange(member.user_id, assignment);
    }
  };

  const handleRoleChange = async (userId: string, assignment: Assignment<WorkspaceRole>) => {
    const member = members.find((candidate) => candidate.user_id === userId);
    if (!member) return;

    setUpdatingMemberId(member.id);
    setError(undefined);

    try {
      await client.updateWorkspaceMemberRole(workspaceId, userId, assignment);
      if (assignsCustomRole(assignment) || member.custom_role !== null) {
        void refreshRoles();
      }
      showSuccessMessage('Role updated successfully');
      await loadMembers();
    } catch (error) {
      setError(assignmentFailure(error, 'Failed to update role'));
    } finally {
      setUpdatingMemberId(null);
    }
  };

  const confirmRoleChange = async () => {
    if (!pendingRoleChange) return;

    const { member, newRole } = pendingRoleChange;
    setPendingRoleChange(null);
    setShowRoleConfirmModal(false);
    await handleRoleChange(member.user_id, { role: newRole });
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
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Failed to remove member');
    } finally {
      setRemoving(false);
    }
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
    return members.filter((member) => member.role === 'owner').length;
  };

  const adds = WORKSPACE_LADDER.grants(standing, WORKSPACE_LADDER.create);
  const updates = WORKSPACE_LADDER.grants(standing, WORKSPACE_LADDER.update);
  const removes = WORKSPACE_LADDER.grants(standing, WORKSPACE_LADDER.remove);
  const manages = updates || removes;
  const grantable = grantableRoles(WORKSPACE_LADDER, standing);

  const { response: customRoles, refresh: refreshRoles } = useRoles(updates ? orgId : null);

  const canModifyMember = (member: WorkspaceMember): boolean => {
    if (member.user_id === user?.id) return false;
    if (member.role === 'owner' && countOwners() === 1) return false;
    return reaches(WORKSPACE_LADDER, standing, member);
  };

  const getAvailableRoles = (member: WorkspaceMember): SelectOption[] =>
    updates && canModifyMember(member)
      ? [
          ...grantable,
          ...customRoleOptions(
            customRoles,
            WORKSPACE_LADDER.scope,
            member.custom_role,
            standing.permissions
          ),
        ]
      : [];

  if (loading) {
    return <div className="loading-state">Loading members...</div>;
  }

  const availableOrgMembers = getAvailableOrgMembers();

  return (
    <div className="org-members-section">
      <div className="section-row">
        <div className="section-row-copy">
          <h2 className="section-title">Workspace Members</h2>
          <p className="section-description">Manage members and their roles in this workspace.</p>
        </div>
        {adds && (
          <div className="section-row-actions">
            <Button
              size="sm"
              onClick={() => {
                setAddRole(
                  grantable.some(({ value }) => value === DEFAULT_ROLE)
                    ? DEFAULT_ROLE
                    : UNSEATED.role
                );
                setShowAddModal(true);
              }}
            >
              Add Member
            </Button>
          </div>
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
        <EmptyState title="No members found" />
      ) : (
        <div className="members-table-container">
          <table className="members-table">
            <thead>
              <tr>
                <th>Member</th>
                <th>Role</th>
                <th>Joined</th>
                {manages && <th className="members-actions-head">Actions</th>}
              </tr>
            </thead>
            <tbody>
              {members.map((member) => {
                const isUpdating = updatingMemberId === member.id;
                const canModify = canModifyMember(member);
                const availableRoles = getAvailableRoles(member);
                const editable = availableRoles.length > 1;
                return (
                  <tr key={member.id}>
                    <td>
                      <div className="member-info">
                        <div className="member-avatar" aria-hidden="true">
                          {memberLabel(member)[0].toUpperCase()}
                        </div>
                        <div className="member-identity">
                          <div className="member-name">{memberLabel(member)}</div>
                          {member.display_name && member.email && (
                            <div className="member-email">{member.email}</div>
                          )}
                        </div>
                      </div>
                    </td>
                    <td>
                      {editable ? (
                        <Select
                          compact
                          value={heldRole(member)}
                          onValueChange={(next) => handleRoleChangeRequest(member, next)}
                          disabled={isUpdating}
                          className="role-select"
                          aria-label={`Change role for ${memberLabel(member)}`}
                          options={availableRoles}
                        />
                      ) : (
                        <span className={roleBadgeClass(member)}>{roleLabel(member)}</span>
                      )}
                    </td>
                    <td className="member-joined">{formatDate(member.joined_at)}</td>
                    {manages && (
                      <td>
                        <div className="member-actions">
                          {isUpdating && (
                            <span className="member-updating" aria-live="polite">
                              Updating...
                            </span>
                          )}
                          {removes && (
                            <Button
                              className="member-remove"
                              onClick={() => {
                                setMemberToRemove(member);
                                setShowRemoveModal(true);
                              }}
                              disabled={!canModify || isUpdating}
                              variant="ghost"
                              size="sm"
                            >
                              Remove
                            </Button>
                          )}
                        </div>
                      </td>
                    )}
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
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
            onChange={(event) => {
              setAddUserId(event.target.value);
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
            onChange={(event) => setAddRole(event.target.value as WorkspaceRole)}
            options={grantable}
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
      <Modal
        isOpen={showRemoveModal}
        onClose={() => setShowRemoveModal(false)}
        title="Remove Member"
        size="sm"
      >
        <div className="confirm-modal">
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
      <Modal
        isOpen={showRoleConfirmModal}
        onClose={() => {
          setShowRoleConfirmModal(false);
          setPendingRoleChange(null);
        }}
        title="Confirm Role Change"
        size="sm"
      >
        <div className="confirm-modal">
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
