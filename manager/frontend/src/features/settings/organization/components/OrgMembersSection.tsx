import { type FormEvent, useCallback, useEffect, useState } from 'react';
import { Button, Input, Modal, Select } from '@zone/ui';
import { client } from '../../../../api/client';
import { useAuth } from '../../../auth';
import type { AddOrgMemberRequest, OrgRole, OrganizationMember } from '../types';
import './OrgMembersSection.css';

interface OrgMembersSectionProps {
  orgId: string;
}

const roleOptions: Array<{ value: OrgRole; label: string }> = [
  { value: 'member', label: 'Member' },
  { value: 'admin', label: 'Admin' },
  { value: 'owner', label: 'Owner' },
];

// Email validation helper
const isValidEmail = (email: string): boolean => {
  const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
  return emailRegex.test(email);
};

export default function OrgMembersSection({ orgId }: OrgMembersSectionProps) {
  const { user } = useAuth();

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | undefined>(undefined);
  const [success, setSuccess] = useState<string | undefined>(undefined);
  const [members, setMembers] = useState<OrganizationMember[]>([]);
  const [currentUserRole, setCurrentUserRole] = useState<OrgRole>('member');

  // Track which member is being modified to prevent race conditions
  const [updatingMemberId, setUpdatingMemberId] = useState<string | null>(null);

  // Add member modal state
  const [showAddModal, setShowAddModal] = useState(false);
  const [addEmail, setAddEmail] = useState('');
  const [addEmailError, setAddEmailError] = useState<string | undefined>(undefined);
  const [addRole, setAddRole] = useState<OrgRole>('member');
  const [adding, setAdding] = useState(false);

  // Remove member modal state
  const [showRemoveModal, setShowRemoveModal] = useState(false);
  const [memberToRemove, setMemberToRemove] = useState<OrganizationMember | null>(null);
  const [removing, setRemoving] = useState(false);

  // Role elevation confirmation modal state
  const [showRoleConfirmModal, setShowRoleConfirmModal] = useState(false);
  const [pendingRoleChange, setPendingRoleChange] = useState<{
    member: OrganizationMember;
    newRole: OrgRole;
  } | null>(null);

  const loadMembers = useCallback(async () => {
    setLoading(true);
    setError(undefined);
    try {
      const response = await client.getOrgMembers(orgId);
      setMembers(response.members);

      // Find current user's role
      const currentMember = response.members.find((m) => m.user_id === user?.id);
      if (currentMember) {
        setCurrentUserRole(currentMember.role);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load members');
    } finally {
      setLoading(false);
    }
  }, [orgId, user?.id]);

  useEffect(() => {
    loadMembers();
  }, [loadMembers]);

  const showSuccessMessage = (message: string) => {
    setSuccess(message);
    setTimeout(() => setSuccess(undefined), 3000);
  };

  const handleAddMember = async (e: FormEvent) => {
    e.preventDefault();
    setAddEmailError(undefined);

    // Validate email
    if (!addEmail.trim()) {
      setAddEmailError('Email is required');
      return;
    }

    if (!isValidEmail(addEmail)) {
      setAddEmailError('Please enter a valid email address');
      return;
    }

    setAdding(true);
    setError(undefined);

    try {
      const request: AddOrgMemberRequest = {
        email: addEmail,
        role: addRole,
      };
      await client.addOrgMember(orgId, request);
      showSuccessMessage('Member added successfully');
      setShowAddModal(false);
      setAddEmail('');
      setAddRole('member');
      setAddEmailError(undefined);
      await loadMembers();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to add member');
    } finally {
      setAdding(false);
    }
  };

  const handleRoleChangeRequest = (member: OrganizationMember, newRole: OrgRole) => {
    // Check if this is an elevation to admin or owner
    if ((newRole === 'admin' || newRole === 'owner') && member.role === 'member') {
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

  const handleRoleChange = async (userId: string, newRole: OrgRole) => {
    // Find the member
    const member = members.find((m) => m.user_id === userId);
    if (!member) return;

    setUpdatingMemberId(member.id);
    setError(undefined);

    try {
      await client.updateOrgMemberRole(orgId, userId, { role: newRole });
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

    setShowRoleConfirmModal(false);
    await handleRoleChange(pendingRoleChange.member.user_id, pendingRoleChange.newRole);
    setPendingRoleChange(null);
  };

  const handleRemoveMember = async () => {
    if (!memberToRemove) return;

    setRemoving(true);
    setError(undefined);

    try {
      await client.removeOrgMember(orgId, memberToRemove.user_id);
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

  const getRoleBadgeClass = (role: OrgRole): string => {
    return `role-badge role-badge-${role}`;
  };

  const formatDate = (dateString: string): string => {
    const date = new Date(dateString);
    // Use user's locale instead of hardcoded 'en-US'
    return date.toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  };

  const countOwners = (): number => {
    return members.filter((m) => m.role === 'owner').length;
  };

  const canModifyMember = (member: OrganizationMember): boolean => {
    // Cannot modify yourself
    if (member.user_id === user?.id) return false;
    // Cannot modify last owner
    if (member.role === 'owner' && countOwners() === 1) return false;
    return true;
  };

  const getAvailableRoles = (
    member: OrganizationMember
  ): Array<{ value: OrgRole; label: string }> => {
    // Members and viewers can't change roles (already prevented by canModifyMember, but extra safety)
    if (currentUserRole === 'member') {
      return roleOptions.filter((r) => r.value === member.role);
    }

    // Admins can only assign member/admin roles (not owner)
    if (currentUserRole === 'admin') {
      return roleOptions.filter((r) => r.value !== 'owner');
    }

    // Owners can assign any role
    return roleOptions;
  };

  if (loading) {
    return <div className="loading-state">Loading members...</div>;
  }

  return (
    <div className="org-members-section">
      <div className="section-header">
        <div>
          <h2 className="section-title">Organization Members</h2>
          <p className="section-description">
            Manage members and their roles in this organization.
          </p>
        </div>
        <Button onClick={() => setShowAddModal(true)} variant="primary">
          Add Member
        </Button>
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
                        <div className="member-avatar">
                          {(member.display_name || member.email)[0].toUpperCase()}
                        </div>
                        <div className="member-name">{member.display_name || member.email}</div>
                      </div>
                    </td>
                    <td>{member.email}</td>
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
                            handleRoleChangeRequest(member, e.target.value as OrgRole)
                          }
                          disabled={!canModify || isUpdating}
                          className="role-select"
                          aria-label={`Change role for ${member.display_name || member.email}`}
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
          setAddEmailError(undefined);
        }}
        title="Add Organization Member"
      >
        <form onSubmit={handleAddMember} className="add-member-form">
          <Input
            label="Email"
            type="email"
            value={addEmail}
            onChange={(e) => {
              setAddEmail(e.target.value);
              setAddEmailError(undefined);
            }}
            placeholder="member@example.com"
            required
            error={addEmailError}
          />
          <Select
            label="Role"
            value={addRole}
            onChange={(e) => setAddRole(e.target.value as OrgRole)}
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
                setAddEmailError(undefined);
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
            <strong>{memberToRemove?.display_name || memberToRemove?.email}</strong> from this
            organization?
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
            <strong>
              {pendingRoleChange?.member.display_name || pendingRoleChange?.member.email}
            </strong>{' '}
            to <strong>{pendingRoleChange?.newRole}</strong>?
          </p>
          {pendingRoleChange?.newRole === 'owner' && (
            <div className="alert alert-warning">
              Warning: Owners have full control over the organization, including the ability to
              remove other owners.
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
