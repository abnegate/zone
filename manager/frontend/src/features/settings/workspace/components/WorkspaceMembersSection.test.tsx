import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { clientModule } from '../../../../test/client';
import { chooseSelectElement } from '../../../../test/select';
import { ACTIVE, AUDITOR, LAPSED, RELEASE_MANAGER } from '../../../enterprise/roles/fixtures';
import { RoleRefusal } from '../../../enterprise/roles/RoleRefusal';
import { Permission, RoleCode } from '../../../enterprise/roles/schemas';
import type { RolesResponse } from '../../../enterprise/roles/types';
import { customRoleValue } from '../../assignment';
import type { OrganizationMember, WorkspaceMember, WorkspaceRole } from '../types';

const mockClient = {
  getWorkspaceMembers: mock(),
  getOrgMembers: mock(),
  addWorkspaceMember: mock(),
  updateWorkspaceMemberRole: mock(),
  removeWorkspaceMember: mock(),
};

mock.module('../../../../api/client', () => clientModule(mockClient));

const mockUseAuth = mock(() => ({
  isAuthenticated: true,
  user: { id: 'user-1', email: 'owner@test.com' },
}));

mock.module('../../../auth', () => ({
  useAuth: mockUseAuth,
}));

let rolesResponse: RolesResponse | null = null;

const mockRefreshRoles = mock(async () => {});

const mockUseRoles = mock((_organizationId: string | null) => ({
  response: rolesResponse,
  loading: false,
  error: null,
  refresh: mockRefreshRoles,
}));

mock.module('../../../enterprise/roles/hooks/useRoles', () => ({ useRoles: mockUseRoles }));

let WorkspaceMembersSection: typeof import('./WorkspaceMembersSection').default;

beforeAll(async () => {
  WorkspaceMembersSection = (await import('./WorkspaceMembersSection')).default;
});

afterAll(() => {
  mock.restore();
});

const mockOwner: WorkspaceMember = {
  id: 'ws-member-1',
  user_id: 'user-1',
  workspace_id: 'ws-123',
  role: 'owner',
  custom_role: null,
  email: 'owner@test.com',
  display_name: 'Test Owner',
  joined_at: '2024-01-01T00:00:00Z',
};

const mockAdmin: WorkspaceMember = {
  id: 'ws-member-2',
  user_id: 'user-2',
  workspace_id: 'ws-123',
  role: 'admin',
  custom_role: null,
  email: 'admin@test.com',
  display_name: 'Test Admin',
  joined_at: '2024-01-02T00:00:00Z',
};

const mockMember: WorkspaceMember = {
  id: 'ws-member-3',
  user_id: 'user-3',
  workspace_id: 'ws-123',
  role: 'member',
  custom_role: null,
  email: 'member@test.com',
  display_name: null,
  joined_at: '2024-01-03T00:00:00Z',
};

const mockViewer: WorkspaceMember = {
  id: 'ws-member-4',
  user_id: 'user-4',
  workspace_id: 'ws-123',
  role: 'viewer',
  custom_role: null,
  email: 'viewer@test.com',
  display_name: 'Test Viewer',
  joined_at: '2024-01-04T00:00:00Z',
};

const mockOrgMember1: OrganizationMember = {
  id: 'org-member-1',
  user_id: 'user-5',
  organization_id: 'org-123',
  role: 'member',
  custom_role: null,
  email: 'orgmember@test.com',
  display_name: 'Org Member',
  joined_at: '2024-01-05T00:00:00Z',
};

const rosterWithSignedInUserAs = (role: WorkspaceRole): WorkspaceMember[] => [
  {
    ...mockOwner,
    id: 'ws-member-0',
    user_id: 'user-0',
    email: 'other-owner@test.com',
    display_name: 'Other Owner',
  },
  // A second owner, so it is the caller's role and not the last-owner rule that
  // disables the owner row.
  {
    ...mockOwner,
    id: 'ws-member-8',
    user_id: 'user-8',
    email: 'spare-owner@test.com',
    display_name: 'Spare Owner',
  },
  mockAdmin,
  mockMember,
  mockViewer,
  {
    ...mockMember,
    id: 'ws-member-9',
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
  const dialog = screen.getByRole('dialog');
  const trigger = within(dialog).getByRole('combobox', { name: 'Role', hidden: true });
  if (trigger.getAttribute('data-state') !== 'open') {
    fireEvent.keyDown(trigger, { key: 'ArrowDown' });
  }
  const values = screen
    .getAllByRole('option')
    .map((option) => option.getAttribute('data-value') ?? '');
  fireEvent.keyDown(trigger, { key: 'Escape' });
  return values;
};

describe('WorkspaceMembersSection', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    rolesResponse = null;
    mockClient.getWorkspaceMembers.mockResolvedValue({
      members: [mockOwner, mockAdmin, mockMember, mockViewer],
    });
    mockClient.getOrgMembers.mockResolvedValue({
      members: [
        {
          ...mockOwner,
          organization_id: 'org-123',
          workspace_id: undefined,
        } as unknown as OrganizationMember,
        {
          ...mockAdmin,
          organization_id: 'org-123',
          workspace_id: undefined,
        } as unknown as OrganizationMember,
        {
          ...mockMember,
          organization_id: 'org-123',
          workspace_id: undefined,
        } as unknown as OrganizationMember,
        {
          ...mockViewer,
          organization_id: 'org-123',
          workspace_id: undefined,
        } as unknown as OrganizationMember,
        mockOrgMember1,
      ],
    });
  });

  describe('Loading State', () => {
    it('shows loading state initially', () => {
      mockClient.getWorkspaceMembers.mockImplementation(() => new Promise(() => {}));
      mockClient.getOrgMembers.mockImplementation(() => new Promise(() => {}));
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      expect(screen.getByText('Loading members...')).toBeInTheDocument();
    });

    it('shows error when loading workspace members fails', async () => {
      mockClient.getWorkspaceMembers.mockRejectedValueOnce(
        new Error('Failed to load workspace members')
      );
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText(/Failed to load workspace members/i)).toBeInTheDocument();
      });
    });

    it('shows error when loading org members fails', async () => {
      mockClient.getOrgMembers.mockRejectedValueOnce(new Error('Failed to load org members'));
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText(/Failed to load org members/i)).toBeInTheDocument();
      });
    });
  });

  describe('Members Table', () => {
    it('renders members table with all workspace members', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText('owner@test.com')).toBeInTheDocument();
        expect(screen.getByText('admin@test.com')).toBeInTheDocument();
        expect(screen.getAllByText('member@test.com').length).toBeGreaterThan(0);
        expect(screen.getByText('viewer@test.com')).toBeInTheDocument();
      });
    });

    it('displays member names when available', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText('Test Owner')).toBeInTheDocument();
        expect(screen.getByText('Test Admin')).toBeInTheDocument();
        expect(screen.getByText('Test Viewer')).toBeInTheDocument();
      });
    });

    it('displays email when display name is null', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const emailElements = screen.getAllByText('member@test.com');
        expect(emailElements.length).toBeGreaterThan(0);
      });
    });

    it('shows the email under the name and never labels two unresolved seats alike', async () => {
      mockClient.getWorkspaceMembers.mockResolvedValue({
        members: [
          mockOwner,
          { ...mockMember, email: '', user_id: 'a27bd650-3602-430a-884a-3cb91f97e084' },
          {
            ...mockViewer,
            email: '',
            display_name: null,
            user_id: '3c92e73e-c4bd-4a66-91c9-15cb6b30fbd7',
          },
        ],
      });
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText('Test Owner')).toBeInTheDocument();
      });
      const identity = screen.getByText('Test Owner').closest('.member-identity');
      expect(within(identity as HTMLElement).getByText('owner@test.com')).toHaveClass(
        'member-email'
      );
      const names = [...document.querySelectorAll('.member-name')].map((name) => name.textContent);
      expect(names).toEqual(['Test Owner', 'a27bd650', '3c92e73e']);
      expect(
        screen.getByRole('combobox', { name: 'Change role for a27bd650' })
      ).toBeInTheDocument();
    });

    it('shows one role control per row: a badge where it is fixed, a select where it can change', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText('Test Owner')).toBeInTheDocument();
      });
      expect(roleBadgeFor('owner@test.com')).toHaveClass('role-badge-owner');
      expect(roleSelectFor('admin@test.com')).toHaveTextContent('Admin');
      expect(roleSelectFor('member@test.com')).toHaveTextContent('Member');
      expect(roleSelectFor('viewer@test.com')).toHaveTextContent('Viewer');
      expect(document.querySelectorAll('.role-badge')).toHaveLength(1);
    });

    it('formats joined dates using user locale', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const text = screen.getByRole('table').textContent;
        expect(text).toContain('Jan');
      });
    });

    it('shows Add Member button', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: /Add Member/i })).toBeInTheDocument();
      });
    });
  });

  describe('Add Member Modal', () => {
    it('opens add member modal when Add Member button clicked', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: /Add Member/i })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: /Add Member/i }));

      await waitFor(() => {
        expect(screen.getByText('Add Workspace Member')).toBeInTheDocument();
      });
    });

    it('shows dropdown of org members not in workspace', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        fireEvent.click(screen.getByRole('button', { name: /Add Member/i }));
      });

      await waitFor(() => {
        const select = screen.getByLabelText(/User/i);
        expect(select).toBeInTheDocument();
        // Should show org member who's not in workspace
        expect(screen.getByText(/Org Member/i)).toBeInTheDocument();
      });
    });

    it('adds member successfully and refreshes list', async () => {
      const newMember: WorkspaceMember = {
        id: 'ws-member-5',
        user_id: 'user-5',
        workspace_id: 'ws-123',
        role: 'member',
        custom_role: null,
        email: 'orgmember@test.com',
        display_name: 'Org Member',
        joined_at: '2024-01-05T00:00:00Z',
      };

      // The refresh has to be staged from inside the add, not queued up front:
      // a `mockResolvedValueOnce` here is consumed by the *initial* load, which
      // would make user-5 an existing workspace member and filter it out of the
      // very select this test needs to choose from.
      mockClient.addWorkspaceMember.mockImplementationOnce(async () => {
        mockClient.getWorkspaceMembers.mockResolvedValue({
          members: [mockOwner, mockAdmin, mockMember, mockViewer, newMember],
        });
        return newMember;
      });

      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: /Add Member/i })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: /Add Member/i }));

      await waitFor(() => {
        expect(screen.getByText('Add Workspace Member')).toBeInTheDocument();
      });

      // `@zone/ui`'s Select is a Radix one: the labelled element is a trigger
      // button, not a native select, so it has to be opened and its item
      // chosen rather than assigned a value.
      const user = userEvent.setup();
      const dialog = screen.getByRole('dialog');
      const trigger = await waitFor(() => within(dialog).getByRole('combobox', { name: /User/i }));
      await user.click(trigger);
      await user.click(await screen.findByRole('option', { name: /orgmember@test\.com/ }));

      // Find all forms (there should be one visible modal form)
      const forms = document.querySelectorAll('form');
      const addMemberForm = Array.from(forms).find((f) => f.querySelector('button[type="submit"]'));

      expect(addMemberForm).toBeDefined();
      fireEvent.submit(addMemberForm!);

      await waitFor(
        () => {
          expect(mockClient.addWorkspaceMember).toHaveBeenCalledWith('ws-123', {
            user_id: 'user-5',
            role: 'member',
          });
        },
        { timeout: 3000 }
      );

      await waitFor(() => {
        expect(mockClient.getWorkspaceMembers).toHaveBeenCalledTimes(2);
      });
      // The refreshed list is what the reader actually sees.
      await waitFor(() => {
        expect(screen.getAllByText('orgmember@test.com').length).toBeGreaterThan(0);
      });
    });

    it('validates user selection', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        fireEvent.click(screen.getByRole('button', { name: /Add Member/i }));
      });

      const submitButton = screen
        .getAllByRole('button', { name: /Add/i })
        .find((btn) => btn.getAttribute('type') === 'submit');
      fireEvent.click(submitButton!);

      await waitFor(() => {
        expect(screen.getByText(/User is required/i)).toBeInTheDocument();
        expect(mockClient.addWorkspaceMember).not.toHaveBeenCalled();
      });
    });
  });

  describe('Role Change Functionality', () => {
    it('shows confirmation modal when promoting member to admin', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        const memberRoleSelect = roleSelects.find((select) => {
          const row = select.closest('tr');
          return row?.textContent?.includes('member@test.com');
        });
        chooseSelectElement(memberRoleSelect!, 'Admin');
      });

      await waitFor(() => {
        expect(screen.getByText(/Confirm Role Change/i)).toBeInTheDocument();
        expect(screen.getByText(/promote/i)).toBeInTheDocument();
      });
    });

    it('shows warning for owner promotion', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        const memberRoleSelect = roleSelects.find((select) => {
          const row = select.closest('tr');
          return row?.textContent?.includes('member@test.com');
        });
        chooseSelectElement(memberRoleSelect!, 'Owner');
      });

      await waitFor(() => {
        expect(screen.getByText(/Warning/i)).toBeInTheDocument();
      });
    });

    it('does not show confirmation for demotions', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        const adminRoleSelect = roleSelects.find((select) => {
          const row = select.closest('tr');
          return row?.textContent?.includes('admin@test.com');
        });
        chooseSelectElement(adminRoleSelect!, 'Viewer');
      });

      // Should update immediately without confirmation
      await waitFor(() => {
        expect(mockClient.updateWorkspaceMemberRole).toHaveBeenCalled();
      });

      // Confirmation modal should not appear
      expect(screen.queryByText(/Confirm Role Change/i)).not.toBeInTheDocument();
    });

    it('completes role change after confirmation', async () => {
      mockClient.updateWorkspaceMemberRole.mockResolvedValueOnce({
        ...mockMember,
        role: 'admin',
      });

      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        const memberRoleSelect = roleSelects.find((select) => {
          const row = select.closest('tr');
          return row?.textContent?.includes('member@test.com');
        });
        chooseSelectElement(memberRoleSelect!, 'Admin');
      });

      await waitFor(() => {
        expect(screen.getByText(/Confirm Role Change/i)).toBeInTheDocument();
      });

      const confirmButtons = screen.getAllByRole('button', { name: /Confirm/i });
      fireEvent.click(confirmButtons[0]);

      await waitFor(() => {
        expect(mockClient.updateWorkspaceMemberRole).toHaveBeenCalledWith('ws-123', 'user-3', {
          role: 'admin',
        });
      });
    });
  });

  describe('Remove Member Functionality', () => {
    it('removes member after confirmation', async () => {
      mockClient.removeWorkspaceMember.mockResolvedValueOnce(undefined);

      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const removeButtons = screen.getAllByRole('button', { name: /Remove/i });
        fireEvent.click(removeButtons[2]);
      });

      await waitFor(() => {
        fireEvent.click(screen.getByRole('button', { name: /Confirm/i }));
      });

      await waitFor(() => {
        expect(mockClient.removeWorkspaceMember).toHaveBeenCalledWith('ws-123', 'user-3');
      });
    });
  });

  describe('Protection Logic', () => {
    it('prevents removing last owner', async () => {
      mockClient.getWorkspaceMembers.mockResolvedValueOnce({
        members: [mockOwner],
      });

      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const removeButtons = screen.getAllByRole('button', { name: /Remove/i });
        expect(removeButtons[0]).toBeDisabled();
      });
    });

    it('prevents changing role of last owner', async () => {
      mockClient.getWorkspaceMembers.mockResolvedValueOnce({
        members: [mockOwner],
      });

      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(roleBadgeFor('owner@test.com')).toHaveTextContent('Owner');
      });
    });

    it('prevents user from modifying themselves', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(roleBadgeFor('owner@test.com')).toHaveTextContent('Owner');
      });
    });
  });

  describe('Caller Role Restrictions', () => {
    const renderAs = async (role: WorkspaceRole) => {
      mockClient.getWorkspaceMembers.mockResolvedValue({ members: rosterWithSignedInUserAs(role) });
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText('admin@test.com')).toBeInTheDocument();
      });
    };

    const openAddMemberModal = async () => {
      fireEvent.click(screen.getByRole('button', { name: /Add Member/i }));
      await waitFor(() => {
        expect(screen.getByText('Add Workspace Member')).toBeInTheDocument();
      });
    };

    it('offers a viewer no control over any other member', async () => {
      await renderAs('viewer');

      expect(screen.queryByRole('button', { name: /Add Member/i })).not.toBeInTheDocument();
      expect(screen.queryByRole('columnheader', { name: 'Actions' })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /Remove/i })).not.toBeInTheDocument();
      expect(roleBadgeFor('viewer@test.com')).toHaveTextContent('Viewer');
      expect(roleBadgeFor('member@test.com')).toHaveTextContent('Member');
      expect(roleBadgeFor('admin@test.com')).toHaveTextContent('Admin');
      expect(roleBadgeFor('other-owner@test.com')).toHaveTextContent('Owner');
    });

    it('offers a member no control over any other member', async () => {
      await renderAs('member');

      expect(screen.queryByRole('button', { name: /Add Member/i })).not.toBeInTheDocument();
      expect(screen.queryByRole('columnheader', { name: 'Actions' })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /Remove/i })).not.toBeInTheDocument();
      expect(roleBadgeFor('viewer@test.com')).toHaveTextContent('Viewer');
      expect(roleBadgeFor('member@test.com')).toHaveTextContent('Member');
      expect(roleBadgeFor('admin@test.com')).toHaveTextContent('Admin');
      expect(roleBadgeFor('other-owner@test.com')).toHaveTextContent('Owner');
    });

    it('offers an admin control over a member', async () => {
      await renderAs('admin');

      expect(screen.getByRole('columnheader', { name: 'Actions' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /Add Member/i })).toBeInTheDocument();
      expect(removeButtonFor('member@test.com')).toBeEnabled();
      expect(roleSelectFor('member@test.com')).toBeEnabled();
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

      expect(rolesOfferedForNewMember()).toEqual(['viewer', 'member']);
    });

    it('offers an owner every role when adding a member', async () => {
      await renderAs('owner');
      await openAddMemberModal();

      expect(rolesOfferedForNewMember()).toEqual(['viewer', 'member', 'admin', 'owner']);
    });
  });

  describe('Empty State', () => {
    it('shows message when no members exist', async () => {
      mockClient.getWorkspaceMembers.mockResolvedValueOnce({
        members: [],
      });

      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByText(/No members found/i)).toBeInTheDocument();
      });
    });
  });

  describe('Accessibility', () => {
    it('has ARIA labels on role selects', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = document.querySelectorAll('.role-select');
        roleSelects.forEach((select) => {
          expect(select).toHaveAttribute('aria-label');
        });
      });
    });

    it('has ARIA roles on alerts', async () => {
      mockClient.getWorkspaceMembers.mockRejectedValueOnce(new Error('Failed'));
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const alert = screen.getByRole('alert');
        expect(alert).toBeInTheDocument();
      });
    });
  });

  describe('Race Condition Protection', () => {
    it('disables role select during update', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        expect(roleSelects.length).toBeGreaterThan(0);
      });

      const roleSelects = screen.getAllByRole('combobox');
      const viewerRoleSelect = roleSelects.find((select) => {
        const row = select.closest('tr');
        return row?.textContent?.includes('viewer@test.com');
      });

      // Use a demotion (viewer -> member) which doesn't require confirmation
      mockClient.updateWorkspaceMemberRole.mockImplementationOnce(
        () =>
          new Promise((resolve) =>
            setTimeout(() => resolve({ ...mockViewer, role: 'member' }), 100)
          )
      );

      chooseSelectElement(viewerRoleSelect!, 'Member');

      // Select should be disabled during update
      await waitFor(
        () => {
          expect(viewerRoleSelect).toBeDisabled();
        },
        { timeout: 500 }
      );

      // Select should be enabled after update completes
      await waitFor(
        () => {
          expect(viewerRoleSelect).not.toBeDisabled();
        },
        { timeout: 2000 }
      );
    });
  });
});

describe('WorkspaceMembersSection custom roles', () => {
  const releaseManager = { id: RELEASE_MANAGER.id, name: RELEASE_MANAGER.name };

  const holdingReleaseManager: WorkspaceMember = {
    ...mockViewer,
    role: 'member',
    custom_role: releaseManager,
    permissions: RELEASE_MANAGER.permissions,
  };

  const optionsOf = (email: string): string[] => {
    const trigger = roleSelectFor(email);
    fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    const values = screen
      .getAllByRole('option')
      .map((option) => option.getAttribute('data-value') ?? '');
    fireEvent.keyDown(trigger, { key: 'Escape' });
    return values;
  };

  const renderRoster = async (members: WorkspaceMember[]) => {
    mockClient.getWorkspaceMembers.mockResolvedValue({ members });
    mockClient.getOrgMembers.mockResolvedValue({ members: [mockOrgMember1] });
    render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
    await waitFor(() => {
      expect(screen.getByText('viewer@test.com')).toBeInTheDocument();
    });
  };

  beforeEach(() => {
    mock.clearAllMocks();
    rolesResponse = ACTIVE;
    mockUseAuth.mockReturnValue({
      isAuthenticated: true,
      user: { id: 'user-1', email: 'owner@test.com' },
    });
  });

  it('offers the workspace roles of the organization after the built-in ones when licensed', async () => {
    await renderRoster([mockOwner, mockAdmin, mockMember, mockViewer]);

    expect(optionsOf('viewer@test.com')).toEqual([
      'viewer',
      'member',
      'admin',
      'owner',
      customRoleValue(RELEASE_MANAGER.id),
    ]);
    expect(optionsOf('viewer@test.com')).not.toContain(customRoleValue(AUDITOR.id));
  });

  it('asks for the roles of the workspace organization', async () => {
    await renderRoster([mockOwner, mockAdmin, mockMember, mockViewer]);

    expect(mockUseRoles).toHaveBeenLastCalledWith('org-123');
  });

  it('offers no custom roles without an active license', async () => {
    rolesResponse = LAPSED;
    await renderRoster([mockOwner, mockAdmin, mockMember, mockViewer]);

    expect(optionsOf('viewer@test.com')).toEqual(['viewer', 'member', 'admin', 'owner']);
  });

  it('shows the custom role a member holds by name', async () => {
    await renderRoster([mockOwner, mockAdmin, mockMember, holdingReleaseManager]);

    expect(roleSelectFor('viewer@test.com')).toHaveTextContent('Release manager');
  });

  it('shows the custom role a member holds by name to someone who cannot change it', async () => {
    await renderRoster(
      rosterWithSignedInUserAs('viewer').map((member) =>
        member.user_id === mockViewer.user_id ? holdingReleaseManager : member
      )
    );

    const badge = roleBadgeFor('viewer@test.com');
    expect(badge).toHaveTextContent('Release manager');
    expect(badge).toHaveClass('role-badge-custom');
    expect(mockUseRoles.mock.calls.every(([organizationId]) => organizationId === null)).toBe(true);
  });

  it('assigns a custom role by its id without asking to confirm', async () => {
    mockClient.updateWorkspaceMemberRole.mockResolvedValueOnce(holdingReleaseManager);
    await renderRoster([mockOwner, mockAdmin, mockMember, mockViewer]);

    chooseSelectElement(roleSelectFor('viewer@test.com'), /Release manager/);

    await waitFor(() => {
      expect(mockClient.updateWorkspaceMemberRole).toHaveBeenCalledWith('ws-123', 'user-4', {
        custom_role_id: RELEASE_MANAGER.id,
      });
    });
    expect(screen.queryByText('Confirm Role Change')).not.toBeInTheDocument();
    await waitFor(() => expect(mockRefreshRoles).toHaveBeenCalledTimes(1));
  });

  it('asks to confirm promoting a custom role holder to admin', async () => {
    await renderRoster([mockOwner, mockAdmin, mockMember, holdingReleaseManager]);

    chooseSelectElement(roleSelectFor('viewer@test.com'), 'Admin');

    expect(await screen.findByText('Confirm Role Change')).toBeInTheDocument();
    expect(mockClient.updateWorkspaceMemberRole).not.toHaveBeenCalled();
  });

  it('lets an admin give a viewer a custom role', async () => {
    await renderRoster(
      rosterWithSignedInUserAs('admin').filter((member) => member.user_id !== mockAdmin.user_id)
    );

    expect(optionsOf('viewer@test.com')).toEqual([
      'viewer',
      'member',
      customRoleValue(RELEASE_MANAGER.id),
    ]);
  });

  const refusals: [RoleCode, RegExp][] = [
    [RoleCode.Exceeds, /permissions you don't hold/],
    [RoleCode.LicenseRequired, /Zone Enterprise license/],
  ];

  for (const [code, message] of refusals) {
    it(`explains a ${code} refusal`, async () => {
      mockClient.updateWorkspaceMemberRole.mockRejectedValueOnce(
        new RoleRefusal({ error: 'Refused', code }, 403)
      );
      await renderRoster([mockOwner, mockAdmin, mockMember, mockViewer]);

      chooseSelectElement(roleSelectFor('viewer@test.com'), /Release manager/);

      expect(await screen.findByRole('alert')).toHaveTextContent(message);
    });
  }

  it('offers only built-in roles when adding a member', async () => {
    await renderRoster([mockOwner, mockAdmin, mockMember, mockViewer]);
    fireEvent.click(screen.getByRole('button', { name: /Add Member/i }));

    expect(rolesOfferedForNewMember()).toEqual(['viewer', 'member', 'admin', 'owner']);
  });
});

describe('WorkspaceMembersSection for a custom role holder', () => {
  const MEMBERS = [
    Permission.WorkspaceRead,
    Permission.WorkspaceMembersCreate,
    Permission.WorkspaceMembersUpdate,
    Permission.WorkspaceMembersDelete,
  ];

  const ADMIN_GRANT = [
    Permission.WorkspaceRead,
    Permission.WorkspaceWrite,
    Permission.WorkspaceUpdate,
    Permission.WorkspaceMembersCreate,
    Permission.WorkspaceMembersUpdate,
    Permission.WorkspaceMembersDelete,
    Permission.WorkspaceAiSettingsUpdate,
    Permission.WorkspaceMcpServersUpdate,
    Permission.WorkspacePoliciesUpdate,
    Permission.ProjectSyncsUpdate,
    Permission.PullRequestsOverride,
  ];

  const steward = (permissions: readonly string[]): WorkspaceMember => ({
    ...mockViewer,
    id: 'ws-member-9',
    user_id: 'user-1',
    email: 'self@test.com',
    display_name: 'Signed In User',
    custom_role: { id: 'role-steward', name: 'Steward' },
    permissions,
  });

  const otherOwner: WorkspaceMember = { ...mockOwner, id: 'ws-member-0', user_id: 'user-0' };

  const plainViewer: WorkspaceMember = { ...mockViewer, permissions: [Permission.WorkspaceRead] };

  const plainMember: WorkspaceMember = {
    ...mockMember,
    permissions: [Permission.WorkspaceRead, Permission.WorkspaceWrite],
  };

  const renderAs = async (caller: WorkspaceMember, others: WorkspaceMember[]) => {
    mockClient.getWorkspaceMembers.mockResolvedValue({ members: [otherOwner, ...others, caller] });
    mockClient.getOrgMembers.mockResolvedValue({ members: [mockOrgMember1] });
    render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
    await waitFor(() => {
      expect(screen.getByText('self@test.com')).toBeInTheDocument();
    });
  };

  const optionsOf = (email: string): string[] => {
    const trigger = roleSelectFor(email);
    fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    const values = screen
      .getAllByRole('option')
      .map((option) => option.getAttribute('data-value') ?? '');
    fireEvent.keyDown(trigger, { key: 'Escape' });
    return values;
  };

  beforeEach(() => {
    mock.clearAllMocks();
    rolesResponse = ACTIVE;
    mockUseAuth.mockReturnValue({
      isAuthenticated: true,
      user: { id: 'user-1', email: 'self@test.com' },
    });
  });

  it('lets a holder of workspace_members:create add a viewer, and a member only with write', async () => {
    await renderAs(steward(MEMBERS), [plainViewer]);
    fireEvent.click(screen.getByRole('button', { name: /Add Member/i }));
    await waitFor(() => {
      expect(screen.getByText('Add Workspace Member')).toBeInTheDocument();
    });

    expect(rolesOfferedForNewMember()).toEqual(['viewer']);
  });

  it('offers a writer the member role and the workspace roles their grant covers', async () => {
    await renderAs(
      steward([
        ...MEMBERS,
        Permission.WorkspaceWrite,
        Permission.ProjectSyncsUpdate,
        Permission.PullRequestsOverride,
      ]),
      [plainViewer]
    );

    expect(optionsOf('viewer@test.com')).toEqual([
      'viewer',
      'member',
      customRoleValue(RELEASE_MANAGER.id),
    ]);
  });

  it('leaves a member alone when the holder lacks workspace:write', async () => {
    await renderAs(steward(MEMBERS), [plainMember]);

    expect(roleBadgeFor('member@test.com')).toHaveTextContent('Member');
    expect(removeButtonFor('member@test.com')).toBeDisabled();
  });

  it('never lets a holder touch a built-in admin', async () => {
    await renderAs(steward(ADMIN_GRANT), [{ ...mockAdmin, permissions: ADMIN_GRANT }]);

    expect(roleBadgeFor('admin@test.com')).toHaveTextContent('Admin');
    expect(removeButtonFor('admin@test.com')).toBeDisabled();
  });

  it('lets a holder re-seat a custom holder ranked admin whose grant they cover', async () => {
    await renderAs(steward(ADMIN_GRANT), [
      {
        ...mockViewer,
        role: 'admin',
        custom_role: { id: 'role-full', name: 'Everything' },
        permissions: ADMIN_GRANT,
      },
    ]);

    expect(optionsOf('viewer@test.com')).toContain('viewer');
    expect(removeButtonFor('viewer@test.com')).toBeEnabled();
  });

  it('lets a built-in workspace admin re-seat a custom holder the API ranks admin', async () => {
    await renderAs({ ...mockAdmin, user_id: 'user-1', email: 'self@test.com' }, [
      {
        ...mockViewer,
        role: 'admin',
        custom_role: { id: 'role-full', name: 'Everything' },
        permissions: ADMIN_GRANT,
      },
    ]);

    expect(roleSelectFor('viewer@test.com')).toHaveTextContent('Everything');
    expect(removeButtonFor('viewer@test.com')).toBeEnabled();
  });

  it('gives a holder without the workspace members permissions no control', async () => {
    await renderAs(steward([Permission.WorkspaceRead, Permission.WorkspaceWrite]), [plainViewer]);

    expect(screen.queryByRole('button', { name: /Add Member/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('columnheader', { name: 'Actions' })).not.toBeInTheDocument();
    expect(roleBadgeFor('viewer@test.com')).toHaveTextContent('Viewer');
  });
});
