import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import { AuthRequestError } from '../../../api/auth';
import { memberCapMessage } from '../../enterprise/members';
import { SsoCode } from '../SsoCode';
import { SSO_ATTEMPT_KEY } from '../ssoAttempt';
import { SSO_INCOMPLETE, SSO_MESSAGES } from '../ssoMessages';

const mockNavigate = mock();
const mockCompleteSso = mock();

mock.module('../context', () => ({
  useAuth: () => ({ completeSso: mockCompleteSso }),
}));

mock.module('react-router-dom', () => ({
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
  useNavigate: () => mockNavigate,
}));

let SsoCallbackPage: typeof import('./SsoCallbackPage').default;

beforeAll(async () => {
  SsoCallbackPage = (await import('./SsoCallbackPage')).default;
});

afterAll(() => {
  mock.restore();
  window.history.replaceState(null, '', '/');
});

function arrive(query: string, attempt?: { state: string; next: unknown }) {
  window.history.replaceState(null, '', `/sso/callback${query}`);
  if (attempt) {
    sessionStorage.setItem(SSO_ATTEMPT_KEY, JSON.stringify(attempt));
  }
  return render(<SsoCallbackPage />);
}

async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 10));
}

beforeEach(() => {
  mockNavigate.mockReset();
  mockCompleteSso.mockReset();
  mockCompleteSso.mockResolvedValue(undefined);
  sessionStorage.clear();
});

describe('SsoCallbackPage', () => {
  it('completes the sign-in and goes to the saved path', async () => {
    arrive('?code=code-1&state=state-1', { state: 'state-1', next: '/chats?id=chat-1' });

    await waitFor(() =>
      expect(mockNavigate).toHaveBeenCalledWith('/chats?id=chat-1', { replace: true })
    );
    expect(mockCompleteSso).toHaveBeenCalledTimes(1);
    expect(mockCompleteSso).toHaveBeenCalledWith({ state: 'state-1', code: 'code-1' });
  });

  it('shows progress while the sign-in completes', () => {
    mockCompleteSso.mockReturnValue(new Promise(() => {}));
    arrive('?code=code-1&state=state-1', { state: 'state-1', next: '/' });

    expect(screen.getByRole('status')).toHaveTextContent('Signing you in...');
  });

  it('strips the code, state and any error from the address bar straight away', () => {
    mockCompleteSso.mockReturnValue(new Promise(() => {}));
    arrive('?code=code-1&state=state-1&error=x&error_description=y', {
      state: 'state-1',
      next: '/',
    });

    expect(window.location.pathname).toBe('/sso/callback');
    expect(window.location.search).toBe('');
  });

  it('strips the query even when the callback is refused locally', () => {
    arrive('?code=code-1&state=forged');

    expect(window.location.search).toBe('');
  });

  it('spends the saved attempt so the callback cannot be replayed', async () => {
    arrive('?code=code-1&state=state-1', { state: 'state-1', next: '/' });

    await waitFor(() => expect(mockNavigate).toHaveBeenCalled());
    expect(sessionStorage.getItem(SSO_ATTEMPT_KEY)).toBeNull();
  });

  it('never posts when the state does not match the saved one', async () => {
    arrive('?code=code-1&state=forged', { state: 'state-1', next: '/' });
    await settle();

    expect(mockCompleteSso).not.toHaveBeenCalled();
    expect(mockNavigate).not.toHaveBeenCalled();
    expect(screen.getByRole('alert')).toHaveTextContent(SSO_INCOMPLETE);
  });

  it('never posts when nothing was saved in this browser', async () => {
    arrive('?code=code-1&state=state-1');
    await settle();

    expect(mockCompleteSso).not.toHaveBeenCalled();
    expect(screen.getByRole('alert')).toHaveTextContent(SSO_INCOMPLETE);
  });

  it('never posts without a state', async () => {
    arrive('?code=code-1', { state: 'state-1', next: '/' });
    await settle();

    expect(mockCompleteSso).not.toHaveBeenCalled();
    expect(screen.getByRole('alert')).toHaveTextContent(SSO_INCOMPLETE);
  });

  it('never posts without a code', async () => {
    arrive('?state=state-1', { state: 'state-1', next: '/' });
    await settle();

    expect(mockCompleteSso).not.toHaveBeenCalled();
    expect(screen.getByRole('alert')).toHaveTextContent(SSO_INCOMPLETE);
  });

  it.each(['//evil.test', '/\\evil.test', 'https://evil.test', 'evil'])(
    'goes to / instead of the unsafe saved path %s',
    async (next) => {
      arrive('?code=code-1&state=state-1', { state: 'state-1', next });

      await waitFor(() => expect(mockNavigate).toHaveBeenCalledWith('/', { replace: true }));
    }
  );

  it("shows the identity provider's description when the state matches", async () => {
    arrive('?error=access_denied&error_description=You+are+not+assigned+to+Zone&state=state-1', {
      state: 'state-1',
      next: '/',
    });
    await settle();

    expect(screen.getByRole('alert')).toHaveTextContent('You are not assigned to Zone');
    expect(mockCompleteSso).not.toHaveBeenCalled();
  });

  it('names the identity provider error when it sent no description', () => {
    arrive('?error=access_denied&state=state-1', { state: 'state-1', next: '/' });

    expect(screen.getByRole('alert')).toHaveTextContent('access_denied');
  });

  it('hides the description and shows only the generic message when the state does not match', async () => {
    arrive('?error=access_denied&error_description=Call+0800+to+fix+your+account&state=forged', {
      state: 'state-1',
      next: '/',
    });
    await settle();

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent(SSO_INCOMPLETE);
    expect(alert.textContent).not.toContain('0800');
    expect(screen.queryByText(/0800/)).toBeNull();
    expect(mockCompleteSso).not.toHaveBeenCalled();
  });

  it.each([
    SsoCode.AssertionInvalid,
    SsoCode.SignInRefused,
    SsoCode.SignInFailed,
    SsoCode.StateInvalid,
    SsoCode.DomainNotAllowed,
  ])('shows the Zone message for the %s code the SAML endpoint sends', async (code) => {
    arrive(`?error=${code}&state=state-1`, { state: 'state-1', next: '/' });
    await settle();

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent(SSO_MESSAGES[code]);
    expect(alert.textContent).not.toContain(code);
    expect(mockCompleteSso).not.toHaveBeenCalled();
  });

  it('prefers the Zone message over any description sent with a Zone code', async () => {
    arrive(
      `?error=${SsoCode.SignInRefused}&error_description=Call+0800+to+fix+your+account&state=state-1`,
      { state: 'state-1', next: '/' }
    );
    await settle();

    expect(screen.getByRole('alert')).toHaveTextContent(SSO_MESSAGES[SsoCode.SignInRefused]);
    expect(screen.queryByText(/0800/)).toBeNull();
  });

  it('still shows the identity provider text for an error that is not a Zone code', async () => {
    arrive('?error=access_denied&state=state-1', { state: 'state-1', next: '/' });
    await settle();

    expect(screen.getByRole('alert')).toHaveTextContent(
      "Your identity provider didn't complete the sign-in (access_denied)."
    );
  });

  it('shows only the generic message for a Zone code whose state does not match', async () => {
    arrive(`?error=${SsoCode.AssertionInvalid}&state=forged`, { state: 'state-1', next: '/' });
    await settle();

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent(SSO_INCOMPLETE);
    expect(alert.textContent).not.toContain(SSO_MESSAGES[SsoCode.AssertionInvalid]);
  });

  it('shows only the generic message for a Zone code with no state, as sent without RelayState', async () => {
    arrive(`?error=${SsoCode.StateInvalid}`, { state: 'state-1', next: '/' });
    await settle();

    expect(screen.getByRole('alert')).toHaveTextContent(SSO_INCOMPLETE);
  });

  it('explains a rate-limited SAML post, which arrives with no state, to a browser mid sign-in', async () => {
    arrive(`?error=${SsoCode.RateLimited}`, { state: 'state-1', next: '/' });
    await settle();

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent(SSO_MESSAGES[SsoCode.RateLimited]);
    expect(alert.textContent).not.toContain(SsoCode.RateLimited);
    expect(mockCompleteSso).not.toHaveBeenCalled();
  });

  it('shows only the generic message for a rate-limited callback when nothing was saved', async () => {
    arrive(`?error=${SsoCode.RateLimited}`);
    await settle();

    expect(screen.getByRole('alert')).toHaveTextContent(SSO_INCOMPLETE);
  });

  it('shows only the generic message for a rate-limited callback whose state does not match', async () => {
    arrive(`?error=${SsoCode.RateLimited}&state=forged`, { state: 'state-1', next: '/' });
    await settle();

    expect(screen.getByRole('alert')).toHaveTextContent(SSO_INCOMPLETE);
  });

  it('hides the description when nothing was saved', () => {
    arrive('?error=access_denied&error_description=Call+0800&state=state-1');

    expect(screen.queryByText(/0800/)).toBeNull();
    expect(screen.getByRole('alert')).toHaveTextContent(SSO_INCOMPLETE);
  });

  it.each(Object.values(SsoCode))('shows the message for %s', async (code) => {
    mockCompleteSso.mockRejectedValue(new AuthRequestError('Server words', code));
    arrive('?code=code-1&state=state-1', { state: 'state-1', next: '/' });

    expect(await screen.findByRole('alert')).toHaveTextContent(SSO_MESSAGES[code]);
    expect(mockNavigate).not.toHaveBeenCalled();
  });

  it("shows the server's message for a refusal it has no message for", async () => {
    mockCompleteSso.mockRejectedValue(
      new AuthRequestError('This device is waiting for an admin to allow it', 'device_pending')
    );
    arrive('?code=code-1&state=state-1', { state: 'state-1', next: '/' });

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'This device is waiting for an admin to allow it'
    );
  });

  it('explains the member cap when the sign-in would add a member past it', async () => {
    mockCompleteSso.mockRejectedValue(new AuthRequestError('Server text', 'member_cap_reached'));
    arrive('?code=code-1&state=state-1', { state: 'state-1', next: '/' });

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(memberCapMessage());
    expect(alert).not.toHaveTextContent('Server text');
    expect(mockNavigate).not.toHaveBeenCalled();
  });

  it('links back to sign in after a failure', async () => {
    arrive('?code=code-1&state=forged', { state: 'state-1', next: '/' });

    expect(screen.getByRole('link', { name: 'Back to sign in' })).toHaveAttribute('href', '/login');
  });
});
