import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import type { Invitation, Workspace } from '../types';

const mockGetInvitations = mock();
const mockCreateInvitation = mock();
const mockRevokeInvitation = mock();

mock.module('../../../../api/client', () => ({
  client: {
    getInvitations: mockGetInvitations,
    createInvitation: mockCreateInvitation,
    revokeInvitation: mockRevokeInvitation,
  },
}));

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
    jest.clearAllMocks();
    mockGetInvitations.mockResolvedValue({ invitations: mockInvitations });
  });

  it('renders loading state initially', () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);
    expect(screen.getByText('Loading invitations...')).toBeInTheDocument();
  });

  it('fetches and displays invitations', async () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

    await waitFor(() => {
      expect(mockGetInvitations).toHaveBeenCalledWith(orgId);
    });

    expect(screen.getByText('invitee1@test.com')).toBeInTheDocument();
    expect(screen.getByText('invitee2@test.com')).toBeInTheDocument();
    expect(screen.getByText('Engineering')).toBeInTheDocument();
  });

  it('displays empty state when no invitations', async () => {
    mockGetInvitations.mockResolvedValue({ invitations: [] });

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

    await waitFor(() => {
      expect(screen.getByText('No pending invitations')).toBeInTheDocument();
    });

    expect(screen.getByText('Invite team members to join this organization')).toBeInTheDocument();
  });

  it('displays error message on fetch failure', async () => {
    mockGetInvitations.mockRejectedValue(new Error('Network error'));

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

    await waitFor(() => {
      expect(screen.getByText('Network error')).toBeInTheDocument();
    });
  });

  it('opens invitation modal when invite button clicked', async () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    const inviteButton = screen.getByRole('button', { name: /invite member/i });
    fireEvent.click(inviteButton);

    expect(screen.getByRole('heading', { name: /invite member/i })).toBeInTheDocument();
    expect(screen.getByLabelText(/email address/i)).toBeInTheDocument();
  });

  it('submits org-only invitation', async () => {
    mockCreateInvitation.mockResolvedValue(mockInvitations[1]);

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    // Open modal
    fireEvent.click(screen.getByRole('button', { name: /invite member/i }));

    // Fill form
    fireEvent.change(screen.getByLabelText(/email address/i), {
      target: { value: 'newuser@test.com' },
    });
    fireEvent.change(screen.getByLabelText(/organization role/i), {
      target: { value: 'admin' },
    });

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

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    // Open modal
    fireEvent.click(screen.getByRole('button', { name: /invite member/i }));

    // Fill form
    fireEvent.change(screen.getByLabelText(/email address/i), {
      target: { value: 'dev@test.com' },
    });
    fireEvent.change(screen.getByLabelText(/organization role/i), {
      target: { value: 'member' },
    });
    fireEvent.change(screen.getByLabelText(/workspace \(optional\)/i), {
      target: { value: 'ws-1' },
    });

    // Workspace role field should appear
    await waitFor(() => {
      expect(screen.getByLabelText(/workspace role/i)).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText(/workspace role/i), {
      target: { value: 'admin' },
    });

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

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

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

  // Note: Uses Jest syntax (jest.fn) for global.confirm mock
  it.skip('revokes invitation when revoke button clicked', async () => {
    mockRevokeInvitation.mockResolvedValue();
    global.confirm = jest.fn(() => true);

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

    await waitFor(() => {
      expect(screen.getByText('invitee1@test.com')).toBeInTheDocument();
    });

    const revokeButtons = screen.getAllByRole('button', { name: /revoke/i });
    fireEvent.click(revokeButtons[0]);

    expect(global.confirm).toHaveBeenCalledWith('Are you sure you want to revoke this invitation?');

    await waitFor(() => {
      expect(mockRevokeInvitation).toHaveBeenCalledWith(orgId, 'inv-1');
    });

    // Should reload invitations
    expect(mockGetInvitations).toHaveBeenCalledTimes(2);
  });

  // Note: Uses Jest syntax (jest.fn) for global.confirm mock
  it.skip('does not revoke invitation when user cancels confirmation', async () => {
    global.confirm = jest.fn(() => false);

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

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

    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

    await waitFor(() => {
      expect(screen.getByText(/expired/i)).toBeInTheDocument();
    });

    // Row should have expired class
    const row = screen.getByText('invitee1@test.com').closest('tr');
    expect(row).toHaveClass('expired');
  });

  it('closes modal when close button clicked', async () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    // Open modal
    fireEvent.click(screen.getByRole('button', { name: /invite member/i }));
    expect(screen.getByRole('heading', { name: /invite member/i })).toBeInTheDocument();

    // Close modal - find by class to distinguish from other elements
    const closeButton = document.querySelector('.modal-close') as HTMLElement;
    fireEvent.click(closeButton);
    expect(screen.queryByRole('heading', { name: /invite member/i })).not.toBeInTheDocument();
  });

  it('closes modal when cancel button clicked', async () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    // Open modal
    fireEvent.click(screen.getByRole('button', { name: /invite member/i }));
    expect(screen.getByRole('heading', { name: /invite member/i })).toBeInTheDocument();

    // Click cancel
    fireEvent.click(screen.getByRole('button', { name: /cancel/i }));
    expect(screen.queryByRole('heading', { name: /invite member/i })).not.toBeInTheDocument();
  });

  it('closes modal when clicking overlay', async () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

    await waitFor(() => {
      expect(screen.queryByText('Loading invitations...')).not.toBeInTheDocument();
    });

    // Open modal
    fireEvent.click(screen.getByRole('button', { name: /invite member/i }));
    expect(screen.getByRole('heading', { name: /invite member/i })).toBeInTheDocument();

    // Click overlay (parent of modal-content)
    const modal = screen.getByRole('heading', { name: /invite member/i }).closest('.modal-content');
    const overlay = modal?.parentElement;
    if (overlay) {
      fireEvent.click(overlay);
      expect(screen.queryByRole('heading', { name: /invite member/i })).not.toBeInTheDocument();
    }
  });

  it('displays role badges with correct styling', async () => {
    render(<InvitationsSection orgId={orgId} workspaces={mockWorkspaces} />);

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
