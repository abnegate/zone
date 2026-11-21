import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { AuthRequestError } from '../../../../api/auth';
import { clientModule } from '../../../../test/client';
import { chooseSelect, openSelect } from '../../../../test/select';
import { memberCapMessage } from '../../../enterprise/members';
import { PRICING_URL } from '../../../enterprise/pricing';
import { Permission } from '../../../enterprise/roles/schemas';
import type { Invitation, InvitationAccess, Workspace } from '../types';

const mockGetInvitations = mock();
const mockCreateInvitation = mock();
const mockRevokeInvitation = mock();

mock.module('../../../../api/client', () =>
  clientModule({
    getInvitations: mockGetInvitations,
    createInvitation: mockCreateInvitation,
    revokeInvitation: mockRevokeInvitation,
  })
);

const MANAGER: Omit<InvitationAccess, 'read'> = { create: true, revoke: true };

const ADMIN_PERMISSIONS = [
  Permission.OrganizationRead,
  Permission.OrganizationUpdate,
  Permission.MembersCreate,
  Permission.MembersUpdate,
  Permission.MembersDelete,
  Permission.InvitationsCreate,
  Permission.InvitationsRead,
  Permission.InvitationsDelete,
  Permission.RolesUpdate,
];

let InvitationsSection: typeof import('./InvitationsSection').InvitationsSection;

beforeAll(async () => {
  ({ InvitationsSection } = await import('./InvitationsSection'));
});

afterAll(() => {
  mock.restore();
});

describe('InvitationsSection', () => {
  const orgId = 'org-123';
  const mockWorkspaces: Workspace[] = [
    {
      id: 'ws-1',
      organization_id: orgId,
      name: 'Engineering',
      slug: 'engineering',
      description: null,
      is_active: true,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
    {
      id: 'ws-2',
      organization_id: orgId,
      name: 'Sales',
      slug: 'sales',
      description: null,
      is_active: true,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    },
  ];

  const mockInvitations: Invitation[] = [
    {
      id: 'inv-1',
      organization_id: orgId,
      organization_name: 'Test Org',
      email: 'invitee1@test.com',
      org_role: 'member',
      workspace_id: 'ws-1',
      workspace_name: 'Engineering',
      workspace_role: 'member',
      invited_by_email: 'admin@test.com',
      created_at: '2024-01-01T00:00:00Z',
      expires_at: '2024-12-31T23:59:59Z',
    },
    {
      id: 'inv-2',
      organization_id: orgId,
      organization_name: 'Test Org',
      email: 'invitee2@test.com',
      org_role: 'admin',
      workspace_id: null,
      workspace_name: null,
      workspace_role: null,
      invited_by_email: 'owner@test.com',
      created_at: '2024-01-02T00:00:00Z',
      expires_at: '2024-12-31T23:59:59Z',
    },
  ];

  beforeEach(() => {
    mock.clearAllMocks();
    mockGetInvitations.mockResolvedValue({ invitations: mockInvitations });
  });

  it('renders loading state initially', () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);
    expect(screen.getByText('Loading invitations...')).toBeInTheDocument();
  });

  it('fetches and displays invitations', async () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    expect(await screen.findByText('invitee1@test.com')).toBeInTheDocument();
    expect(mockGetInvitations).toHaveBeenCalledWith(orgId);
    expect(screen.getByText('invitee2@test.com')).toBeInTheDocument();
    expect(screen.getByText('Engineering')).toBeInTheDocument();
  });

  it('displays empty state when no invitations', async () => {
    mockGetInvitations.mockResolvedValue({ invitations: [] });

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.getByText('No pending invitations')).toBeInTheDocument();
    });

    expect(screen.getByText('Invite team members to join this organization')).toBeInTheDocument();
  });

  it('displays error message on fetch failure', async () => {
    mockGetInvitations.mockRejectedValue(new Error('Network error'));

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.getByText('Network error')).toBeInTheDocument();
    });
  });

  it('opens invitation modal when invite button clicked', async () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    const inviteButton = screen.getByRole('button', { name: /invite member/i });
    fireEvent.click(inviteButton);

    expect(screen.getByRole('heading', { name: /invite member/i })).toBeInTheDocument();
    expect(screen.getByLabelText(/email address/i)).toBeInTheDocument();
  });

  it('offers no invitation to someone who can read invitations but not send them', async () => {
    render(
      <InvitationsSection
        orgId={orgId}
        workspaces={mockWorkspaces}
        access={{ create: false, revoke: true }}
      />
    );

    expect(await screen.findByText('invitee1@test.com')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /invite member/i })).not.toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: 'Revoke' }).length).toBeGreaterThan(0);
  });

  it('offers no empty-state invitation to someone who cannot send one', async () => {
    mockGetInvitations.mockResolvedValue({ invitations: [] });

    render(
      <InvitationsSection
        orgId={orgId}
        workspaces={mockWorkspaces}
        access={{ create: false, revoke: false }}
      />
    );

    expect(await screen.findByText('No pending invitations')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /send an invitation/i })).not.toBeInTheDocument();
  });

  it('offers no revoke to someone who can read invitations but not delete them', async () => {
    render(
      <InvitationsSection
        orgId={orgId}
        workspaces={mockWorkspaces}
        access={{ create: true, revoke: false }}
      />
    );

    expect(await screen.findByText('invitee1@test.com')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Revoke' })).not.toBeInTheDocument();
    expect(screen.queryByRole('columnheader', { name: 'Actions' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: /invite member/i })).toBeInTheDocument();
  });

  it('submits org-only invitation', async () => {
    mockCreateInvitation.mockResolvedValue(mockInvitations[1]);

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    // Open modal
    fireEvent.click(screen.getByRole('button', { name: /invite member/i }));

    // Fill form
    fireEvent.change(screen.getByLabelText(/email address/i), {
      target: { value: 'newuser@test.com' },
    });
    chooseSelect(/organization role/i, 'Admin');

    // Submit
    fireEvent.submit(screen.getByRole('button', { name: /send invitation/i }).closest('form')!);

    await waitFor(() => {
      expect(mockCreateInvitation).toHaveBeenCalledWith(orgId, {
        email: 'newuser@test.com',
        org_role: 'admin',
      });
    });

    // Modal should close and invitations should reload
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: /invite member/i })).not.toBeInTheDocument();
    });
    expect(mockGetInvitations).toHaveBeenCalledTimes(2);
  });

  it('submits invitation with workspace', async () => {
    mockCreateInvitation.mockResolvedValue(mockInvitations[0]);

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    // Open modal
    fireEvent.click(screen.getByRole('button', { name: /invite member/i }));

    // Fill form
    fireEvent.change(screen.getByLabelText(/email address/i), {
      target: { value: 'dev@test.com' },
    });
    chooseSelect(/organization role/i, 'Member');
    chooseSelect(/^workspace\s+optional$/i, 'Engineering');

    // Workspace role field should appear
    await waitFor(() => {
      expect(screen.getByLabelText(/workspace role/i)).toBeInTheDocument();
    });

    chooseSelect(/workspace role/i, 'Admin');

    // Submit
    fireEvent.submit(screen.getByRole('button', { name: /send invitation/i }).closest('form')!);

    await waitFor(() => {
      expect(mockCreateInvitation).toHaveBeenCalledWith(orgId, {
        email: 'dev@test.com',
        org_role: 'member',
        workspace_id: 'ws-1',
        workspace_role: 'admin',
      });
    });
  });

  it('displays error when invitation creation fails', async () => {
    mockCreateInvitation.mockRejectedValue(new Error('Email already invited'));

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    // Open modal and submit
    fireEvent.click(screen.getByRole('button', { name: /invite member/i }));
    fireEvent.change(screen.getByLabelText(/email address/i), {
      target: { value: 'existing@test.com' },
    });
    fireEvent.submit(screen.getByRole('button', { name: /send invitation/i }).closest('form')!);

    await waitFor(() => {
      expect(screen.getByText('Email already invited')).toBeInTheDocument();
    });

    // Modal should stay open
    expect(screen.getByRole('heading', { name: /invite member/i })).toBeInTheDocument();
  });

  async function invite() {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);
    fireEvent.click(await screen.findByRole('button', { name: /invite member/i }));
    fireEvent.change(screen.getByLabelText(/email address/i), {
      target: { value: 'fourth@test.com' },
    });
    fireEvent.submit(screen.getByRole('button', { name: /send invitation/i }).closest('form')!);
  }

  it('explains the member cap inside the invite form, with a link to the plans', async () => {
    mockCreateInvitation.mockRejectedValue(
      new AuthRequestError('Server text', 'member_cap_reached')
    );

    await invite();

    const dialog = screen.getByRole('dialog', { name: /invite member/i });
    const alert = await within(dialog).findByRole('alert');
    expect(alert).toHaveTextContent(memberCapMessage());
    expect(alert).not.toHaveTextContent('Server text');
    expect(within(alert).getByRole('link', { name: /Compare plans/ })).toHaveAttribute(
      'href',
      PRICING_URL
    );
    expect(screen.queryByText('Server text')).not.toBeInTheDocument();
  });

  it('clears the member cap explanation when the form is closed and opened again', async () => {
    mockCreateInvitation.mockRejectedValue(
      new AuthRequestError('Server text', 'member_cap_reached')
    );
    await invite();
    await screen.findByText(memberCapMessage(), { exact: false });

    fireEvent.click(screen.getByRole('button', { name: /cancel/i }));
    fireEvent.click(screen.getByRole('button', { name: /invite member/i }));

    expect(screen.queryByText(memberCapMessage(), { exact: false })).not.toBeInTheDocument();
  });

  it('shows the plain message for a refusal that is not the cap', async () => {
    mockCreateInvitation.mockRejectedValue(new AuthRequestError('Not allowed', 'forbidden'));

    await invite();

    expect(await screen.findByText('Not allowed')).toBeInTheDocument();
    expect(screen.queryByText(memberCapMessage(), { exact: false })).not.toBeInTheDocument();
  });

  it('revokes invitation when revoke button clicked', async () => {
    mockRevokeInvitation.mockResolvedValue(undefined);
    window.confirm = mock(() => true) as typeof window.confirm;

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.getByText('invitee1@test.com')).toBeInTheDocument();
    });

    const revokeButtons = screen.getAllByRole('button', { name: /revoke/i });
    fireEvent.click(revokeButtons[0]);

    expect(window.confirm).toHaveBeenCalledWith('Are you sure you want to revoke this invitation?');

    await waitFor(() => {
      expect(mockRevokeInvitation).toHaveBeenCalledWith(orgId, 'inv-1');
    });

    // Should reload invitations
    expect(mockGetInvitations).toHaveBeenCalledTimes(2);
  });

  it('does not revoke invitation when user cancels confirmation', async () => {
    window.confirm = mock(() => false) as typeof window.confirm;

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.getByText('invitee1@test.com')).toBeInTheDocument();
    });

    const revokeButtons = screen.getAllByRole('button', { name: /revoke/i });
    fireEvent.click(revokeButtons[0]);

    expect(mockRevokeInvitation).not.toHaveBeenCalled();
  });

  it('displays expired invitation with visual indicator', async () => {
    const expiredInvitation: Invitation = {
      ...mockInvitations[0],
      expires_at: '2023-01-01T00:00:00Z',
    };

    mockGetInvitations.mockResolvedValue({ invitations: [expiredInvitation] });

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.getByText(/expired/i)).toBeInTheDocument();
    });

    // Row should have expired class
    const row = screen.getByText('invitee1@test.com').closest('tr');
    expect(row).toHaveClass('expired');
  });

  it('closes modal when close button clicked', async () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    // Open modal
    fireEvent.click(screen.getByRole('button', { name: /invite member/i }));
    expect(screen.getByRole('heading', { name: /invite member/i })).toBeInTheDocument();

    expect(screen.getByRole('dialog', { name: 'Invite Member' })).toHaveClass('ui-dialog-content');
    expect(document.querySelector('.modal-content')).toBeNull();
    const closeButton = document.querySelector('.ui-dialog-close') as HTMLElement;
    expect(closeButton.querySelector('svg')).not.toBeNull();
    fireEvent.click(closeButton);
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: /invite member/i })).not.toBeInTheDocument();
    });
  });

  it('closes modal when cancel button clicked', async () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    // Open modal
    fireEvent.click(screen.getByRole('button', { name: /invite member/i }));
    expect(screen.getByRole('heading', { name: /invite member/i })).toBeInTheDocument();

    // Click cancel
    fireEvent.click(screen.getByRole('button', { name: /cancel/i }));
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: /invite member/i })).not.toBeInTheDocument();
    });
  });

  it('closes modal on Escape', async () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    // Open modal
    fireEvent.click(screen.getByRole('button', { name: /invite member/i }));
    expect(screen.getByRole('heading', { name: /invite member/i })).toBeInTheDocument();

    fireEvent.keyDown(document, { key: 'Escape' });
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: /invite member/i })).not.toBeInTheDocument();
    });
  });

  describe('roles on offer', () => {
    const openForm = async (access: Omit<InvitationAccess, 'read'>) => {
      render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={access} />);
      fireEvent.click(await screen.findByRole('button', { name: /invite member/i }));
      chooseSelect(/^workspace\s+optional$/i, 'Engineering');
      await screen.findByRole('combobox', { name: /workspace role/i });
    };

    const labelsOf = (label: RegExp): string[] => {
      const trigger = openSelect(label);
      const labels = screen.getAllByRole('option').map((option) => option.textContent ?? '');
      fireEvent.keyDown(trigger, { key: 'Escape' });
      return labels;
    };

    it('offers a custom holder of invitations:create only the roles their grant covers', async () => {
      await openForm({
        create: true,
        revoke: false,
        role: 'member',
        permissions: [
          Permission.OrganizationRead,
          Permission.InvitationsRead,
          Permission.InvitationsCreate,
        ],
      });

      expect(labelsOf(/organization role/i)).toEqual(['Member']);
      expect(labelsOf(/workspace role/i)).toEqual(['Viewer', 'Member']);
    });

    it('offers an admin the admin role but no owner and no workspace administrator', async () => {
      await openForm({
        create: true,
        revoke: true,
        role: 'admin',
        permissions: ADMIN_PERMISSIONS,
      });

      expect(labelsOf(/organization role/i)).toEqual(['Member', 'Admin']);
      expect(labelsOf(/workspace role/i)).toEqual(['Viewer', 'Member']);
    });

    it('offers an owner every role', async () => {
      await openForm({
        create: true,
        revoke: true,
        role: 'owner',
        permissions: [...ADMIN_PERMISSIONS, Permission.AdministratorsUpdate],
      });

      expect(labelsOf(/organization role/i)).toEqual(['Member', 'Admin', 'Owner']);
      expect(labelsOf(/workspace role/i)).toEqual(['Viewer', 'Member', 'Admin', 'Owner']);
    });

    it('falls back to the rank when the permissions are unknown', async () => {
      await openForm({ create: true, revoke: true, role: 'admin', permissions: undefined });

      expect(labelsOf(/organization role/i)).toEqual(['Member', 'Admin']);
      expect(labelsOf(/workspace role/i)).toEqual(['Viewer', 'Member']);
    });
  });

  it('announces a failure as an alert', async () => {
    mockGetInvitations.mockRejectedValue(new Error('Network error'));

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    expect(await screen.findByRole('alert')).toHaveTextContent('Network error');
  });

  it('displays role badges with correct styling', async () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} access={MANAGER} />);

    await waitFor(() => {
      expect(screen.getByText('invitee1@test.com')).toBeInTheDocument();
    });

    // Check for role badges
    const memberBadges = screen.getAllByText('member');
    expect(memberBadges.length).toBeGreaterThan(0);

    const adminBadges = screen.getAllByText('admin');
    expect(adminBadges.length).toBeGreaterThan(0);

    // Check they have the correct classes
    expect(memberBadges[0]).toHaveClass('role-badge', 'role-member');
    expect(adminBadges[0]).toHaveClass('role-badge', 'role-admin');
  });
});
