import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import { clientModule } from '../../../../test/client';
import type { Workspace } from '../../workspace/types';
import type { InvitationAccess } from '../types';

const mockGetWorkspaces = mock();

mock.module('../../../../api/client', () => clientModule({ getWorkspaces: mockGetWorkspaces }));

const organizationId = '00000000-0000-0000-0000-000000000001';

const workspaceContext: { currentOrganization: { id: string } | null } = {
  currentOrganization: { id: organizationId },
};

mock.module('../../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => workspaceContext,
}));

mock.module('./OrgMembersSection', () => ({
  default: ({ orgId }: { orgId: string }) => <div data-testid="org-members-section">{orgId}</div>,
}));

mock.module('./InvitationsSection', () => ({
  InvitationsSection: ({
    orgId,
    workspaces,
    access,
  }: {
    orgId: string;
    workspaces: Workspace[];
    access: Pick<InvitationAccess, 'create' | 'revoke'>;
  }) => (
    <div data-testid="invitations-section" data-access={JSON.stringify(access)}>
      {orgId}: {workspaces.map((workspace) => workspace.name).join(',')}
    </div>
  ),
}));

const HIDDEN: InvitationAccess = { read: false, create: false, revoke: false };

const MANAGER: InvitationAccess = { read: true, create: true, revoke: true };

let OrganizationMembersSection: typeof import('./OrganizationMembersSection').OrganizationMembersSection;

beforeAll(async () => {
  ({ OrganizationMembersSection } = await import('./OrganizationMembersSection'));
});

afterAll(() => {
  mock.restore();
});

const workspace = (name: string, active: boolean): Workspace => ({
  id: `ws-${name}`,
  organization_id: organizationId,
  name,
  slug: name.toLowerCase(),
  description: null,
  is_active: active,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
});

describe('OrganizationMembersSection', () => {
  beforeEach(() => {
    mockGetWorkspaces.mockReset();
    mockGetWorkspaces.mockResolvedValue([workspace('Active', true), workspace('Archived', false)]);
    workspaceContext.currentOrganization = { id: organizationId };
  });

  it('renders members without invitations or a workspace fetch without the invitations read permission', async () => {
    render(<OrganizationMembersSection invitations={HIDDEN} />);

    expect(screen.getByTestId('org-members-section')).toHaveTextContent(organizationId);
    expect(screen.queryByTestId('invitations-section')).not.toBeInTheDocument();
    await Promise.resolve();
    expect(mockGetWorkspaces).not.toHaveBeenCalled();
  });

  it('renders members and invitations with the invitations read permission', async () => {
    render(<OrganizationMembersSection invitations={MANAGER} />);

    expect(screen.getByTestId('org-members-section')).toHaveTextContent(organizationId);
    await waitFor(() => {
      expect(screen.getByTestId('invitations-section')).toHaveTextContent(
        `${organizationId}: Active,Archived`
      );
    });
  });

  it('fetches workspaces with the organization id only, so inactive workspaces stay invitable', async () => {
    render(<OrganizationMembersSection invitations={MANAGER} />);

    await waitFor(() => {
      expect(mockGetWorkspaces).toHaveBeenCalledTimes(1);
    });
    expect(mockGetWorkspaces.mock.calls[0]).toEqual([organizationId]);
  });

  it('keeps invitations usable and explains when the workspace fetch fails', async () => {
    mockGetWorkspaces.mockRejectedValue(new Error('Failed to fetch workspaces: 500'));

    render(<OrganizationMembersSection invitations={MANAGER} />);

    expect(await screen.findByText('Couldn’t load workspaces')).toBeInTheDocument();
    expect(screen.getByTestId('invitations-section')).toHaveTextContent(`${organizationId}:`);
  });

  it('hands the invitation controls the caller may use to the invitations', async () => {
    render(
      <OrganizationMembersSection invitations={{ read: true, create: false, revoke: true }} />
    );

    await waitFor(() => {
      expect(screen.getByTestId('invitations-section')).toHaveTextContent('Active');
    });
    expect(
      JSON.parse(screen.getByTestId('invitations-section').getAttribute('data-access') ?? '')
    ).toEqual({ read: true, create: false, revoke: true });
  });

  it('renders nothing without a current organization', async () => {
    workspaceContext.currentOrganization = null;

    const { container } = render(<OrganizationMembersSection invitations={MANAGER} />);

    expect(container.innerHTML).toBe('');
    await Promise.resolve();
    expect(mockGetWorkspaces).not.toHaveBeenCalled();
  });
});
