import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  mock,
  setSystemTime,
  vi,
} from 'bun:test';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { Window as HappyWindow } from 'happy-dom';
import { type ComponentProps, useCallback, useState } from 'react';
import fixture from '../../../../../../runner/zone_server/tests/fixtures/agents.json';
import { AgentRequestError } from '../../../api/AgentRequestError';
import type { Agent, AgentStatus } from './schemas';
import type { AgentAccess, Attempt } from './types';

const agentsApi = {
  list: mock(),
  get: mock(),
  start: mock(),
  submitCode: mock(),
  cancel: mock(),
  signOut: mock(),
};

mock.module('../../../api/agents', () => ({ agentsApi }));

let AgentSignIn: typeof import('./AgentSignIn').AgentSignIn;
let POLL_INTERVAL: number;
let POLL_INTERVAL_LIMIT: number;

beforeAll(async () => {
  ({ AgentSignIn, POLL_INTERVAL, POLL_INTERVAL_LIMIT } = await import('./AgentSignIn'));
});

afterAll(() => {
  mock.restore();
});

const organization = '00000000-0000-0000-0000-000000000001';
const beforeTheCodeExpires = new Date('2026-09-23T04:00:00Z');
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
const restartAuthorize =
  'https://claude.com/cai/oauth/authorize?code=true&client_id=fake-client&response_type=code&scope=user%3Ainference&state=fake-state-3';
const focused = (element: Element) => document.activeElement === element;
const later = (minutes = 10) => new Date(Date.now() + minutes * 60_000).toISOString();
const attempt = '6f1b1f63-5a3e-4c8e-9d0e-2b7f7c1d9a10';
const onTheZoneMachine = 'http://localhost:3000';
const claudeLogin = (url: string) => ({
  agent: 'claude',
  authorize_url: url,
  expires_at: later(),
  flow: 'paste',
  attempt,
});
const openAt = (url: string) => (window as unknown as HappyWindow).happyDOM.setURL(url);
const loopbackLogin = (url: string) => ({ ...claudeLogin(url), flow: 'loopback' });
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
}: Omit<
  Props,
  'status' | 'attempt' | 'onStatusChange' | 'onAttemptChange' | 'organizationId' | 'loadError'
> & {
  initial: AgentStatus | undefined;
  onChange: (status: AgentStatus) => void;
}) {
  const [status, setStatus] = useState(initial);
  const [attempts, setAttempts] = useState<Partial<Record<Agent, Attempt>>>({});
  const report = useCallback(
    (next: AgentStatus) => {
      onChange(next);
      setStatus(next);
    },
    [onChange]
  );
  const hold = useCallback((agent: Agent, attempt: Attempt | null) => {
    setAttempts((current) => ({ ...current, [agent]: attempt ?? undefined }));
  }, []);
  return (
    <AgentSignIn
      {...props}
      organizationId={organization}
      loadError={null}
      status={status}
      attempt={attempts[props.agent]}
      onStatusChange={report}
      onAttemptChange={hold}
    />
  );
}

function renderPanel(
  agent: Props['agent'],
  initial: AgentStatus | undefined,
  access: AgentAccess = 'manage',
  unsaved = false
): { onChange: ReturnType<typeof mock> } {
  const onChange = mock();
  render(
    <Harness
      agent={agent}
      access={access}
      unsaved={unsaved}
      heading="h4"
      initial={initial}
      onChange={onChange}
    />
  );
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
        expires_at: later(),
        flow: 'paste',
      });
      agentsApi.submitCode.mockResolvedValue(claudeSignedIn);
      const { onChange } = renderPanel('claude', claudeSignedOut);

      expect(screen.getByText('Not signed in')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));

      const link = await screen.findByRole('link', { name: 'Open claude.com' });
      expect(agentsApi.start).toHaveBeenCalledWith(organization, 'claude', {});
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
      agentsApi.start.mockResolvedValue(claudeLogin(authorize));
      agentsApi.submitCode.mockResolvedValue(claudeSignedIn);
      const save = mock((event: Event) => event.preventDefault());
      render(
        <form onSubmit={save}>
          <Harness
            agent="claude"
            access="manage"
            unsaved={false}
            heading="h4"
            initial={claudeSignedOut}
            onChange={mock()}
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

    it('offers full access before a code is submitted, and not once the link asks for it', async () => {
      agentsApi.start
        .mockResolvedValueOnce(claudeLogin(authorize))
        .mockResolvedValueOnce(claudeLogin(fullAuthorize));
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
      expect(agentsApi.start).toHaveBeenLastCalledWith(organization, 'claude', { scope: 'full' });
      expect(screen.getByText(/full access to your Claude account/)).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Try again with full access' })).toBeNull();
      expect(agentsApi.submitCode).not.toHaveBeenCalled();
    });

    it('keeps the link and offers full access after a failure that names no kind', async () => {
      agentsApi.start
        .mockResolvedValueOnce(claudeLogin(authorize))
        .mockResolvedValueOnce(claudeLogin(fullAuthorize));
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
      expect(screen.getByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
        'href',
        authorize
      );

      fireEvent.click(screen.getByRole('button', { name: 'Try again with full access' }));

      await waitFor(() =>
        expect(agentsApi.start).toHaveBeenLastCalledWith(organization, 'claude', { scope: 'full' })
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

    it('keeps the link and lets a code that could not be read be pasted again', async () => {
      const unreadable = 'The code could not be read. Paste the whole code claude.com shows.';
      agentsApi.start.mockResolvedValue(claudeLogin(authorize));
      agentsApi.submitCode
        .mockRejectedValueOnce(new AgentRequestError(unreadable, 400, 'invalid_code'))
        .mockResolvedValueOnce(claudeSignedIn);
      const { onChange } = renderPanel('claude', claudeSignedOut);

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      const field = await screen.findByLabelText('Code from claude.com');
      fireEvent.change(field, { target: { value: 'half-a-code' } });
      fireEvent.click(screen.getByRole('button', { name: 'Submit code' }));

      expect(await screen.findByText(unreadable)).toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
        'href',
        authorize
      );
      expect(screen.queryByRole('button', { name: 'Start again' })).toBeNull();

      fireEvent.change(field, { target: { value: 'fake-code#fake-state' } });
      fireEvent.click(screen.getByRole('button', { name: 'Submit code' }));

      await waitFor(() => expect(onChange).toHaveBeenCalledWith(claudeSignedIn));
      expect(agentsApi.submitCode).toHaveBeenLastCalledWith(organization, 'fake-code#fake-state');
      expect(agentsApi.start).toHaveBeenCalledTimes(1);
      expect(screen.queryByText(unreadable)).toBeNull();
    });

    it('drops a link the server spent and offers to start again beside full access', async () => {
      const spent = 'The sign-in expired or was already used. Start again.';
      agentsApi.start
        .mockResolvedValueOnce(claudeLogin(authorize))
        .mockResolvedValueOnce(claudeLogin(restartAuthorize));
      agentsApi.submitCode.mockRejectedValue(new AgentRequestError(spent, 400, 'start_again'));
      renderPanel('claude', claudeSignedOut);

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      fireEvent.change(await screen.findByLabelText('Code from claude.com'), {
        target: { value: 'fake-code#fake-state' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Submit code' }));

      expect(await screen.findByRole('alert')).toHaveTextContent(spent);
      expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull();
      expect(screen.queryByLabelText('Code from claude.com')).toBeNull();
      expect(screen.getByText('Not signed in')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Try again with full access' })).toBeEnabled();
      expect(screen.queryByRole('button', { name: 'Sign in with Claude' })).toBeNull();

      fireEvent.click(screen.getByRole('button', { name: 'Start again' }));

      await waitFor(() =>
        expect(screen.getByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
          'href',
          restartAuthorize
        )
      );
      expect(agentsApi.start).toHaveBeenLastCalledWith(organization, 'claude', {});
      expect(screen.queryByRole('alert')).toBeNull();
    });

    it('starts a spent full-access sign-in again at full access', async () => {
      agentsApi.start
        .mockResolvedValueOnce(claudeLogin(authorize))
        .mockResolvedValue(claudeLogin(fullAuthorize));
      agentsApi.submitCode.mockRejectedValue(
        new AgentRequestError('Claude rejected the code. Start again.', 502, 'start_again')
      );
      renderPanel('claude', claudeSignedOut);

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      fireEvent.click(await screen.findByRole('button', { name: 'Try again with full access' }));
      await waitFor(() =>
        expect(screen.getByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
          'href',
          fullAuthorize
        )
      );
      fireEvent.change(screen.getByLabelText('Code from claude.com'), {
        target: { value: 'fake-code#fake-state' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Submit code' }));
      fireEvent.click(await screen.findByRole('button', { name: 'Start again' }));

      await waitFor(() => expect(agentsApi.start).toHaveBeenCalledTimes(3));
      expect(agentsApi.start).toHaveBeenLastCalledWith(organization, 'claude', { scope: 'full' });
      expect(await screen.findByRole('link', { name: 'Open claude.com' })).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Try again with full access' })).toBeNull();
    });

    it('cancels the sign-in on the server, and keeps it while that fails', async () => {
      const unreachable = 'Failed to cancel the claude sign-in: 502';
      agentsApi.start.mockResolvedValue(claudeLogin(authorize));
      agentsApi.cancel
        .mockRejectedValueOnce(new AgentRequestError(unreachable, 502))
        .mockResolvedValueOnce(undefined);
      renderPanel('claude', claudeSignedOut);
      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));

      fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));

      expect(await screen.findByRole('alert')).toHaveTextContent(unreachable);
      expect(agentsApi.cancel).toHaveBeenCalledWith(organization, 'claude');
      expect(screen.getByRole('link', { name: 'Open claude.com' })).toBeInTheDocument();

      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      await waitFor(() =>
        expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull()
      );
      expect(agentsApi.cancel).toHaveBeenCalledTimes(2);
      expect(screen.queryByRole('alert')).toBeNull();
      expect(screen.getByRole('button', { name: 'Sign in with Claude' })).toBeEnabled();
    });

    it('shows when the link expires and drops it once it has', async () => {
      vi.useFakeTimers({ now: new Date('2026-09-23T04:00:00Z') });
      agentsApi.start.mockResolvedValue({
        agent: 'claude',
        authorize_url: authorize,
        expires_at: '2026-09-23T04:10:00Z',
        flow: 'paste',
      });
      renderPanel('claude', claudeSignedOut);

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      });
      expect(screen.getByText('The link expires at 4:10 AM.')).toBeInTheDocument();

      await act(async () => {
        vi.advanceTimersByTime(10 * 60_000 - 1);
      });
      expect(screen.getByRole('link', { name: 'Open claude.com' })).toBeInTheDocument();
      screen.getByLabelText('Code from claude.com').focus();

      await act(async () => {
        vi.advanceTimersByTime(1);
      });
      expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull();
      expect(screen.queryByLabelText('Code from claude.com')).toBeNull();
      expect(screen.getByText(/The link from claude.com expired/)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Start again' })).toBeEnabled();
      expect(focused(screen.getByRole('status'))).toBe(true);
    });

    it('shows the plan and expiry of a sign-in this organization holds, and signs it out', async () => {
      const held: AgentStatus = { ...claudeSignedIn, expires_at: '2027-09-23T12:00:00Z' };
      agentsApi.signOut.mockResolvedValue(undefined);
      agentsApi.get.mockResolvedValue(claudeSignedOut);
      const { onChange } = renderPanel('claude', held);

      expect(screen.getByText('Signed in')).toBeInTheDocument();
      expect(screen.getByText('Claude Max · Expires Sep 23, 2027')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));

      const dialog = await screen.findByRole('dialog', { name: 'Sign out of Claude Code?' });
      expect(dialog).toHaveTextContent(
        'This signs Claude Code out for every workspace in this organization.'
      );
      expect(agentsApi.signOut).not.toHaveBeenCalled();
      fireEvent.click(within(dialog).getByRole('button', { name: 'Sign out' }));

      await waitFor(() => expect(onChange).toHaveBeenCalledWith(claudeSignedOut));
      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
      expect(agentsApi.signOut).toHaveBeenCalledWith(organization, 'claude');
      expect(agentsApi.get).toHaveBeenCalledWith(organization, 'claude');
      expect(await screen.findByRole('button', { name: 'Sign in with Claude' })).toBeEnabled();
    });

    it('keeps the sign-in when the sign-out is not confirmed', async () => {
      renderPanel('claude', claudeSignedIn);

      fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
      const dialog = await screen.findByRole('dialog', { name: 'Sign out of Claude Code?' });
      fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));

      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
      expect(agentsApi.signOut).not.toHaveBeenCalled();
      expect(screen.getByText('Signed in')).toBeInTheDocument();
    });

    it('leaves out an expiry that has passed while the sign-in still holds', () => {
      renderPanel('claude', { ...claudeSignedIn, expires_at: '2020-01-01T00:00:00Z' });

      expect(screen.getByText('Signed in')).toBeInTheDocument();
      expect(screen.getByText('Claude Max')).toBeInTheDocument();
      expect(screen.queryByText(/Expires/)).toBeNull();
    });

    it('names only the plan of a sign-in that renews itself', () => {
      renderPanel('claude', { ...claudeSignedIn, expires_at: null });

      expect(screen.getByText('Claude Max')).toBeInTheDocument();
      expect(screen.queryByText(/Expires/)).toBeNull();
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

  describe('claude, returned to Zone by the browser', () => {
    const approve = 'Approve on claude.com; Zone finishes the sign-in automatically.';
    const scopeRefused =
      'claude.com would not grant the access Zone asked for. Try again with full access.';
    const hostSignedIn: AgentStatus = { ...claudeSignedIn, source: 'host', label: 'team' };

    afterEach(() => {
      vi.useRealTimers();
    });

    async function begin(initial: AgentStatus = claudeSignedOut) {
      const rendered = renderPanel('claude', initial);
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      });
      return rendered;
    }

    async function wait(milliseconds: number) {
      await act(async () => {
        vi.advanceTimersByTime(milliseconds);
      });
    }

    it('links to claude.com, asks for no code, and signs in once claude.com sends the browser back', async () => {
      vi.useFakeTimers({ now: new Date('2026-09-23T04:00:00Z') });
      agentsApi.start.mockResolvedValue(loopbackLogin(authorize));
      agentsApi.get.mockResolvedValueOnce(claudeSignedOut).mockResolvedValueOnce(claudeSignedIn);
      const { onChange } = await begin();

      expect(agentsApi.start).toHaveBeenCalledWith(organization, 'claude', {});
      const link = screen.getByRole('link', { name: 'Open claude.com' });
      expect(link).toHaveAttribute('href', authorize);
      expect(link).toHaveAttribute('target', '_blank');
      expect(link).toHaveAttribute('rel', 'noopener noreferrer');
      expect(screen.getByText('Signing in')).toBeInTheDocument();
      expect(screen.getByText(approve)).toBeInTheDocument();
      expect(screen.getByText(/in this browser, on the machine Zone runs on/)).toBeInTheDocument();
      expect(screen.queryByLabelText('Code from claude.com')).toBeNull();
      expect(screen.queryByRole('button', { name: 'Submit code' })).toBeNull();
      expect(screen.getByRole('button', { name: 'Paste a code instead' })).toBeEnabled();
      expect(screen.getByRole('button', { name: 'Try again with full access' })).toBeEnabled();

      await wait(POLL_INTERVAL - 1);
      expect(agentsApi.get).not.toHaveBeenCalled();
      await wait(1);
      expect(agentsApi.get).toHaveBeenCalledWith(organization, 'claude', attempt);
      expect(screen.getByText(approve)).toBeInTheDocument();

      await wait(POLL_INTERVAL);
      expect(agentsApi.get).toHaveBeenCalledTimes(2);
      expect(onChange).toHaveBeenLastCalledWith(claudeSignedIn);
      expect(screen.getByText('Signed in')).toBeInTheDocument();
      expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull();
      expect(focused(screen.getByRole('status'))).toBe(true);

      await wait(POLL_INTERVAL * 5);
      expect(agentsApi.get).toHaveBeenCalledTimes(2);
    });

    it("keeps waiting through the server's own sign-in until the organization's lands", async () => {
      vi.useFakeTimers({ now: new Date('2026-09-23T04:00:00Z') });
      agentsApi.start.mockResolvedValue(loopbackLogin(authorize));
      agentsApi.get.mockResolvedValueOnce(hostSignedIn).mockResolvedValueOnce(claudeSignedIn);
      await begin(hostSignedIn);

      await wait(POLL_INTERVAL);
      expect(screen.getByText(approve)).toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'Open claude.com' })).toBeInTheDocument();

      await wait(POLL_INTERVAL);
      expect(agentsApi.get).toHaveBeenCalledTimes(2);
      expect(screen.getByText(/^Claude Max/)).toBeInTheDocument();
      expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull();
    });

    it('stops waiting and says why when claude.com sent back a sign-in that failed', async () => {
      vi.useFakeTimers({ now: new Date('2026-09-23T04:00:00Z') });
      agentsApi.start
        .mockResolvedValueOnce(loopbackLogin(authorize))
        .mockResolvedValueOnce(loopbackLogin(fullAuthorize));
      agentsApi.get.mockResolvedValue({ ...claudeSignedOut, error: scopeRefused });
      await begin();

      await wait(POLL_INTERVAL);
      expect(screen.getByRole('alert')).toHaveTextContent(scopeRefused);
      expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull();
      expect(screen.getByRole('button', { name: 'Start again' })).toBeEnabled();
      expect(screen.getByRole('button', { name: 'Paste a code instead' })).toBeEnabled();
      expect(focused(screen.getByRole('status'))).toBe(true);

      await wait(POLL_INTERVAL * 5);
      expect(agentsApi.get).toHaveBeenCalledTimes(1);

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Try again with full access' }));
      });
      expect(agentsApi.start).toHaveBeenLastCalledWith(organization, 'claude', { scope: 'full' });
      expect(screen.getByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
        'href',
        fullAuthorize
      );
      expect(screen.queryByRole('alert')).toBeNull();
    });

    it('offers to paste a code instead, which starts a sign-in whose code is pasted', async () => {
      vi.useFakeTimers({ now: new Date('2026-09-23T04:00:00Z') });
      agentsApi.start
        .mockResolvedValueOnce(loopbackLogin(authorize))
        .mockResolvedValueOnce(claudeLogin(restartAuthorize));
      agentsApi.submitCode.mockResolvedValue(claudeSignedIn);
      await begin();

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Paste a code instead' }));
      });

      expect(agentsApi.start).toHaveBeenLastCalledWith(organization, 'claude', { flow: 'paste' });
      expect(screen.getByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
        'href',
        restartAuthorize
      );
      const field = screen.getByLabelText('Code from claude.com');
      expect(focused(field)).toBe(true);
      expect(screen.queryByText(approve)).toBeNull();
      expect(screen.queryByRole('button', { name: 'Paste a code instead' })).toBeNull();

      await wait(POLL_INTERVAL * 3);
      expect(agentsApi.get).not.toHaveBeenCalled();

      fireEvent.change(field, { target: { value: 'fake-code#fake-state-3' } });
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Submit code' }));
      });
      expect(agentsApi.submitCode).toHaveBeenCalledWith(organization, 'fake-code#fake-state-3');
      expect(screen.getByText('Signed in')).toBeInTheDocument();
    });

    it('starts a pasted sign-in again as a pasted one', async () => {
      vi.useFakeTimers({ now: new Date('2026-09-23T04:00:00Z') });
      agentsApi.start
        .mockResolvedValueOnce(loopbackLogin(authorize))
        .mockResolvedValueOnce(claudeLogin(restartAuthorize))
        .mockResolvedValueOnce(claudeLogin(fullAuthorize));
      agentsApi.submitCode.mockRejectedValue(
        new AgentRequestError('Claude rejected the code. Start again.', 502, 'start_again')
      );
      await begin();
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Paste a code instead' }));
      });
      fireEvent.change(screen.getByLabelText('Code from claude.com'), {
        target: { value: 'fake-code#fake-state-3' },
      });
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Submit code' }));
      });

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Start again' }));
      });

      expect(agentsApi.start).toHaveBeenLastCalledWith(organization, 'claude', { flow: 'paste' });
      expect(screen.getByLabelText('Code from claude.com')).toBeInTheDocument();
    });

    it('stops waiting once the link has expired', async () => {
      vi.useFakeTimers({ now: new Date('2026-09-23T04:00:00Z') });
      agentsApi.start.mockResolvedValue({
        ...loopbackLogin(authorize),
        expires_at: '2026-09-23T04:10:00Z',
      });
      agentsApi.get.mockResolvedValue(claudeSignedOut);
      await begin();
      expect(screen.getByText('The link expires at 4:10 AM.')).toBeInTheDocument();
      await wait(POLL_INTERVAL);
      expect(agentsApi.get).toHaveBeenCalledTimes(1);

      await wait(10 * 60_000);
      const polls = agentsApi.get.mock.calls.length;
      expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull();
      expect(screen.getByText(/The link from claude.com expired/)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Start again' })).toBeEnabled();
      await wait(POLL_INTERVAL * 5);
      expect(agentsApi.get).toHaveBeenCalledTimes(polls);
    });

    it('cancels on the server and stops waiting', async () => {
      vi.useFakeTimers({ now: new Date('2026-09-23T04:00:00Z') });
      agentsApi.start.mockResolvedValue(loopbackLogin(authorize));
      agentsApi.get.mockResolvedValue(claudeSignedOut);
      agentsApi.cancel.mockResolvedValue(undefined);
      await begin();
      await wait(POLL_INTERVAL);
      expect(agentsApi.get).toHaveBeenCalledTimes(1);

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
      });

      expect(agentsApi.cancel).toHaveBeenCalledWith(organization, 'claude');
      expect(screen.queryByRole('link', { name: 'Open claude.com' })).toBeNull();
      expect(screen.getByRole('button', { name: 'Sign in with Claude' })).toBeEnabled();
      await wait(POLL_INTERVAL * 5);
      expect(agentsApi.get).toHaveBeenCalledTimes(1);
    });

    it('stops waiting once the panel goes away', async () => {
      vi.useFakeTimers({ now: new Date('2026-09-23T04:00:00Z') });
      agentsApi.start.mockResolvedValue(loopbackLogin(authorize));
      agentsApi.get.mockResolvedValue(claudeSignedOut);
      await begin();
      await wait(POLL_INTERVAL);
      expect(agentsApi.get).toHaveBeenCalledTimes(1);

      cleanup();
      await wait(POLL_INTERVAL * 5);

      expect(agentsApi.get).toHaveBeenCalledTimes(1);
    });

    it('moves focus to the claude.com link once the sign-in starts', async () => {
      agentsApi.start.mockResolvedValue(loopbackLogin(authorize));
      renderPanel('claude', claudeSignedOut);
      const start = screen.getByRole('button', { name: 'Sign in with Claude' });
      start.focus();

      fireEvent.click(start);

      const link = await screen.findByRole('link', { name: 'Open claude.com' });
      await waitFor(() => expect(focused(link)).toBe(true));
    });
  });

  describe('from a console on another machine', () => {
    beforeEach(() => {
      openAt('https://zone.example.com/org-settings');
    });

    afterEach(() => {
      openAt(onTheZoneMachine);
    });

    it('asks claude.com for a code to paste, at whatever scope', async () => {
      agentsApi.start
        .mockResolvedValueOnce(claudeLogin(authorize))
        .mockResolvedValueOnce(claudeLogin(fullAuthorize));
      renderPanel('claude', claudeSignedOut);

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      await screen.findByLabelText('Code from claude.com');
      fireEvent.click(screen.getByRole('button', { name: 'Try again with full access' }));

      await waitFor(() => expect(agentsApi.start).toHaveBeenCalledTimes(2));
      expect(agentsApi.start.mock.calls).toEqual([
        [organization, 'claude', { flow: 'paste' }],
        [organization, 'claude', { scope: 'full', flow: 'paste' }],
      ]);
    });

    it('starts codex as it does anywhere', async () => {
      agentsApi.start.mockResolvedValue(prompt);
      renderPanel('codex', codexSignedOut);

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }));

      await waitFor(() => expect(agentsApi.start).toHaveBeenCalledWith(organization, 'codex', {}));
    });
  });

  describe('codex', () => {
    beforeEach(() => {
      setSystemTime(beforeTheCodeExpires);
    });

    afterEach(() => {
      setSystemTime();
    });

    it('shows the link and code, polls every 3 s, and stops once signed in', async () => {
      vi.useFakeTimers({ now: beforeTheCodeExpires });
      agentsApi.start.mockResolvedValue(prompt);
      agentsApi.get.mockResolvedValueOnce(codexPending).mockResolvedValueOnce(codexSignedIn);
      const { onChange } = renderPanel('codex', codexSignedOut);

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }));
      });

      expect(agentsApi.start).toHaveBeenCalledWith(organization, 'codex', {});
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
      expect(agentsApi.get).toHaveBeenCalledWith(organization, 'codex', undefined);
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

    it('shows no code while the server records how the sign-in ended, and polls on', async () => {
      vi.useFakeTimers({ now: beforeTheCodeExpires });
      const recording: AgentStatus = { ...codexPending, pending: null };
      agentsApi.start.mockResolvedValue(prompt);
      agentsApi.get.mockResolvedValueOnce(recording).mockResolvedValueOnce(codexSignedIn);
      renderPanel('codex', codexSignedOut);

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }));
      });
      expect(screen.getByText('ABCD-EFGHI')).toBeInTheDocument();

      await act(async () => {
        vi.advanceTimersByTime(POLL_INTERVAL);
      });
      expect(agentsApi.get).toHaveBeenCalledTimes(1);
      expect(screen.getByText('Signing in')).toBeInTheDocument();
      expect(screen.queryByText('ABCD-EFGHI')).toBeNull();
      expect(screen.queryByRole('link', { name: 'auth.openai.com/codex/device' })).toBeNull();

      await act(async () => {
        vi.advanceTimersByTime(POLL_INTERVAL);
      });
      expect(agentsApi.get).toHaveBeenCalledTimes(2);
      expect(screen.getByText('Signed in')).toBeInTheDocument();
    });

    it('resumes a sign-in already in progress and stops polling once unmounted', async () => {
      vi.useFakeTimers({ now: beforeTheCodeExpires });
      agentsApi.get.mockResolvedValue(codexPending);
      const { unmount } = render(
        <AgentSignIn
          organizationId={organization}
          agent="codex"
          access="manage"
          unsaved={false}
          heading="h4"
          status={codexPending}
          attempt={undefined}
          loadError={null}
          onStatusChange={mock()}
          onAttemptChange={mock()}
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

    it('backs off while polling fails, up to a limit', async () => {
      vi.useFakeTimers({ now: beforeTheCodeExpires });
      agentsApi.get.mockRejectedValue(
        new AgentRequestError('Failed to load the codex sign-in: 502', 502)
      );
      renderPanel('codex', codexPending);
      const calls = () => agentsApi.get.mock.calls.length;
      const wait = async (milliseconds: number) => {
        await act(async () => {
          vi.advanceTimersByTime(milliseconds);
        });
      };

      await wait(POLL_INTERVAL);
      expect(calls()).toBe(1);
      expect(screen.getByRole('alert')).toHaveTextContent('Failed to load the codex sign-in: 502');

      for (const delay of [6000, 12000, 24000, POLL_INTERVAL_LIMIT, POLL_INTERVAL_LIMIT]) {
        const before = calls();
        await wait(delay - 1);
        expect(calls()).toBe(before);
        await wait(1);
        expect(calls()).toBe(before + 1);
      }
      expect(POLL_INTERVAL_LIMIT).toBe(30000);
    });

    it('starts again at the normal pace once a poll succeeds', async () => {
      vi.useFakeTimers({ now: beforeTheCodeExpires });
      agentsApi.get
        .mockRejectedValueOnce(new AgentRequestError('Failed to load the codex sign-in: 503', 503))
        .mockRejectedValueOnce(new AgentRequestError('Too many requests', 429))
        .mockRejectedValueOnce(new AgentRequestError('Request timeout', 408))
        .mockResolvedValue(codexPending);
      renderPanel('codex', codexPending);
      const wait = async (milliseconds: number) => {
        await act(async () => {
          vi.advanceTimersByTime(milliseconds);
        });
      };

      for (const delay of [POLL_INTERVAL, 6000, 12000, 24000]) {
        await wait(delay);
      }
      expect(agentsApi.get).toHaveBeenCalledTimes(4);
      expect(screen.queryByRole('alert')).toBeNull();

      await wait(POLL_INTERVAL);
      expect(agentsApi.get).toHaveBeenCalledTimes(5);
    });

    it('stops polling on a refusal that asking again cannot change', async () => {
      vi.useFakeTimers({ now: beforeTheCodeExpires });
      agentsApi.get.mockRejectedValue(new AgentRequestError('Organization not found', 404));
      renderPanel('codex', codexPending);

      await act(async () => {
        vi.advanceTimersByTime(POLL_INTERVAL);
      });
      expect(agentsApi.get).toHaveBeenCalledTimes(1);
      expect(screen.getByRole('alert')).toHaveTextContent('Organization not found');

      await act(async () => {
        vi.advanceTimersByTime(POLL_INTERVAL_LIMIT * 10);
      });
      expect(agentsApi.get).toHaveBeenCalledTimes(1);
      expect(screen.getByRole('button', { name: 'Cancel' })).toBeEnabled();
    });

    it('hides the one-time code once it has expired', async () => {
      vi.useFakeTimers({ now: beforeTheCodeExpires });
      agentsApi.get.mockResolvedValue(codexPending);
      renderPanel('codex', codexPending);

      expect(screen.getByText('ABCD-EFGHI')).toBeInTheDocument();
      expect(screen.getByText(/Expires at 4:15 AM/)).toBeInTheDocument();

      await act(async () => {
        vi.advanceTimersByTime(15 * 60_000);
      });

      expect(screen.queryByText('ABCD-EFGHI')).toBeNull();
      expect(screen.queryByText(/Expires at/)).toBeNull();
      expect(
        screen.getByText('The one-time code expired. Cancel, then sign in again.')
      ).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Cancel' })).toBeEnabled();
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

    it('hides why an earlier device login failed once the agent is signed in', () => {
      renderPanel('codex', { ...codexSignedIn, source: 'host', error: refusal });

      expect(screen.getByText('Signed in')).toBeInTheDocument();
      expect(screen.queryByRole('alert')).toBeNull();
    });
  });

  describe('for assistive technology', () => {
    it('announces the status and names the panel by its heading', () => {
      renderPanel('claude', claudeSignedIn);

      expect(
        screen.getByRole('heading', { name: 'Claude Code sign-in', level: 4 })
      ).toBeInTheDocument();
      const status = screen.getByRole('status');
      expect(status).toHaveTextContent('Signed in');
      expect(status).toHaveTextContent('Claude Max');
      expect(screen.getByRole('region', { name: 'Claude Code sign-in' })).toBeInTheDocument();
    });

    it('moves focus to the paste field once claude.com is ready, and to the status once signed in', async () => {
      agentsApi.start.mockResolvedValue(claudeLogin(authorize));
      agentsApi.submitCode.mockResolvedValue(claudeSignedIn);
      renderPanel('claude', claudeSignedOut);

      const start = screen.getByRole('button', { name: 'Sign in with Claude' });
      start.focus();
      fireEvent.click(start);

      const field = await screen.findByLabelText('Code from claude.com');
      await waitFor(() => expect(focused(field)).toBe(true));

      fireEvent.change(field, { target: { value: 'fake-code#fake-state' } });
      fireEvent.keyDown(field, { key: 'Enter' });

      await waitFor(() => expect(focused(screen.getByRole('status'))).toBe(true));
      expect(screen.getByRole('status')).toHaveTextContent('Signed in');
    });

    it('moves focus to the one-time code once the device sign-in starts', async () => {
      setSystemTime(beforeTheCodeExpires);
      agentsApi.start.mockResolvedValue(prompt);
      renderPanel('codex', codexSignedOut);

      const start = screen.getByRole('button', { name: 'Sign in with ChatGPT' });
      start.focus();
      fireEvent.click(start);

      const code = await screen.findByText('ABCD-EFGHI');
      await waitFor(() => expect(focused(code)).toBe(true));
      setSystemTime();
    });

    it('returns focus to the status when a sign-in is cancelled', async () => {
      agentsApi.start.mockResolvedValue(claudeLogin(authorize));
      renderPanel('claude', claudeSignedOut);

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      const cancel = await screen.findByRole('button', { name: 'Cancel' });
      cancel.focus();
      fireEvent.click(cancel);

      await waitFor(() => expect(focused(screen.getByRole('status'))).toBe(true));
    });

    it('leaves focus alone when the panel changes while the admin works elsewhere', async () => {
      vi.useFakeTimers({ now: beforeTheCodeExpires });
      agentsApi.start.mockResolvedValue(prompt);
      agentsApi.get.mockResolvedValue(codexSignedIn);
      render(
        <>
          <input aria-label="Elsewhere" />
          <Harness
            agent="codex"
            access="manage"
            unsaved={false}
            heading="h4"
            initial={codexSignedOut}
            onChange={mock()}
          />
        </>
      );
      const elsewhere = screen.getByLabelText('Elsewhere');

      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Sign in with ChatGPT' }));
      });
      expect(screen.getByText('ABCD-EFGHI')).toBeInTheDocument();
      elsewhere.focus();

      await act(async () => {
        vi.advanceTimersByTime(POLL_INTERVAL);
      });
      expect(screen.getByText('Signed in')).toBeInTheDocument();
      expect(focused(elsewhere)).toBe(true);
    });

    it('takes no focus when it opens on a code that has already expired', async () => {
      renderPanel('codex', codexPending);

      expect(
        screen.getByText('The one-time code expired. Cancel, then sign in again.')
      ).toBeInTheDocument();
      await act(async () => {});
      expect(focused(screen.getByRole('status'))).toBe(false);
    });

    it('describes the paste field by its hint, and by its error once a code cannot be read', async () => {
      const unreadable = 'The code could not be read.';
      agentsApi.start.mockResolvedValue(claudeLogin(authorize));
      agentsApi.submitCode.mockRejectedValue(
        new AgentRequestError(unreadable, 400, 'invalid_code')
      );
      renderPanel('claude', claudeSignedOut);

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      const field = await screen.findByLabelText('Code from claude.com');
      const described = () =>
        (field.getAttribute('aria-describedby') ?? '')
          .split(' ')
          .map((id) => document.getElementById(id)?.textContent);
      expect(described()).toEqual([
        'Paste the code claude.com shows. If claude.com refuses the request, try again with full access.',
      ]);
      expect(field.getAttribute('aria-invalid')).toBeNull();

      fireEvent.change(field, { target: { value: 'half-a-code' } });
      fireEvent.click(screen.getByRole('button', { name: 'Submit code' }));

      await waitFor(() => expect(field.getAttribute('aria-invalid')).toBe('true'));
      expect(described()).toEqual([
        'Paste the code claude.com shows. If claude.com refuses the request, try again with full access.',
        unreadable,
      ]);
      expect(screen.getByRole('alert')).toHaveTextContent(unreadable);
      expect(focused(field)).toBe(true);
    });
  });

  describe('with the provider change not saved yet', () => {
    it('asks to save once the agent is signed in', () => {
      renderPanel('claude', claudeSignedIn, 'manage', true);

      expect(screen.getByText('Signed in')).toBeInTheDocument();
      expect(screen.getByText('Save Changes to use this provider.')).toBeInTheDocument();
    });

    it('asks nothing before the agent is signed in, or once the provider is saved', () => {
      renderPanel('claude', claudeSignedOut, 'manage', true);
      expect(screen.queryByText('Save Changes to use this provider.')).toBeNull();
      cleanup();

      renderPanel('claude', claudeSignedIn, 'manage', false);
      expect(screen.queryByText('Save Changes to use this provider.')).toBeNull();
    });

    it('asks to save right after a sign-in succeeds', async () => {
      agentsApi.start.mockResolvedValue(claudeLogin(authorize));
      agentsApi.submitCode.mockResolvedValue(claudeSignedIn);
      renderPanel('claude', claudeSignedOut, 'manage', true);

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      fireEvent.change(await screen.findByLabelText('Code from claude.com'), {
        target: { value: 'fake-code#fake-state' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Submit code' }));

      expect(await screen.findByText('Save Changes to use this provider.')).toBeInTheDocument();
    });
  });

  describe('when the organization changes or the panel goes away', () => {
    it('shows none of a sign-in that another organization started', async () => {
      let finish!: (login: unknown) => void;
      agentsApi.start.mockReturnValue(
        new Promise((resolve) => {
          finish = resolve;
        })
      );
      const onAttemptChange = mock();
      const panel = (organizationId: string) => (
        <AgentSignIn
          organizationId={organizationId}
          agent="claude"
          access="manage"
          unsaved={false}
          heading="h4"
          status={claudeSignedOut}
          attempt={undefined}
          loadError={null}
          onStatusChange={mock()}
          onAttemptChange={onAttemptChange}
        />
      );
      const { rerender } = render(panel(organization));

      fireEvent.click(screen.getByRole('button', { name: 'Sign in with Claude' }));
      rerender(panel('00000000-0000-0000-0000-000000000002'));
      await act(async () => finish(claudeLogin(authorize)));

      expect(onAttemptChange).not.toHaveBeenCalled();
      expect(screen.getByRole('button', { name: 'Sign in with Claude' })).toBeEnabled();
    });

    it('still reports a sign-out that finishes after its panel closed', async () => {
      let finish!: () => void;
      agentsApi.signOut.mockReturnValue(
        new Promise<void>((resolve) => {
          finish = resolve;
        })
      );
      agentsApi.get.mockResolvedValue(claudeSignedOut);
      const onStatusChange = mock();
      const { unmount } = render(
        <AgentSignIn
          organizationId={organization}
          agent="claude"
          access="manage"
          unsaved={false}
          heading="h4"
          status={claudeSignedIn}
          attempt={undefined}
          loadError={null}
          onStatusChange={onStatusChange}
          onAttemptChange={mock()}
        />
      );

      fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
      const dialog = await screen.findByRole('dialog', { name: 'Sign out of Claude Code?' });
      fireEvent.click(within(dialog).getByRole('button', { name: 'Sign out' }));
      unmount();
      await act(async () => finish());

      await waitFor(() => expect(onStatusChange).toHaveBeenCalledWith(claudeSignedOut));
    });
  });

  describe('members', () => {
    it('see a neutral status and whom to ask, with no buttons and no code', () => {
      renderPanel('codex', codexPending, 'view');

      expect(screen.getByText('Not signed in')).toBeInTheDocument();
      expect(screen.queryByText('Signing in')).toBeNull();
      expect(screen.getByText('Ask an organization admin to sign in.')).toBeInTheDocument();
      expect(screen.queryAllByRole('button')).toHaveLength(0);
      expect(screen.queryAllByRole('link')).toHaveLength(0);
      expect(screen.queryByText('ABCD-EFGHI')).toBeNull();
    });

    it('see a signed-out agent without a way to start signing in', () => {
      renderPanel('claude', claudeSignedOut, 'view');

      expect(screen.getByText('Not signed in')).toBeInTheDocument();
      expect(screen.getByText('Ask an organization admin to sign in.')).toBeInTheDocument();
      expect(screen.queryAllByRole('button')).toHaveLength(0);
    });

    it('see a signed-in agent without a way to sign it out', () => {
      renderPanel('claude', claudeSignedIn, 'view');

      expect(screen.getByText('Signed in')).toBeInTheDocument();
      expect(screen.queryAllByRole('button')).toHaveLength(0);
    });
  });

  describe('while the role is resolving', () => {
    it('shows the status alone, with no buttons and no one to ask', () => {
      renderPanel('claude', claudeSignedOut, 'resolving');

      expect(screen.getByText('Not signed in')).toBeInTheDocument();
      expect(screen.queryAllByRole('button')).toHaveLength(0);
      expect(screen.queryByText('Ask an organization admin to sign in.')).toBeNull();
    });

    it('shows no device code, even to someone who may turn out to be an admin', () => {
      renderPanel('codex', codexPending, 'resolving');

      expect(screen.getByText('Not signed in')).toBeInTheDocument();
      expect(screen.queryByText('ABCD-EFGHI')).toBeNull();
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
          access="manage"
          unsaved={false}
          heading="h4"
          status={undefined}
          attempt={undefined}
          loadError="Failed to load coding agent sign-ins: 502"
          onStatusChange={mock()}
          onAttemptChange={mock()}
        />
      );

      expect(screen.getByRole('alert')).toHaveTextContent(
        'Failed to load coding agent sign-ins: 502'
      );
    });
  });
});
