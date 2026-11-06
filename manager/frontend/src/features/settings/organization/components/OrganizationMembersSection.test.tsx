import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import { clientModule } from '../../../../test/client';
import type { Workspace } from '../../workspace/types';

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
  InvitationsSection: ({ orgId, workspaces }: { orgId: string; workspaces: Workspace[] }) => (
    <div data-testid="invitations-section">
      {orgId}: {workspaces.map((workspace) => workspace.name).join(',')}
    </div>
  ),
}));

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

  it('renders members without invitations or a workspace fetch when invitations is false', async () => {
    render(<OrganizationMembersSection invitations={false} />);

    expect(screen.getByTestId('org-members-section')).toHaveTextContent(organizationId);
    expect(screen.queryByTestId('invitations-section')).not.toBeInTheDocument();
    await Promise.resolve();
    expect(mockGetWorkspaces).not.toHaveBeenCalled();
  });

  it('renders members and invitations when invitations is true', async () => {
    render(<OrganizationMembersSection invitations />);

    expect(screen.getByTestId('org-members-section')).toHaveTextContent(organizationId);
    await waitFor(() => {
      expect(screen.getByTestId('invitations-section')).toHaveTextContent(
        `${organizationId}: Active,Archived`
      );
    });
  });

  it('fetches workspaces with the organization id only, so inactive workspaces stay invitable', async () => {
    render(<OrganizationMembersSection invitations />);

    await waitFor(() => {
      expect(mockGetWorkspaces).toHaveBeenCalledTimes(1);
    });
    expect(mockGetWorkspaces.mock.calls[0]).toEqual([organizationId]);
  });

  it('keeps invitations usable and explains when the workspace fetch fails', async () => {
    mockGetWorkspaces.mockRejectedValue(new Error('Failed to fetch workspaces: 500'));

    render(<OrganizationMembersSection invitations />);

    expect(await screen.findByText('Couldn’t load workspaces')).toBeInTheDocument();
    expect(screen.getByTestId('invitations-section')).toHaveTextContent(`${organizationId}:`);
  });

  it('renders nothing without a current organization', async () => {
    workspaceContext.currentOrganization = null;

    const { container } = render(<OrganizationMembersSection invitations />);

    expect(container.innerHTML).toBe('');
    await Promise.resolve();
    expect(mockGetWorkspaces).not.toHaveBeenCalled();
  });
});
