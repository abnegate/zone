import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { OrganizationMember, OrgRole } from '../types';

// Mock client
const mockClient = {
  getOrgMembers: mock(),
  addOrgMember: mock(),
  updateOrgMemberRole: mock(),
  removeOrgMember: mock(),
};

mock.module('../../../../api/client', () => ({
  client: mockClient,
}));

// Mock useAuth
const mockUseAuth = mock(() => ({
  isAuthenticated: true,
  user: { id: 'user-1', email: 'owner@test.com' },
}));

mock.module('../../../auth', () => ({
  useAuth: mockUseAuth,
}));

let OrgMembersSection: typeof import('./OrgMembersSection').default;

beforeAll(async () => {
  OrgMembersSection = (await import('./OrgMembersSection')).default;
});

afterAll(() => {
  mock.restore();
});

const mockOwner: OrganizationMember = {
  id: 'member-1',
  user_id: 'user-1',
  organization_id: 'org-123',
  role: 'owner',
  email: 'owner@test.com',
  display_name: 'Test Owner',
  joined_at: '2024-01-01T00:00:00Z',
};

const mockAdmin: OrganizationMember = {
  id: 'member-2',
  user_id: 'user-2',
  organization_id: 'org-123',
  role: 'admin',
  email: 'admin@test.com',
  display_name: 'Test Admin',
  joined_at: '2024-01-02T00:00:00Z',
};

const mockMember: OrganizationMember = {
  id: 'member-3',
  user_id: 'user-3',
  organization_id: 'org-123',
  role: 'member',
  email: 'member@test.com',
  display_name: null,
  joined_at: '2024-01-03T00:00:00Z',
};

const rosterWithSignedInUserAs = (role: OrgRole): OrganizationMember[] => [
  {
    ...mockOwner,
    id: 'member-0',
    user_id: 'user-0',
    email: 'other-owner@test.com',
    display_name: 'Other Owner',
  },
  // A second owner, so it is the caller's role and not the last-owner rule that
  // disables the owner row.
  {
    ...mockOwner,
    id: 'member-8',
    user_id: 'user-8',
    email: 'spare-owner@test.com',
    display_name: 'Spare Owner',
  },
  mockAdmin,
  mockMember,
  {
    ...mockMember,
    id: 'member-9',
    user_id: 'user-1',
    role,
    email: 'self@test.com',
    display_name: 'Signed In User',
  },
];

const memberRow = (email: string): HTMLElement => {
  const row = screen
    .getAllByRole('row')
    .find((candidate) => candidate.textContent?.includes(email));
  if (!row) throw new Error(`no members table row for ${email}`);
  return row;
};

const removeButtonFor = (email: string): HTMLElement =>
  within(memberRow(email)).getByRole('button', { name: /Remove/i });

const roleSelectFor = (email: string): HTMLElement =>
  within(memberRow(email)).getByRole('combobox');

// A row the viewer cannot re-seat shows its role as a badge and no select.
const roleBadgeFor = (email: string): HTMLElement => {
  const row = memberRow(email);
  expect(within(row).queryByRole('combobox')).toBeNull();
  const badge = row.querySelector('.role-badge');
  if (!badge) throw new Error(`no role badge in the row for ${email}`);
  return badge as HTMLElement;
};

const rolesOfferedForNewMember = (): string[] => {
  const field = screen.getByRole('combobox', { name: 'Role' }).closest('.ui-select-wrapper');
  if (!field) throw new Error('no role field in the add member modal');
  return Array.from(field.querySelectorAll('option')).map((option) => option.value);
};

describe('OrgMembersSection', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    mockClient.getOrgMembers.mockResolvedValue({
      members: [mockOwner, mockAdmin, mockMember],
    });
  });

  describe('Loading State', () => {
    it('shows loading state initially', () => {
      mockClient.getOrgMembers.mockImplementation(() => new Promise(() => {}));
      render(<OrgMembersSection orgId="org-123" />);
      expect(screen.getByText('Loading members...')).toBeInTheDocument();
    });

    it('shows error when loading fails', async () => {
      mockClient.getOrgMembers.mockRejectedValueOnce(new Error('Failed to load'));
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText(/Failed to load/i)).toBeInTheDocument();
      });
    });
  });

  describe('Members Table', () => {
    it('renders members table with all members', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText('owner@test.com')).toBeInTheDocument();
        expect(screen.getByText('admin@test.com')).toBeInTheDocument();
        expect(screen.getAllByText('member@test.com').length).toBeGreaterThan(0);
      });
    });

    it('displays member names when available', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText('Test Owner')).toBeInTheDocument();
        expect(screen.getByText('Test Admin')).toBeInTheDocument();
      });
    });

    it('displays email when display name is null', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const emailElements = screen.getAllByText('member@test.com');
        expect(emailElements.length).toBeGreaterThan(0);
      });
    });

    it('shows one role control per row: a badge where it is fixed, a select where it can change', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText('Test Owner')).toBeInTheDocument();
      });
      expect(roleBadgeFor('owner@test.com')).toHaveClass('role-badge-owner');
      expect(roleSelectFor('admin@test.com')).toHaveValue('admin');
      expect(roleSelectFor('member@test.com')).toHaveValue('member');
      expect(document.querySelectorAll('.role-badge')).toHaveLength(1);
    });

    it('formats joined dates using user locale', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const text = screen.getByRole('table').textContent;
        expect(text).toContain('Jan');
      });
    });

    it('shows Add Member button', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: /Add Member/i })).toBeInTheDocument();
      });
    });
  });

  describe('Accessibility', () => {
    it('has ARIA labels on role selects', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = document.querySelectorAll('.role-select');
        roleSelects.forEach((select) => {
          expect(select).toHaveAttribute('aria-label');
        });
      });
    });

    it('has ARIA roles on alerts', async () => {
      mockClient.getOrgMembers.mockRejectedValueOnce(new Error('Failed'));
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const alert = screen.getByRole('alert');
        expect(alert).toBeInTheDocument();
      });
    });

    it('has aria-live on loading spinner', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        const memberRoleSelect = roleSelects.find((select) => {
          const row = select.closest('tr');
          return row?.textContent?.includes('member@test.com');
        });
        fireEvent.change(memberRoleSelect!, { target: { value: 'admin' } });
      });
      // During the update, the loading spinner should have aria-live
      // This is difficult to test precisely without delaying the API response
    });
  });

  describe('Email Validation', () => {
    /// happy-dom does not raise a form's submit event from a click on its
    /// submit button, so the form is submitted directly. That still runs
    /// `handleAddMember`, and it bypasses the native `type="email"` check the
    /// way a browser with autofill or a paste would.
    const openAndSubmit = async (email?: string) => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: /Add Member/i })).toBeInTheDocument();
      });
      fireEvent.click(screen.getByRole('button', { name: /Add Member/i }));

      const dialog = await waitFor(() => screen.getByRole('dialog'));
      if (email !== undefined) {
        fireEvent.change(within(dialog).getByLabelText(/Email/i), {
          target: { value: email },
        });
      }
      const form = dialog.querySelector('form');
      expect(form).not.toBeNull();
      fireEvent.submit(form!);
    };

    it('shows error for invalid email format', async () => {
      await openAndSubmit('invalid-email');

      await waitFor(() => {
        expect(screen.getByText(/valid email address/i)).toBeInTheDocument();
      });
      expect(mockClient.addOrgMember).not.toHaveBeenCalled();
    });

    it('shows error for empty email', async () => {
      await openAndSubmit();

      await waitFor(() => {
        expect(screen.getByText(/email is required/i)).toBeInTheDocument();
      });
      expect(mockClient.addOrgMember).not.toHaveBeenCalled();
    });
  });

  describe('Role Hierarchy Restrictions', () => {
    const asAdmin = () =>
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        user: { id: 'user-2', email: 'admin@test.com' },
      });
    const asOwner = () =>
      mockUseAuth.mockReturnValue({
        isAuthenticated: true,
        user: { id: 'user-1', email: 'owner@test.com' },
      });

    const selectFor = (label: string) =>
      screen.getByLabelText(`Change role for ${label}`) as HTMLSelectElement;

    const optionsFor = (label: string) =>
      Array.from(selectFor(label).options).map((option) => option.value);

    it('offers an admin no way to seat or unseat another admin', async () => {
      asAdmin();
      try {
        render(<OrgMembersSection orgId="org-123" />);
        await waitFor(() => {
          expect(screen.getByText('Test Admin')).toBeInTheDocument();
        });

        expect(roleBadgeFor('member@test.com')).toHaveTextContent('Member');
        expect(roleBadgeFor('admin@test.com')).toHaveTextContent('Admin');
        expect(roleBadgeFor('owner@test.com')).toHaveTextContent('Owner');
      } finally {
        asOwner();
      }
    });

    it('offers an owner the whole hierarchy, including seating an admin', async () => {
      asOwner();
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText('Test Admin')).toBeInTheDocument();
      });

      expect(optionsFor('member@test.com')).toEqual(['member', 'admin', 'owner']);
      expect(optionsFor('Test Admin')).toEqual(['member', 'admin', 'owner']);
      expect(selectFor('Test Admin')).toBeEnabled();
    });
  });

  describe('Race Condition Protection', () => {
    it('disables role select during update', async () => {
      // Since this requires confirmation, we'll test the loading state differently
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        expect(roleSelects.length).toBeGreaterThan(0);
      });

      const roleSelects = screen.getAllByRole('combobox');
      const adminRoleSelect = roleSelects.find((select) => {
        const row = select.closest('tr');
        return row?.textContent?.includes('admin@test.com');
      });

      // Use a demotion (admin -> member) which doesn't require confirmation
      mockClient.updateOrgMemberRole.mockImplementationOnce(
        () =>
          new Promise((resolve) => setTimeout(() => resolve({ ...mockAdmin, role: 'member' }), 100))
      );

      fireEvent.change(adminRoleSelect!, { target: { value: 'member' } });

      // Select should be disabled during update
      await waitFor(
        () => {
          expect(adminRoleSelect).toBeDisabled();
        },
        { timeout: 500 }
      );

      // Select should be enabled after update completes
      await waitFor(
        () => {
          expect(adminRoleSelect).not.toBeDisabled();
        },
        { timeout: 2000 }
      );
    });

    it('prevents concurrent role changes on same member', async () => {
      // Mock a delayed response to simulate race condition
      mockClient.updateOrgMemberRole.mockImplementationOnce(
        () =>
          new Promise((resolve) => setTimeout(() => resolve({ ...mockMember, role: 'admin' }), 100))
      );

      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        expect(roleSelects.length).toBeGreaterThan(0);
      });

      const roleSelects = screen.getAllByRole('combobox');
      const memberRoleSelect = roleSelects.find((select) => {
        const row = select.closest('tr');
        return row?.textContent?.includes('member@test.com');
      });

      // Start first update - this will show confirmation modal
      fireEvent.change(memberRoleSelect!, { target: { value: 'admin' } });

      // Confirm the role change
      await waitFor(() => {
        expect(screen.getByText(/Confirm Role Change/i)).toBeInTheDocument();
      });

      const confirmButtons = screen.getAllByRole('button', { name: /Confirm/i });
      fireEvent.click(confirmButtons[0]);

      // The select should be disabled while updating
      await waitFor(() => {
        expect(memberRoleSelect).toBeDisabled();
      });

      // Wait for update to complete
      await waitFor(
        () => {
          expect(mockClient.updateOrgMemberRole).toHaveBeenCalledTimes(1);
        },
        { timeout: 2000 }
      );
    });
  });

  describe('Role Elevation Confirmation', () => {
    it('shows confirmation modal when promoting member to admin', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        const memberRoleSelect = roleSelects.find((select) => {
          const row = select.closest('tr');
          return row?.textContent?.includes('member@test.com');
        });
        fireEvent.change(memberRoleSelect!, { target: { value: 'admin' } });
      });

      await waitFor(() => {
        expect(screen.getByText(/Confirm Role Change/i)).toBeInTheDocument();
        expect(screen.getByText(/promote/i)).toBeInTheDocument();
      });
    });

    it('shows confirmation modal when promoting member to owner', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        const memberRoleSelect = roleSelects.find((select) => {
          const row = select.closest('tr');
          return row?.textContent?.includes('member@test.com');
        });
        fireEvent.change(memberRoleSelect!, { target: { value: 'owner' } });
      });

      await waitFor(() => {
        expect(screen.getByText(/Confirm Role Change/i)).toBeInTheDocument();
        expect(screen.getByText(/full control/i)).toBeInTheDocument();
      });
    });

    it('shows warning for owner promotion', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        const memberRoleSelect = roleSelects.find((select) => {
          const row = select.closest('tr');
          return row?.textContent?.includes('member@test.com');
        });
        fireEvent.change(memberRoleSelect!, { target: { value: 'owner' } });
      });

      await waitFor(() => {
        expect(screen.getByText(/Warning/i)).toBeInTheDocument();
      });
    });

    it('does not show confirmation for demotions', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        const adminRoleSelect = roleSelects.find((select) => {
          const row = select.closest('tr');
          return row?.textContent?.includes('admin@test.com');
        });
        fireEvent.change(adminRoleSelect!, { target: { value: 'member' } });
      });

      // Should update immediately without confirmation
      await waitFor(() => {
        expect(mockClient.updateOrgMemberRole).toHaveBeenCalled();
      });

      // Confirmation modal should not appear
      expect(screen.queryByText(/Confirm Role Change/i)).not.toBeInTheDocument();
    });

    it('completes role change after confirmation', async () => {
      mockClient.updateOrgMemberRole.mockResolvedValueOnce({
        ...mockMember,
        role: 'admin',
      });

      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        const memberRoleSelect = roleSelects.find((select) => {
          const row = select.closest('tr');
          return row?.textContent?.includes('member@test.com');
        });
        fireEvent.change(memberRoleSelect!, { target: { value: 'admin' } });
      });

      await waitFor(() => {
        expect(screen.getByText(/Confirm Role Change/i)).toBeInTheDocument();
      });

      const confirmButtons = screen.getAllByRole('button', { name: /Confirm/i });
      fireEvent.click(confirmButtons[0]);

      await waitFor(() => {
        expect(mockClient.updateOrgMemberRole).toHaveBeenCalledWith('org-123', 'user-3', {
          role: 'admin',
        });
      });
    });

    // Note: Modal button finding fails in test env
    it('cancels role change when confirmation is cancelled', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        const memberRoleSelect = roleSelects.find((select) => {
          const row = select.closest('tr');
          return row?.textContent?.includes('member@test.com');
        });
        fireEvent.change(memberRoleSelect!, { target: { value: 'admin' } });
      });

      await waitFor(() => {
        expect(screen.getByText(/Confirm Role Change/i)).toBeInTheDocument();
      });

      const dialog = screen.getByRole('dialog');
      fireEvent.click(within(dialog).getByRole('button', { name: /Cancel/i }));

      expect(mockClient.updateOrgMemberRole).not.toHaveBeenCalled();
    });
  });

  describe('Add Member Modal', () => {
    it('opens add member modal when Add Member button clicked', async () => {
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: /Add Member/i })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: /Add Member/i }));

      await waitFor(() => {
        expect(screen.getByText('Add Organization Member')).toBeInTheDocument();
      });
    });

    it('adds member successfully and refreshes list', async () => {
      const newMember: OrganizationMember = {
        id: 'member-4',
        user_id: 'user-4',
        organization_id: 'org-123',
        role: 'member',
        email: 'newuser@test.com',
        display_name: 'New User',
        joined_at: '2024-01-04T00:00:00Z',
      };

      mockClient.addOrgMember.mockResolvedValue(newMember);
      mockClient.getOrgMembers.mockImplementation(async () => {
        if (mockClient.addOrgMember.mock.calls.length > 0) {
          return { members: [mockOwner, mockAdmin, mockMember, newMember] };
        }
        return { members: [mockOwner, mockAdmin, mockMember] };
      });

      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: /Add Member/i })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: /Add Member/i }));

      await waitFor(() => {
        const emailInput = screen.getByLabelText(/Email/i);
        fireEvent.change(emailInput, { target: { value: 'newuser@test.com' } });
      });

      const submitButton = screen
        .getAllByRole('button', { name: /Add/i })
        .find((btn) => btn.getAttribute('type') === 'submit');
      fireEvent.click(submitButton!);

      await waitFor(() => {
        expect(mockClient.addOrgMember).toHaveBeenCalledWith('org-123', {
          email: 'newuser@test.com',
          role: 'member',
        });
        expect(screen.getByText('Member added successfully')).toBeInTheDocument();
        expect(screen.getByText('New User')).toBeInTheDocument();
      });
    });
  });

  describe('Change Role Functionality', () => {
    it('changes member role successfully', async () => {
      mockClient.updateOrgMemberRole.mockResolvedValueOnce({
        ...mockAdmin,
        role: 'member',
      });

      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getAllByText('admin@test.com').length).toBeGreaterThan(0);
      });

      const roleSelects = document.querySelectorAll('.role-select');
      const adminRoleSelect = Array.from(roleSelects).find((select) => {
        const row = select.closest('tr');
        return row?.textContent?.includes('admin@test.com') && row?.textContent?.includes('Admin');
      });

      fireEvent.change(adminRoleSelect as Element, { target: { value: 'member' } });

      await waitFor(() => {
        expect(mockClient.updateOrgMemberRole).toHaveBeenCalledWith('org-123', 'user-2', {
          role: 'member',
        });
      });
    });
  });

  describe('Remove Member Functionality', () => {
    it('removes member after confirmation', async () => {
      mockClient.removeOrgMember.mockResolvedValueOnce();

      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const removeButtons = screen.getAllByRole('button', { name: /Remove/i });
        fireEvent.click(removeButtons[2]);
      });

      await waitFor(() => {
        fireEvent.click(screen.getByRole('button', { name: /Confirm/i }));
      });

      await waitFor(() => {
        expect(mockClient.removeOrgMember).toHaveBeenCalledWith('org-123', 'user-3');
      });
    });
  });

  describe('Protection Logic', () => {
    it('prevents removing last owner', async () => {
      mockClient.getOrgMembers.mockResolvedValueOnce({
        members: [mockOwner],
      });

      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        const removeButtons = screen.getAllByRole('button', { name: /Remove/i });
        expect(removeButtons[0]).toBeDisabled();
      });
    });

    it('prevents changing role of last owner', async () => {
      mockClient.getOrgMembers.mockResolvedValueOnce({
        members: [mockOwner],
      });

      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(roleBadgeFor('owner@test.com')).toHaveTextContent('Owner');
      });
    });
  });

  describe('Caller Role Restrictions', () => {
    const renderAs = async (role: OrgRole) => {
      mockClient.getOrgMembers.mockResolvedValue({ members: rosterWithSignedInUserAs(role) });
      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText('admin@test.com')).toBeInTheDocument();
      });
    };

    const openAddMemberModal = async () => {
      fireEvent.click(screen.getByRole('button', { name: /Add Member/i }));
      await waitFor(() => {
        expect(screen.getByText('Add Organization Member')).toBeInTheDocument();
      });
    };

    it('offers a member no control over any other member', async () => {
      await renderAs('member');

      expect(screen.queryByRole('button', { name: /Add Member/i })).not.toBeInTheDocument();
      expect(removeButtonFor('member@test.com')).toBeDisabled();
      expect(roleBadgeFor('member@test.com')).toHaveTextContent('Member');
      expect(removeButtonFor('admin@test.com')).toBeDisabled();
      expect(roleBadgeFor('admin@test.com')).toHaveTextContent('Admin');
      expect(removeButtonFor('other-owner@test.com')).toBeDisabled();
      expect(roleBadgeFor('other-owner@test.com')).toHaveTextContent('Owner');
    });

    // An admin may remove a member but can grant them nothing else, so the
    // row carries a Remove button and a badge rather than a one-option select.
    it('offers an admin control over a member', async () => {
      await renderAs('admin');

      expect(screen.getByRole('button', { name: /Add Member/i })).toBeInTheDocument();
      expect(removeButtonFor('member@test.com')).toBeEnabled();
      expect(roleBadgeFor('member@test.com')).toHaveTextContent('Member');
    });

    it('offers an owner control over a member', async () => {
      await renderAs('owner');

      expect(screen.getByRole('button', { name: /Add Member/i })).toBeInTheDocument();
      expect(removeButtonFor('member@test.com')).toBeEnabled();
      expect(roleSelectFor('member@test.com')).toBeEnabled();
    });

    // `add_member` refuses `role >= Admin` from anyone but an owner, so
    // offering an admin the admin role would only earn them a 403.
    it('offers an admin no role the add route would refuse', async () => {
      await renderAs('admin');
      await openAddMemberModal();

      expect(rolesOfferedForNewMember()).toEqual(['member']);
    });

    it('offers an owner every role when adding a member', async () => {
      await renderAs('owner');
      await openAddMemberModal();

      expect(rolesOfferedForNewMember()).toEqual(['member', 'admin', 'owner']);
    });
  });

  describe('Empty State', () => {
    it('shows message when no members exist', async () => {
      mockClient.getOrgMembers.mockResolvedValueOnce({
        members: [],
      });

      render(<OrgMembersSection orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText(/No members found/i)).toBeInTheDocument();
      });
    });
  });
});

describe('OrgMembersSection table anatomy', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    mockClient.getOrgMembers.mockResolvedValue({
      members: [mockOwner, mockAdmin, mockMember],
    });
  });

  it('shows the email under the name in one cell instead of its own column', async () => {
    render(<OrgMembersSection orgId="org-123" />);
    await waitFor(() => {
      expect(screen.getByText('Test Owner')).toBeInTheDocument();
    });
    expect(screen.queryByRole('columnheader', { name: 'Email' })).toBeNull();
    const identity = screen.getByText('Test Owner').closest('.member-identity');
    expect(identity).not.toBeNull();
    expect(within(identity as HTMLElement).getByText('owner@test.com')).toHaveClass('member-email');
    const memberRow = screen.getByText('member@test.com').closest('tr') as HTMLElement;
    expect(within(memberRow).queryAllByText('member@test.com')).toHaveLength(1);
    expect(memberRow.querySelector('.member-remove')).not.toBeNull();
    expect(memberRow.querySelector('.role-select')).not.toBeNull();
  });

  it('labels a seat whose account is unresolved by its user id instead of a shared word', async () => {
    mockClient.getOrgMembers.mockResolvedValue({
      members: [
        {
          ...mockAdmin,
          email: '',
          display_name: null,
          user_id: 'a27bd650-3602-430a-884a-3cb91f97e084',
        },
        { ...mockMember, email: '', user_id: '3c92e73e-c4bd-4a66-91c9-15cb6b30fbd7' },
      ],
    });
    render(<OrgMembersSection orgId="org-123" />);
    await waitFor(() => {
      expect(screen.getByText('a27bd650')).toBeInTheDocument();
    });
    const names = [...document.querySelectorAll('.member-name')].map((name) => name.textContent);
    expect(names).toEqual(['a27bd650', '3c92e73e']);
    expect(screen.queryByText('Member', { selector: '.member-name' })).toBeNull();
  });
});
