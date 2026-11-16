import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  mock,
  spyOn,
} from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { AuthRequestError } from '../../../api/auth';
import { SSO_ATTEMPT_KEY } from '../ssoAttempt';
import { atOrigin, NO_SSO, OKTA, SSO_OFFERED } from '../ssoFixtures';
import { SSO_MESSAGES } from '../ssoMessages';

const mockAuthorize = mock();

mock.module('../../../api/sso', () => ({
  authorize: mockAuthorize,
  completeCallback: mock(),
  listProviders: mock(),
}));

let SsoButtons: typeof import('./SsoButtons').default;
let assign: ReturnType<typeof spyOn>;

beforeAll(async () => {
  SsoButtons = (await import('./SsoButtons')).default;
});

afterAll(() => {
  mock.restore();
});

beforeEach(() => {
  mockAuthorize.mockReset();
  sessionStorage.clear();
  assign = spyOn(window.location, 'assign').mockImplementation(() => {});
});

afterEach(() => {
  assign.mockRestore();
});

describe('SsoButtons', () => {
  it('renders a button per provider and the divider', () => {
    const { container } = render(<SsoButtons sso={SSO_OFFERED} next="/" />);

    expect(screen.getByRole('button', { name: 'Sign in with Okta' })).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Sign in with Microsoft Entra ID' })
    ).toBeInTheDocument();
    expect(container.querySelector('.auth-divider')).toHaveTextContent('or');
  });

  it('renders nothing, not even the divider, while loading or after a failure', () => {
    const { container } = render(<SsoButtons sso={null} next="/" />);

    expect(container.innerHTML).toBe('');
  });

  it('renders nothing, not even the divider, when no provider is enabled', () => {
    const { container } = render(<SsoButtons sso={NO_SSO} next="/" />);

    expect(container.innerHTML).toBe('');
  });

  it('saves the state and next under zone_sso_attempt before leaving for the identity provider', async () => {
    const url = 'https://acme.okta.com/oauth2/v1/authorize?state=state-1';
    mockAuthorize.mockResolvedValue({ url, state: 'state-1' });
    render(<SsoButtons sso={SSO_OFFERED} next="/chats?id=chat-1" />);

    await userEvent.click(screen.getByRole('button', { name: 'Sign in with Okta' }));

    await waitFor(() => expect(assign).toHaveBeenCalledWith(url));
    expect(mockAuthorize).toHaveBeenCalledWith(OKTA.id);
    expect(JSON.parse(sessionStorage.getItem(SSO_ATTEMPT_KEY) ?? '')).toEqual({
      state: 'state-1',
      next: '/chats?id=chat-1',
    });
  });

  it('sends the browser to the configured origin instead when this page is served elsewhere', async () => {
    render(<SsoButtons sso={atOrigin(SSO_OFFERED, 'https://zone.acme.com')} next="/" />);

    await userEvent.click(screen.getByRole('button', { name: 'Sign in with Okta' }));

    expect(assign).toHaveBeenCalledWith('https://zone.acme.com/login');
    expect(mockAuthorize).not.toHaveBeenCalled();
    expect(sessionStorage.getItem(SSO_ATTEMPT_KEY)).toBeNull();
  });

  it('starts in place when the configured origin cannot be read as a web origin', async () => {
    mockAuthorize.mockResolvedValue({ url: 'https://idp.test/authorize', state: 's1' });
    render(<SsoButtons sso={atOrigin(SSO_OFFERED, 'javascript:alert(1)')} next="/" />);

    await userEvent.click(screen.getByRole('button', { name: 'Sign in with Okta' }));

    await waitFor(() => expect(assign).toHaveBeenCalledWith('https://idp.test/authorize'));
    expect(assign).toHaveBeenCalledTimes(1);
  });

  it('shows the message for a refusal and lets the person try again', async () => {
    mockAuthorize.mockRejectedValue(
      new AuthRequestError('Provider not found', 'sso_provider_not_found')
    );
    render(<SsoButtons sso={SSO_OFFERED} next="/" />);

    await userEvent.click(screen.getByRole('button', { name: 'Sign in with Okta' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(SSO_MESSAGES.sso_provider_not_found);
    expect(assign).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Sign in with Okta' })).toBeEnabled();
  });

  it('disables every button while one sign-in is starting', async () => {
    mockAuthorize.mockReturnValue(new Promise(() => {}));
    render(<SsoButtons sso={SSO_OFFERED} next="/" />);

    await userEvent.click(screen.getByRole('button', { name: 'Sign in with Okta' }));

    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Sign in with Microsoft Entra ID' })).toBeDisabled()
    );
  });

  it('does not leave for the identity provider when the browser refuses session storage', async () => {
    mockAuthorize.mockResolvedValue({ url: 'https://idp.test/authorize', state: 's1' });
    const storage = Object.getOwnPropertyDescriptor(globalThis, 'sessionStorage');
    Object.defineProperty(globalThis, 'sessionStorage', {
      configurable: true,
      value: {
        setItem: () => {
          throw new Error('QuotaExceededError');
        },
      },
    });
    try {
      render(<SsoButtons sso={SSO_OFFERED} next="/" />);

      await userEvent.click(screen.getByRole('button', { name: 'Sign in with Okta' }));

      expect(await screen.findByRole('alert')).toHaveTextContent('session storage');
      expect(assign).not.toHaveBeenCalled();
    } finally {
      if (storage) {
        Object.defineProperty(globalThis, 'sessionStorage', storage);
      }
    }
  });
});
