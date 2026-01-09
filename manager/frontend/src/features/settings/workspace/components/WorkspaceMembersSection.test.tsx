import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { client } from '../../../../api/client';
import type { OrganizationMember, WorkspaceMember } from '../types';
import WorkspaceMembersSection from './WorkspaceMembersSection';

// Mock client
jest.mock('../../../../api/client', () => ({
  client: {
    getWorkspaceMembers: jest.fn(),
    getOrgMembers: jest.fn(),
    addWorkspaceMember: jest.fn(),
    updateWorkspaceMemberRole: jest.fn(),
    removeWorkspaceMember: jest.fn(),
  },
}));

// Mock useAuth
jest.mock('../../../auth', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: 'user-1', email: 'owner@test.com' },
  }),
}));

const mockClient = client as jest.Mocked<typeof client>;

const mockOwner: WorkspaceMember = {
  id: 'ws-member-1',
  user_id: 'user-1',
  workspace_id: 'ws-123',
  role: 'owner',
  email: 'owner@test.com',
  display_name: 'Test Owner',
  joined_at: '2024-01-01T00:00:00Z',
};

const mockAdmin: WorkspaceMember = {
  id: 'ws-member-2',
  user_id: 'user-2',
  workspace_id: 'ws-123',
  role: 'admin',
  email: 'admin@test.com',
  display_name: 'Test Admin',
  joined_at: '2024-01-02T00:00:00Z',
};

const mockMember: WorkspaceMember = {
  id: 'ws-member-3',
  user_id: 'user-3',
  workspace_id: 'ws-123',
  role: 'member',
  email: 'member@test.com',
  display_name: null,
  joined_at: '2024-01-03T00:00:00Z',
};

const mockViewer: WorkspaceMember = {
  id: 'ws-member-4',
  user_id: 'user-4',
  workspace_id: 'ws-123',
  role: 'viewer',
  email: 'viewer@test.com',
  display_name: 'Test Viewer',
  joined_at: '2024-01-04T00:00:00Z',
};

const mockOrgMember1: OrganizationMember = {
  id: 'org-member-1',
  user_id: 'user-5',
  organization_id: 'org-123',
  role: 'member',
  email: 'orgmember@test.com',
  display_name: 'Org Member',
  joined_at: '2024-01-05T00:00:00Z',
};

describe('WorkspaceMembersSection', () => {
  beforeEach(() => {
    jest.clearAllMocks();
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

    it('displays role badges with correct colors', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const badges = document.querySelectorAll('.role-badge');
        expect(badges.length).toBeGreaterThan(0);
        expect(document.querySelector('.role-badge-owner')).toBeInTheDocument();
        expect(document.querySelector('.role-badge-admin')).toBeInTheDocument();
        expect(document.querySelector('.role-badge-member')).toBeInTheDocument();
        expect(document.querySelector('.role-badge-viewer')).toBeInTheDocument();
      });
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

    it.skip('adds member successfully and refreshes list', async () => {
      const newMember: WorkspaceMember = {
        id: 'ws-member-5',
        user_id: 'user-5',
        workspace_id: 'ws-123',
        role: 'member',
        email: 'orgmember@test.com',
        display_name: 'Org Member',
        joined_at: '2024-01-05T00:00:00Z',
      };

      mockClient.addWorkspaceMember.mockResolvedValueOnce(newMember);
      mockClient.getWorkspaceMembers.mockResolvedValueOnce({
        members: [mockOwner, mockAdmin, mockMember, mockViewer, newMember],
      });

      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        expect(screen.getByRole('button', { name: /Add Member/i })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: /Add Member/i }));

      await waitFor(() => {
        expect(screen.getByText('Add Workspace Member')).toBeInTheDocument();
      });

      // Wait for the select to be populated
      await waitFor(() => {
        const userSelect = screen.getByLabelText(/User/i) as HTMLSelectElement;
        // Check that we have options (org member should be available)
        expect(userSelect.options.length).toBeGreaterThan(1);
      });

      const userSelect = screen.getByLabelText(/User/i) as HTMLSelectElement;
      fireEvent.change(userSelect, { target: { value: 'user-5' } });

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
        fireEvent.change(memberRoleSelect!, { target: { value: 'admin' } });
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
        fireEvent.change(memberRoleSelect!, { target: { value: 'owner' } });
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
        fireEvent.change(adminRoleSelect!, { target: { value: 'viewer' } });
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
        fireEvent.change(memberRoleSelect!, { target: { value: 'admin' } });
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
      mockClient.removeWorkspaceMember.mockResolvedValueOnce();

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
        const roleSelects = screen.getAllByRole('combobox');
        expect(roleSelects[0]).toBeDisabled();
      });
    });

    it('prevents user from modifying themselves', async () => {
      render(<WorkspaceMembersSection workspaceId="ws-123" orgId="org-123" />);
      await waitFor(() => {
        const roleSelects = screen.getAllByRole('combobox');
        const ownerRoleSelect = roleSelects.find((select) => {
          const row = select.closest('tr');
          return row?.textContent?.includes('owner@test.com');
        });
        expect(ownerRoleSelect).toBeDisabled();
      });
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

      fireEvent.change(viewerRoleSelect!, { target: { value: 'member' } });

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
