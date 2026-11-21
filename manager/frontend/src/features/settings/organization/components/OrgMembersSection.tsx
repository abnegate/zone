import { Button, EmptyState, Input, Modal, Select, type SelectOption } from '@zone/ui';
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
  ORGANIZATION_LADDER,
  reaches,
  roleBadgeClass,
  roleLabel,
  type Standing,
} from '../../assignment';
import { memberLabel } from '../../memberLabel';
import type { AddOrgMemberRequest, OrganizationMember, OrgRole } from '../types';
import './OrgMembersSection.css';

interface OrgMembersSectionProps {
  orgId: string;
}

const UNSEATED: Standing<OrgRole> = { role: 'member', permissions: undefined };

const isValidEmail = (email: string): boolean => {
  const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
  return emailRegex.test(email);
};

const toUserMessage = (error: unknown, fallback: string) => {
  const message = error instanceof Error ? error.message : fallback;
  return message.startsWith('Validation failed') ? fallback : message;
};

export default function OrgMembersSection({ orgId }: OrgMembersSectionProps) {
  const { user } = useAuth();

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | undefined>(undefined);
  const [success, setSuccess] = useState<string | undefined>(undefined);
  const [members, setMembers] = useState<OrganizationMember[]>([]);
  const [standing, setStanding] = useState<Standing<OrgRole>>(UNSEATED);

  const [updatingMemberId, setUpdatingMemberId] = useState<string | null>(null);

  const [showAddModal, setShowAddModal] = useState(false);
  const [addEmail, setAddEmail] = useState('');
  const [addEmailError, setAddEmailError] = useState<string | undefined>(undefined);
  const [addRole, setAddRole] = useState<OrgRole>('member');
  const [adding, setAdding] = useState(false);

  const [showRemoveModal, setShowRemoveModal] = useState(false);
  const [memberToRemove, setMemberToRemove] = useState<OrganizationMember | null>(null);
  const [removing, setRemoving] = useState(false);

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

      const caller = response.members.find((member) => member.user_id === user?.id);
      if (caller) {
        setStanding({ role: caller.role, permissions: caller.permissions });
      }
    } catch (error) {
      setError(toUserMessage(error, 'Couldn’t load members'));
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

  const handleAddMember = async (event: FormEvent) => {
    event.preventDefault();
    setAddEmailError(undefined);

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
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Failed to add member');
    } finally {
      setAdding(false);
    }
  };

  const handleRoleChangeRequest = (member: OrganizationMember, value: string) => {
    const assignment = assignmentOf<OrgRole>(value);
    if (assignsCustomRole(assignment)) {
      handleRoleChange(member.user_id, assignment);
      return;
    }
    const newRole = assignment.role;
    if ((newRole === 'admin' || newRole === 'owner') && member.role === 'member') {
      setPendingRoleChange({ member, newRole });
      setShowRoleConfirmModal(true);
    } else if (newRole === 'owner' && member.role === 'admin') {
      setPendingRoleChange({ member, newRole });
      setShowRoleConfirmModal(true);
    } else {
      handleRoleChange(member.user_id, assignment);
    }
  };

  const handleRoleChange = async (userId: string, assignment: Assignment<OrgRole>) => {
    const member = members.find((candidate) => candidate.user_id === userId);
    if (!member) return;

    setUpdatingMemberId(member.id);
    setError(undefined);

    try {
      await client.updateOrgMemberRole(orgId, userId, assignment);
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

    setShowRoleConfirmModal(false);
    await handleRoleChange(pendingRoleChange.member.user_id, { role: pendingRoleChange.newRole });
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

  const adds = ORGANIZATION_LADDER.grants(standing, ORGANIZATION_LADDER.create);
  const updates = ORGANIZATION_LADDER.grants(standing, ORGANIZATION_LADDER.update);
  const removes = ORGANIZATION_LADDER.grants(standing, ORGANIZATION_LADDER.remove);
  const manages = updates || removes;
  const grantable = grantableRoles(ORGANIZATION_LADDER, standing);

  const { response: customRoles, refresh: refreshRoles } = useRoles(updates ? orgId : null);

  const canModifyMember = (member: OrganizationMember): boolean => {
    if (member.user_id === user?.id) return false;
    if (member.role === 'owner' && countOwners() === 1) return false;
    return reaches(ORGANIZATION_LADDER, standing, member);
  };

  const getAvailableRoles = (member: OrganizationMember): SelectOption[] =>
    updates && canModifyMember(member)
      ? [
          ...grantable,
          ...customRoleOptions(
            customRoles,
            ORGANIZATION_LADDER.scope,
            member.custom_role,
            standing.permissions
          ),
        ]
      : [];

  if (loading) {
    return <div className="loading-state">Loading members...</div>;
  }

  return (
    <div className="org-members-section">
      <div className="section-row">
        <div className="section-row-copy">
          <h2 className="section-title">Organization Members</h2>
          <p className="section-description">
            Manage members and their roles in this organization.
          </p>
        </div>
        {adds && (
          <div className="section-row-actions">
            <Button size="sm" onClick={() => setShowAddModal(true)}>
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
          setAddEmailError(undefined);
        }}
        title="Add Organization Member"
      >
        <form onSubmit={handleAddMember} className="add-member-form">
          <Input
            label="Email"
            type="email"
            value={addEmail}
            onChange={(event) => {
              setAddEmail(event.target.value);
              setAddEmailError(undefined);
            }}
            placeholder="member@example.com"
            required
            error={addEmailError}
          />
          <Select
            label="Role"
            value={addRole}
            onChange={(event) => setAddRole(event.target.value as OrgRole)}
            options={grantable}
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
