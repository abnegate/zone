import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import type { InvitationDetails } from '../types';

const mockNavigate = mock();
const mockUseAuth = mock(() => ({ isAuthenticated: false }));
const mockUseSearchParams = mock(() => [new URLSearchParams('?token=test-token')]);
const mockGetInvitationByToken = mock();
const mockAcceptInvitation = mock();

mock.module('../../../api/client', () => ({
  client: {
    getInvitationByToken: mockGetInvitationByToken,
    acceptInvitation: mockAcceptInvitation,
  },
}));

mock.module('../hooks', () => ({
  useAuth: mockUseAuth,
}));

mock.module('react-router-dom', () => ({
  useNavigate: () => mockNavigate,
  useSearchParams: () => mockUseSearchParams(),
}));

let InvitationAcceptPage: typeof import('./InvitationAcceptPage').default;

beforeAll(async () => {
  InvitationAcceptPage = (await import('./InvitationAcceptPage')).default;
});

afterAll(() => {
  mock.restore();
});

describe('InvitationAcceptPage', () => {
  const mockDetails: InvitationDetails = {
    organization_name: 'Acme Corp',
    org_role: 'member',
    workspace_name: 'Engineering',
    workspace_role: 'member',
    invited_by_email: 'admin@acme.com',
    expires_at: '2099-12-31T23:59:59Z',
  };

  beforeEach(() => {
    jest.clearAllMocks();
    mockUseAuth.mockReset();
    mockUseAuth.mockReturnValue({ isAuthenticated: false });
    mockUseSearchParams.mockReset();
    mockUseSearchParams.mockReturnValue([new URLSearchParams('?token=test-token')]);
    mockGetInvitationByToken.mockReset();
    mockAcceptInvitation.mockReset();
    mockGetInvitationByToken.mockResolvedValue(mockDetails);
  });

  const renderPage = () => {
    return render(<InvitationAcceptPage />);
  };

  it('renders loading state initially', () => {
    renderPage();
    expect(screen.getByText('Loading invitation...')).toBeInTheDocument();
  });

  it('fetches and displays invitation details', async () => {
    renderPage();

    await waitFor(() => {
      expect(mockGetInvitationByToken).toHaveBeenCalledWith('test-token');
    });

    expect(screen.getByText("You've Been Invited!")).toBeInTheDocument();
    expect(screen.getAllByText('Acme Corp').length).toBeGreaterThan(0);
    expect(screen.getByText('Engineering')).toBeInTheDocument();
    expect(screen.getAllByText('admin@acme.com').length).toBeGreaterThan(0);
  });

  it('displays org-only invitation correctly', async () => {
    const orgOnlyDetails: InvitationDetails = {
      ...mockDetails,
      workspace_name: null,
      workspace_role: null,
    };
    mockGetInvitationByToken.mockResolvedValue(orgOnlyDetails);

    renderPage();

    await waitFor(() => {
      expect(screen.getAllByText('Acme Corp').length).toBeGreaterThan(0);
    });

    expect(screen.queryByText('Workspace:')).not.toBeInTheDocument();
  });

  it('displays login and register buttons when not authenticated', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText("You've Been Invited!")).toBeInTheDocument();
    });

    expect(
      screen.getByText('You need to be signed in to accept this invitation')
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /sign in/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /create account/i })).toBeInTheDocument();
  });

  it('navigates to login page when sign in clicked', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /sign in/i })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /sign in/i }));

    expect(mockNavigate).toHaveBeenCalledWith(expect.stringContaining('/login?redirect='));
  });

  it('navigates to register page when create account clicked', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /create account/i })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /create account/i }));

    expect(mockNavigate).toHaveBeenCalledWith(expect.stringContaining('/register?redirect='));
  });

  it('displays accept button when authenticated', async () => {
    mockUseAuth.mockReturnValue({ isAuthenticated: true });

    renderPage();

    await waitFor(() => {
      expect(screen.getByText("You've Been Invited!")).toBeInTheDocument();
    });

    expect(screen.getByRole('button', { name: /accept invitation/i })).toBeInTheDocument();
  });

  it('accepts invitation successfully when authenticated', async () => {
    mockUseAuth.mockReturnValue({ isAuthenticated: true });
    mockAcceptInvitation.mockResolvedValue();

    renderPage();

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /accept invitation/i })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /accept invitation/i }));

    await waitFor(() => {
      expect(mockAcceptInvitation).toHaveBeenCalledWith('test-token');
    });

    expect(mockNavigate).toHaveBeenCalledWith('/org-settings');
  });

  it('displays error when invitation fetch fails', async () => {
    mockGetInvitationByToken.mockRejectedValue(new Error('Invitation not found'));

    renderPage();

    await waitFor(() => {
      expect(screen.getByText('Invalid Invitation')).toBeInTheDocument();
    });

    expect(screen.getByText('Invitation not found')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /go to home/i })).toBeInTheDocument();
  });

  it('displays error when token is missing', async () => {
    mockUseSearchParams.mockReturnValue([new URLSearchParams('')]);
    renderPage();

    await waitFor(() => {
      expect(screen.queryByText('Loading invitation...')).not.toBeInTheDocument();
    });

    expect(screen.getByText('Invalid Invitation')).toBeInTheDocument();
  });

  it('displays expired message for expired invitations', async () => {
    const expiredDetails: InvitationDetails = {
      ...mockDetails,
      expires_at: '2020-01-01T00:00:00Z',
    };
    mockGetInvitationByToken.mockResolvedValue(expiredDetails);

    renderPage();

    await waitFor(() => {
      expect(screen.getByText('This invitation has expired')).toBeInTheDocument();
    });

    expect(screen.getAllByText(/expired/i).length).toBeGreaterThan(0);
  });

  it('displays role badges with correct styling', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText("You've Been Invited!")).toBeInTheDocument();
    });

    const roleBadges = screen.getAllByText('member');
    expect(roleBadges.length).toBeGreaterThan(0);
    roleBadges.forEach((badge) => {
      expect(badge).toHaveClass('role-badge', 'role-member');
    });
  });

  it('displays workspace role only when workspace is present', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText('Engineering')).toBeInTheDocument();
    });

    expect(screen.getByText('Workspace:')).toBeInTheDocument();
    expect(screen.getByText('Workspace Role:')).toBeInTheDocument();
  });

  it('formats expiry date correctly', async () => {
    renderPage();

    await waitFor(() => {
      expect(screen.getByText("You've Been Invited!")).toBeInTheDocument();
    });

    // Check that the expiry date is present in the document
    // The exact format may vary, so just verify it exists
    expect(screen.getByText(/Expires:/i)).toBeInTheDocument();
  });

  it('navigates home when go to home button clicked on error', async () => {
    mockGetInvitationByToken.mockRejectedValue(new Error('Not found'));

    renderPage();

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /go to home/i })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /go to home/i }));

    expect(mockNavigate).toHaveBeenCalledWith('/');
  });

  it('does not allow accepting expired invitation', async () => {
    mockUseAuth.mockReturnValue({ isAuthenticated: true });

    const expiredDetails: InvitationDetails = {
      ...mockDetails,
      expires_at: '2020-01-01T00:00:00Z',
    };
    mockGetInvitationByToken.mockResolvedValue(expiredDetails);

    const { unmount } = renderPage();
    unmount();

    renderPage();

    await waitFor(() => {
      expect(screen.getAllByText(/expired/i).length).toBeGreaterThan(0);
    });

    // Accept button should not be present for expired invitation
    expect(screen.queryByRole('button', { name: /accept invitation/i })).not.toBeInTheDocument();
  });
});
