import { afterAll, beforeAll, beforeEach, describe, expect, it, mock, vi } from 'bun:test';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { type ComponentProps, useCallback, useState } from 'react';
import fixture from '../../../../../../runner/zone_server/tests/fixtures/agents.json';
import type { AgentStatus } from './schemas';

const agentsApi = {
  list: mock(),
  get: mock(),
  start: mock(),
  submitCode: mock(),
  signOut: mock(),
};

mock.module('../../../api/agents', () => ({ agentsApi }));

let AgentSignIn: typeof import('./AgentSignIn').AgentSignIn;
let POLL_INTERVAL: number;

beforeAll(async () => {
  ({ AgentSignIn, POLL_INTERVAL } = await import('./AgentSignIn'));
});

afterAll(() => {
  mock.restore();
});

const organization = '00000000-0000-0000-0000-000000000001';
const [claudeSignedIn, codexPending] = fixture.agents as AgentStatus[];
const claudeSignedOut: AgentStatus = {
  ...claudeSignedIn,
  state: 'signed_out',
  source: null,
  label: null,
  expires_at: null,
};
const codexSignedOut: AgentStatus = { ...codexPending, state: 'signed_out', pending: null };
const codexSignedIn: AgentStatus = {
  ...codexPending,
  state: 'signed_in',
  source: 'zone',
  label: 'ChatGPT Plus',
  pending: null,
};
const authorize =
  'https://claude.com/cai/oauth/authorize?code=true&client_id=fake-client&response_type=code&scope=user%3Ainference&state=fake-state';
const fullAuthorize =
  'https://claude.com/cai/oauth/authorize?code=true&client_id=fake-client&response_type=code&scope=org%3Acreate_api_key+user%3Ainference&state=fake-state-2';
const prompt = {
  agent: 'codex',
  verification_url: 'https://auth.openai.com/codex/device',
  user_code: 'ABCD-EFGHI',
  expires_at: '2026-09-23T04:15:00Z',
};
const refusal =
  'Error logging in with device code: device code request failed with status 403 Forbidden';

type Props = ComponentProps<typeof import('./AgentSignIn').AgentSignIn>;

function Harness({
  initial,
  onChange,
  ...props
}: Omit<Props, 'status' | 'onStatusChange' | 'organizationId' | 'loadError'> & {
  initial: AgentStatus | undefined;
  onChange: (status: AgentStatus) => void;
}) {
  const [status, setStatus] = useState(initial);
  const report = useCallback(
    (next: AgentStatus) => {
      onChange(next);
      setStatus(next);
    },
    [onChange]
  );
  return (
    <AgentSignIn
      {...props}
      organizationId={organization}
      loadError={null}
      status={status}
      onStatusChange={report}
    />
  );
}

function renderPanel(
  agent: Props['agent'],
  initial: AgentStatus | undefined,
  canManage = true
): { onChange: ReturnType<typeof mock> } {
  const onChange = mock();
  render(<Harness agent={agent} canManage={canManage} initial={initial} onChange={onChange} />);
  return { onChange };
}

describe('AgentSignIn', () => {
  beforeEach(() => {
    for (const request of Object.values(agentsApi)) request.mockReset();
  });

  describe('claude', () => {
    it('links to claude.com and signs in with a pasted callback address', async () => {
      agentsApi.start.mockResolvedValue({
        agent: 'claude',
        authorize_url: authorize,
        expires_at: '2026-09-23T04:10:00Z',
      });
      agentsApi.submitCode.mockResolvedValue(claudeSignedIn);
      const { onChange } = renderPanel('claude', claudeSignedOut);

      expect(screen.getByText('Not signed in')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));

      const link = await screen.findByRole('link', { name: 'Open claude.com' });
      expect(agentsApi.start).toHaveBeenCalledWith(organization, 'claude', undefined);
      expect(link).toHaveAttribute('href', authorize);
      expect(link).toHaveAttribute('target', '_blank');
      expect(link).toHaveAttribute('rel', 'noopener noreferrer');
      expect(screen.getByText('Signing in')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Submit code' })).toBeDisabled();

      const callback =
        'https://platform.claude.com/oauth/code/callback?code=fake-code&state=fake-state';
      fireEvent.change(screen.getByLabelText('Code from claude.com'), {
        target: { value: `  ${callback}\n` },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Submit code' }));

      await waitFor(() => expect(onChange).toHaveBeenCalledWith(claudeSignedIn));
      expect(agentsApi.submitCode).toHaveBeenCalledWith(organization, callback);
      expect(screen.getByText('Signed in')).toBeInTheDocument();
      expect(screen.queryByLabelText('Code from claude.com')).toBeNull();
      expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull();
    });

    it('submits a code#state pasted with Enter without saving the settings around it', async () => {
      agentsApi.start.mockResolvedValue({
        agent: 'claude',
        authorize_url: authorize,
        expires_at: '2026-09-23T04:10:00Z',
      });
      agentsApi.submitCode.mockResolvedValue(claudeSignedIn);
      const save = mock((event: Event) => event.preventDefault());
      render(
        <form onSubmit={save}>
          <AgentSignIn
            organizationId={organization}
            agent="claude"
            canManage
            status={claudeSignedOut}
            loadError={null}
            onStatusChange={mock()}
          />
          <button type="submit">Save Changes</button>
        </form>
      );

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      const field = await screen.findByLabelText('Code from claude.com');
      fireEvent.change(field, { target: { value: 'fake-code#fake-state' } });

      expect(fireEvent.keyDown(field, { key: 'Enter' })).toBe(false);
      await waitFor(() =>
        expect(agentsApi.submitCode).toHaveBeenCalledWith(organization, 'fake-code#fake-state')
      );
      expect(save).not.toHaveBeenCalled();
    });

    it('offers full access before a code is submitted', async () => {
      agentsApi.start
        .mockResolvedValueOnce({
          agent: 'claude',
          authorize_url: authorize,
          expires_at: '2026-09-23T04:10:00Z',
        })
        .mockResolvedValueOnce({
          agent: 'claude',
          authorize_url: fullAuthorize,
          expires_at: '2026-09-23T04:11:00Z',
        });
      renderPanel('claude', claudeSignedOut);

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      await screen.findByRole('link', { name: 'Open claude.com' });
      expect(screen.queryByText(/full access to your Claude account/)).toBeNull();

      fireEvent.click(screen.getByRole('button', { name: 'Try again with full access' }));

      await waitFor(() =>
        expect(screen.getByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
          'href',
          fullAuthorize
        )
      );
      expect(agentsApi.start).toHaveBeenLastCalledWith(organization, 'claude', 'full');
      expect(screen.getByText(/full access to your Claude account/)).toBeInTheDocument();
      expect(agentsApi.submitCode).not.toHaveBeenCalled();
    });

    it('offers full access after claude.com rejects the code for any reason', async () => {
      agentsApi.start
        .mockResolvedValueOnce({
          agent: 'claude',
          authorize_url: authorize,
          expires_at: '2026-09-23T04:10:00Z',
        })
        .mockResolvedValueOnce({
          agent: 'claude',
          authorize_url: fullAuthorize,
          expires_at: '2026-09-23T04:11:00Z',
        });
      agentsApi.submitCode.mockRejectedValue(
        new Error('Claude rejected the code: Invalid authorization code')
      );
      const { onChange } = renderPanel('claude', claudeSignedOut);

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      fireEvent.change(await screen.findByLabelText('Code from claude.com'), {
        target: { value: 'fake-code#fake-state' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Submit code' }));

      expect(await screen.findByRole('alert')).toHaveTextContent(
        'Claude rejected the code: Invalid authorization code'
      );
      expect(onChange).not.toHaveBeenCalled();

      fireEvent.click(screen.getByRole('button', { name: 'Try again with full access' }));

      await waitFor(() =>
        expect(agentsApi.start).toHaveBeenLastCalledWith(organization, 'claude', 'full')
      );
      await waitFor(() =>
        expect(screen.getByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
          'href',
          fullAuthorize
        )
      );
      expect(screen.queryByRole('alert')).toBeNull();
      expect(screen.getByLabelText('Code from claude.com')).toHaveValue('');
    });

    it('shows the plan and expiry of a sign-in this organization holds, and signs it out', async () => {
      const held: AgentStatus = { ...claudeSignedIn, expires_at: '2027-09-23T12:00:00Z' };
      agentsApi.signOut.mockResolvedValue(undefined);
      agentsApi.get.mockResolvedValue(claudeSignedOut);
      const { onChange } = renderPanel('claude', held);

      expect(screen.getByText('Signed in')).toBeInTheDocument();
      expect(screen.getByText('Claude Max · Expires Sep 23, 2027')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));

      await waitFor(() => expect(onChange).toHaveBeenCalledWith(claudeSignedOut));
      expect(agentsApi.signOut).toHaveBeenCalledWith(organization, 'claude');
      expect(agentsApi.get).toHaveBeenCalledWith(organization, 'claude');
      expect(await screen.findByRole('button', { name: 'Sign in with Claude' })).toBeEnabled();
    });

    it("names this server's own sign-in and offers an organization sign-in instead of sign-out", () => {
      renderPanel('claude', { ...claudeSignedIn, source: 'host', label: 'max', expires_at: null });

      expect(screen.getByText(/Using this server's own Claude Code sign-in/)).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Sign out' })).toBeNull();
      expect(screen.getByRole('button', { name: 'Sign in with Claude' })).toBeEnabled();
    });

    it('offers to sign in again when the sign-in expired', () => {
      renderPanel('claude', { ...claudeSignedIn, state: 'expired' });

      expect(screen.getByText('Sign-in expired')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Sign in with Claude' })).toBeEnabled();
      expect(screen.getByRole('button', { name: 'Sign out' })).toBeEnabled();
    });
  });

  describe('codex', () => {
    it('shows the link and code, polls every 3 s, and stops once signed in', async () => {
      vi.useFakeTimers();
      agentsApi.start.mockResolvedValue(prompt);
      agentsApi.get.mockResolvedValueOnce(codexPending).mockResolvedValueOnce(codexSignedIn);
      const { onChange } = renderPanel('codex', codexSignedOut);

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }));
      });

      expect(agentsApi.start).toHaveBeenCalledWith(organization, 'codex', undefined);
      expect(screen.getByText('Signing in')).toBeInTheDocument();
      expect(screen.getByText('ABCD-EFGHI')).toBeInTheDocument();
      const link = screen.getByRole('link', { name: 'auth.openai.com/codex/device' });
      expect(link).toHaveAttribute('href', 'https://auth.openai.com/codex/device');
      expect(link).toHaveAttribute('target', '_blank');
      expect(link).toHaveAttribute('rel', 'noopener noreferrer');

      expect(POLL_INTERVAL).toBe(3000);
      await act(async () => {
        vi.advanceTimersByTime(POLL_INTERVAL - 1);
      });
      expect(agentsApi.get).not.toHaveBeenCalled();

      await act(async () => {
        vi.advanceTimersByTime(1);
      });
      expect(agentsApi.get).toHaveBeenCalledTimes(1);
      expect(agentsApi.get).toHaveBeenCalledWith(organization, 'codex');
      expect(onChange).toHaveBeenLastCalledWith(codexPending);
      expect(screen.getByText('ABCD-EFGHI')).toBeInTheDocument();

      await act(async () => {
        vi.advanceTimersByTime(POLL_INTERVAL);
      });
      expect(agentsApi.get).toHaveBeenCalledTimes(2);
      expect(onChange).toHaveBeenLastCalledWith(codexSignedIn);
      expect(screen.getByText('Signed in')).toBeInTheDocument();
      expect(screen.queryByText('ABCD-EFGHI')).toBeNull();

      await act(async () => {
        vi.advanceTimersByTime(POLL_INTERVAL * 5);
      });
      expect(agentsApi.get).toHaveBeenCalledTimes(2);
    });

    it('resumes a sign-in already in progress and stops polling once unmounted', async () => {
      vi.useFakeTimers();
      agentsApi.get.mockResolvedValue(codexPending);
      const { unmount } = render(
        <AgentSignIn
          organizationId={organization}
          agent="codex"
          canManage
          status={codexPending}
          loadError={null}
          onStatusChange={mock()}
        />
      );

      expect(screen.getByText('ABCD-EFGHI')).toBeInTheDocument();
      await act(async () => {
        vi.advanceTimersByTime(POLL_INTERVAL);
      });
      expect(agentsApi.get).toHaveBeenCalledTimes(1);

      unmount();
      vi.advanceTimersByTime(POLL_INTERVAL * 5);
      await Promise.resolve();
      expect(agentsApi.get).toHaveBeenCalledTimes(1);
    });

    it('cancels a device login in progress', async () => {
      agentsApi.signOut.mockResolvedValue(undefined);
      agentsApi.get.mockResolvedValue(codexSignedOut);
      const { onChange } = renderPanel('codex', codexPending);

      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      await waitFor(() => expect(onChange).toHaveBeenCalledWith(codexSignedOut));
      expect(agentsApi.signOut).toHaveBeenCalledWith(organization, 'codex');
      expect(screen.queryByText('ABCD-EFGHI')).toBeNull();
    });

    it("shows OpenAI's refusal when the device code is refused", async () => {
      agentsApi.start.mockRejectedValue(new Error(refusal));
      renderPanel('codex', codexSignedOut);

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }));

      expect(await screen.findByRole('alert')).toHaveTextContent(refusal);
      expect(screen.getByRole('button', { name: 'Sign in with ChatGPT' })).toBeEnabled();
      expect(screen.queryByText('ABCD-EFGHI')).toBeNull();
    });

    it('shows why the last device login failed', () => {
      renderPanel('codex', { ...codexSignedOut, error: refusal });

      expect(screen.getByRole('alert')).toHaveTextContent(refusal);
    });
  });

  describe('members', () => {
    it('see the status and whom to ask, with no buttons and no code', () => {
      renderPanel('codex', codexPending, false);

      expect(screen.getByText('Signing in')).toBeInTheDocument();
      expect(screen.getByText('Ask an organization admin to sign in.')).toBeInTheDocument();
      expect(screen.queryAllByRole('button')).toHaveLength(0);
      expect(screen.queryAllByRole('link')).toHaveLength(0);
      expect(screen.queryByText('ABCD-EFGHI')).toBeNull();
    });

    it('see a signed-out agent without a way to start signing in', () => {
      renderPanel('claude', claudeSignedOut, false);

      expect(screen.getByText('Not signed in')).toBeInTheDocument();
      expect(screen.getByText('Ask an organization admin to sign in.')).toBeInTheDocument();
      expect(screen.queryAllByRole('button')).toHaveLength(0);
    });

    it('see a signed-in agent without a way to sign it out', () => {
      renderPanel('claude', claudeSignedIn, false);

      expect(screen.getByText('Signed in')).toBeInTheDocument();
      expect(screen.queryAllByRole('button')).toHaveLength(0);
    });
  });

  describe('loading', () => {
    it('says it is checking until the status arrives', () => {
      renderPanel('claude', undefined);

      expect(screen.getByText('Checking sign-in…')).toBeInTheDocument();
      expect(screen.queryAllByRole('button')).toHaveLength(0);
    });

    it('shows why the status could not be loaded', () => {
      render(
        <AgentSignIn
          organizationId={organization}
          agent="claude"
          canManage
          status={undefined}
          loadError="Failed to load coding agent sign-ins: 502"
          onStatusChange={mock()}
        />
      );

      expect(screen.getByRole('alert')).toHaveTextContent(
        'Failed to load coding agent sign-ins: 502'
      );
    });
  });
});
