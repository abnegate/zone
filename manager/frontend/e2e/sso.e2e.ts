import type { Page } from '@playwright/test';
import { DEVICE_HEADER } from '../src/api/device';
import { TIME_ZONE_HEADER } from '../src/api/timeZone';
import { SsoCode } from '../src/features/auth/SsoCode';
import { SSO_ATTEMPT_KEY, type SsoAttempt } from '../src/features/auth/ssoAttempt';
import { atOrigin, SSO_OFFERED, SSO_REQUIRED } from '../src/features/auth/ssoFixtures';
import type { SsoCallbackRequest, SsoPublicProviders } from '../src/features/auth/types';
import {
  COMMUNITY_EMPTY,
  ENTITLED,
  ENTITLED_EMPTY,
  ENTRA,
  LAPSED_WITH_PROVIDER,
  OKTA,
  REDIRECT_URI,
} from '../src/features/enterprise/sso/fixtures';
import type {
  CreateProviderRequest,
  EnforcementRequest,
  ProviderResponse,
  SettingsResponse,
  UpdateProviderRequest,
} from '../src/features/enterprise/sso/types';
import { expect, test } from './fixtures';
import { setupAdminAuth, setupCommonRoutes } from './layout-fixtures';
import {
  blockServiceWorker,
  createMockJwt,
  mockUser,
  routeApi,
  userPermissions,
} from './test-utils';

const SSO_PATH = '/settings/server/sso';
const LICENSE_PATH = '/settings/server/license';
const CALLBACK_PATH = '/sso/callback';

const SETTINGS_API = /\/api\/sso$/;
const PROVIDERS_API = /\/api\/sso\/providers$/;
const PROVIDER_API = /\/api\/sso\/providers\/([^/]+)$/;
const ENFORCEMENT_API = /\/api\/sso\/enforcement$/;
const PUBLIC_PROVIDERS_API = /\/api\/auth\/sso\/providers(\?|$)/;
const AUTHORIZATIONS_API = /\/api\/auth\/sso\/providers\/([^/]+)\/authorizations$/;
const CALLBACKS_API = /\/api\/auth\/sso\/callbacks$/;
const LOGIN_API = /\/api\/auth\/login$/;
const INVITATION_API = /\/api\/invitations\/sso-invitation-token$/;

const IDENTITY_PROVIDER = 'https://identity.example.test';
const STATE = 'zone-state-7d1f';
const CODE = 'c';
const INVITATION_PATH = '/invitations?token=sso-invitation-token';

const UPGRADE_NOTICE = 'Single sign-on is part of Zone Enterprise.';
const LAPSED_NOTICE = 'Your Zone license has lapsed.';
const ENFORCED_READ_ONLY =
  'Single sign-on is required. Only server admins can sign in with a password.';
const ENFORCEMENT_BLOCKED = 'Add and enable a provider before requiring single sign-on.';
const ADMIN_PASSWORD_NOTICE = 'Password sign-in is for server admins.';
const REQUIRED_MESSAGE =
  'This server uses single sign-on. Sign in with your identity provider; only server admins can use a password.';
const INCOMPLETE_MESSAGE = "Sign-in didn't complete. Start again from the sign-in page.";
const FAILED_HEADING = "Sign-in didn't complete";
const REFRESH_TOKEN = 'mock-refresh-token';

const NEW_PROVIDER_ID = '5a6b7c8d-9e0f-4a1b-8c2d-3e4f5a6b7c8d';
const UPDATED_AT = '2026-11-16T09:00:00Z';

const INVITATION = {
  organization_name: 'Acme Corp',
  org_role: 'member',
  workspace_name: 'Engineering',
  workspace_role: 'member',
  invited_by_email: 'admin@example.com',
  expires_at: '2099-01-01T00:00:00Z',
};

interface SsoServer {
  settings: SettingsResponse;
  created: CreateProviderRequest[];
  updates: UpdateProviderRequest[];
  deletions: string[];
  enforcement: EnforcementRequest[];
}

function providerOf(request: CreateProviderRequest): ProviderResponse {
  return {
    id: NEW_PROVIDER_ID,
    protocol: OKTA.protocol,
    display_name: request.display_name,
    issuer: request.issuer ?? '',
    client_id: request.client_id ?? null,
    sso_url: null,
    metadata_url: null,
    certificate_fingerprints: [],
    allowed_domains: request.allowed_domains ?? [],
    require_verified_email: request.require_verified_email ?? true,
    enabled: request.enabled ?? true,
    created_at: UPDATED_AT,
    updated_at: UPDATED_AT,
  };
}

function providerIdOf(url: string, pattern: RegExp): string {
  return decodeURIComponent(new URL(url).pathname.match(pattern)?.[1] ?? '');
}

async function serveSso(page: Page, initial: SettingsResponse): Promise<SsoServer> {
  const server: SsoServer = {
    settings: initial,
    created: [],
    updates: [],
    deletions: [],
    enforcement: [],
  };

  await routeApi(page, SETTINGS_API, (route) =>
    route.fulfill({ status: 200, json: server.settings })
  );

  await routeApi(page, PROVIDERS_API, (route) => {
    if (route.request().method() !== 'POST') {
      return route.fallback();
    }
    const request = route.request().postDataJSON() as CreateProviderRequest;
    server.created.push(request);
    const provider = providerOf(request);
    server.settings = { ...server.settings, providers: [...server.settings.providers, provider] };
    return route.fulfill({ status: 201, json: provider });
  });

  await routeApi(page, PROVIDER_API, (route) => {
    const id = providerIdOf(route.request().url(), PROVIDER_API);
    const existing = server.settings.providers.find((provider) => provider.id === id);
    if (!existing) {
      return route.fulfill({
        status: 404,
        json: { error: 'Provider not found', code: 'sso_provider_not_found' },
      });
    }
    if (route.request().method() === 'DELETE') {
      server.deletions.push(id);
      server.settings = {
        ...server.settings,
        providers: server.settings.providers.filter((provider) => provider.id !== id),
      };
      return route.fulfill({ status: 204, body: '' });
    }
    const request = route.request().postDataJSON() as UpdateProviderRequest;
    server.updates.push(request);
    const { client_secret: _secret, ...visible } = request;
    const updated: ProviderResponse = { ...existing, ...visible, updated_at: UPDATED_AT };
    server.settings = {
      ...server.settings,
      providers: server.settings.providers.map((provider) =>
        provider.id === id ? updated : provider
      ),
    };
    return route.fulfill({ status: 200, json: updated });
  });

  await routeApi(page, ENFORCEMENT_API, (route) => {
    const request = route.request().postDataJSON() as EnforcementRequest;
    server.enforcement.push(request);
    server.settings = { ...server.settings, required: request.required };
    return route.fulfill({ status: 200, json: { required: request.required } });
  });

  return server;
}

async function openSso(page: Page, initial: SettingsResponse): Promise<SsoServer> {
  await blockServiceWorker(page.context());
  await setupCommonRoutes(page, false);
  await page.goto('/login', { waitUntil: 'domcontentloaded' });
  await setupAdminAuth(page);
  const server = await serveSso(page, initial);
  await page.goto(SSO_PATH, { waitUntil: 'domcontentloaded' });
  await expect(page.getByRole('heading', { name: 'Single sign-on', level: 2 })).toBeVisible();
  return server;
}

async function servePublicProviders(page: Page, fixture: SsoPublicProviders): Promise<void> {
  await routeApi(page, PUBLIC_PROVIDERS_API, (route) => {
    if (route.request().method() !== 'GET') {
      return route.fallback();
    }
    return route.fulfill({
      status: 200,
      json: atOrigin(fixture, new URL(route.request().url()).origin),
    });
  });
}

async function startPublic(page: Page, fixture?: SsoPublicProviders): Promise<void> {
  await blockServiceWorker(page.context());
  await setupCommonRoutes(page, false);
  if (fixture) {
    await servePublicProviders(page, fixture);
  }
}

async function openPublic(page: Page, path: string): Promise<void> {
  const providers = page.waitForResponse(PUBLIC_PROVIDERS_API);
  await page.goto(path, { waitUntil: 'domcontentloaded' });
  await providers;
}

async function storeAttempt(page: Page, attempt: SsoAttempt): Promise<void> {
  await page.goto('/login', { waitUntil: 'domcontentloaded' });
  await page.evaluate(({ key, value }) => sessionStorage.setItem(key, value), {
    key: SSO_ATTEMPT_KEY,
    value: JSON.stringify(attempt),
  });
}

function storedAttempt(page: Page): Promise<string | null> {
  return page.evaluate((key) => sessionStorage.getItem(key), SSO_ATTEMPT_KEY);
}

function authResponse() {
  return {
    access_token: createMockJwt({
      sub: mockUser.id,
      email: mockUser.email,
      roles: ['user'],
      permissions: userPermissions,
      exp: Math.floor(Date.now() / 1000) + 900,
    }),
    refresh_token: REFRESH_TOKEN,
    expires_in: 900,
    user: mockUser,
    roles: ['user'],
    permissions: userPermissions,
  };
}

async function expectNoSidewaysScroll(page: Page): Promise<void> {
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth))
    .toBe(true);
}

test.describe('Single sign-on settings', () => {
  test('adds a provider with the contract body and shows the redirect URI', async ({ page }) => {
    await page.addInitScript(() => {
      const copied: string[] = [];
      Object.defineProperty(window, 'copied', { value: copied });
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: { writeText: async (text: string) => void copied.push(text) },
      });
    });
    const server = await openSso(page, ENTITLED_EMPTY);

    await expect(page.getByText(REDIRECT_URI, { exact: true })).toBeVisible();
    await page.getByRole('button', { name: 'Copy redirect URI' }).click();
    await expect(page.getByRole('button', { name: 'Copied redirect URI' })).toBeVisible();
    expect(await page.evaluate(() => (window as unknown as { copied: string[] }).copied)).toEqual([
      REDIRECT_URI,
    ]);

    const enforcement = page.getByRole('switch', { name: 'Require single sign-on' });
    await expect(enforcement).toBeDisabled();
    await expect(page.getByText(ENFORCEMENT_BLOCKED)).toBeVisible();

    await page.getByRole('button', { name: 'Add provider' }).click();
    const dialog = page.getByRole('dialog', { name: 'Add identity provider' });
    await expect(dialog).toBeVisible();
    await dialog.getByLabel('Display name').fill('Google Workspace');
    await dialog.getByLabel('Issuer URL').fill('https://accounts.google.com');
    await dialog.getByLabel('Client ID').fill('zone-client');
    await dialog.getByLabel('Client secret').fill('s3cret value');
    await dialog.getByLabel('Allowed domains').fill('Acme.com, acme.co.uk acme.com');
    const verified = dialog.getByRole('switch', { name: 'Require verified email' });
    await expect(verified).toHaveAttribute('aria-checked', 'true');
    await verified.click();
    await expect(verified).toHaveAttribute('aria-checked', 'false');
    await expect(dialog.getByRole('switch', { name: 'Enabled' })).toHaveAttribute(
      'aria-checked',
      'true'
    );
    await dialog.getByRole('button', { name: 'Add provider' }).click();

    await expect(dialog).toBeHidden();
    expect(server.created).toEqual([
      {
        display_name: 'Google Workspace',
        issuer: 'https://accounts.google.com',
        client_id: 'zone-client',
        client_secret: 's3cret value',
        allowed_domains: ['acme.com', 'acme.co.uk'],
        require_verified_email: false,
        enabled: true,
      },
    ]);
    const added = page.getByRole('listitem', { name: 'Google Workspace' });
    await expect(added).toContainText('https://accounts.google.com');
    await expect(added).toContainText('Domains: acme.com, acme.co.uk');
    await expect(added).toContainText('Provider emails trusted');
    await expect(added.getByRole('button', { name: 'Edit Google Workspace' })).toBeVisible();
    await expect(enforcement).toBeEnabled();
    await expect(page.getByText(REDIRECT_URI, { exact: true })).toBeVisible();
  });

  test('refuses to add a provider until the required fields are filled', async ({ page }) => {
    const server = await openSso(page, ENTITLED_EMPTY);

    await page.getByRole('button', { name: 'Add provider' }).click();
    const dialog = page.getByRole('dialog', { name: 'Add identity provider' });
    await dialog.getByRole('button', { name: 'Add provider' }).click();

    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole('alert').first()).toBeVisible();
    expect(server.created).toEqual([]);
  });

  test('edits a provider with a sparse update that keeps the secret', async ({ page }) => {
    const server = await openSso(page, ENTITLED);

    await page.getByRole('button', { name: `Edit ${OKTA.display_name}` }).click();
    const dialog = page.getByRole('dialog', { name: `Edit ${OKTA.display_name}` });
    const secret = dialog.getByLabel('Client secret');
    await expect(secret).toHaveValue('');
    await expect(secret).toHaveAttribute('placeholder', 'Leave blank to keep the current secret');
    await expect(dialog.getByRole('button', { name: 'Save' })).toBeDisabled();

    await dialog.getByLabel('Display name').fill('Okta Workforce');
    await dialog.getByRole('button', { name: 'Save' }).click();

    await expect(dialog).toBeHidden();
    expect(server.updates).toEqual([{ display_name: 'Okta Workforce' }]);
    await expect(page.getByRole('listitem', { name: 'Okta Workforce' })).toBeVisible();
  });

  test('deletes a provider after confirming', async ({ page }) => {
    const server = await openSso(page, ENTITLED);

    await page.getByRole('button', { name: `Delete ${ENTRA.display_name}` }).click();
    const dialog = page.getByRole('dialog', { name: `Delete ${ENTRA.display_name}?` });
    await dialog.getByRole('button', { name: 'Delete provider' }).click();

    await expect(dialog).toBeHidden();
    expect(server.deletions).toEqual([ENTRA.id]);
    await expect(page.getByRole('listitem', { name: ENTRA.display_name })).toHaveCount(0);
    await expect(page.getByRole('listitem', { name: OKTA.display_name })).toBeVisible();
  });

  test('shows the upgrade notice and no configuration on Community', async ({ page }) => {
    const server = await openSso(page, COMMUNITY_EMPTY);

    await expect(page.getByRole('note').filter({ hasText: UPGRADE_NOTICE })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Add provider' })).toHaveCount(0);
    await expect(page.getByRole('switch')).toHaveCount(0);
    await expect(page.getByRole('heading', { name: 'Identity providers' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Copy redirect URI' })).toHaveCount(0);

    await page.getByRole('link', { name: 'Go to License' }).click();
    await expect(page).toHaveURL(LICENSE_PATH);
    expect(server.created).toEqual([]);
  });

  test('keeps a lapsed configuration read-only with a warning', async ({ page }) => {
    await openSso(page, LAPSED_WITH_PROVIDER);

    await expect(page.getByRole('status').filter({ hasText: LAPSED_NOTICE })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Renew on the License page' })).toBeVisible();
    await expect(page.getByRole('listitem', { name: OKTA.display_name })).toBeVisible();
    await expect(page.getByRole('button', { name: `Edit ${OKTA.display_name}` })).toHaveCount(0);
    await expect(page.getByRole('button', { name: `Delete ${OKTA.display_name}` })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Add provider' })).toHaveCount(0);
    await expect(page.getByRole('switch')).toHaveCount(0);
    await expect(page.getByText(ENFORCED_READ_ONLY)).toBeVisible();
  });

  test('requires single sign-on only after the confirm dialog', async ({ page }) => {
    const server = await openSso(page, ENTITLED);
    const enforcement = page.getByRole('switch', { name: 'Require single sign-on' });
    await expect(enforcement).toHaveAttribute('aria-checked', 'false');

    await enforcement.click();
    const dialog = page.getByRole('dialog', { name: 'Require single sign-on?' });
    await expect(dialog).toContainText('Everyone except server admins');
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(dialog).toBeHidden();
    expect(server.enforcement).toEqual([]);

    await enforcement.click();
    await dialog.getByRole('button', { name: 'Require single sign-on' }).click();

    await expect(dialog).toBeHidden();
    await expect(enforcement).toHaveAttribute('aria-checked', 'true');
    expect(server.enforcement).toEqual([{ required: true }]);

    await enforcement.click();
    await expect(enforcement).toHaveAttribute('aria-checked', 'false');
    await expect(page.getByRole('dialog')).toHaveCount(0);
    expect(server.enforcement).toEqual([{ required: true }, { required: false }]);
  });

  test.describe('on a phone', () => {
    test.use({ viewport: { width: 375, height: 812 } });

    test('fits the providers and the form without scrolling sideways', async ({ page }) => {
      await openSso(page, ENTITLED);

      await expect(page.getByRole('button', { name: `Edit ${ENTRA.display_name}` })).toBeVisible();
      await expect(page.getByRole('button', { name: 'Copy redirect URI' })).toBeVisible();
      await expectNoSidewaysScroll(page);

      await page.getByRole('button', { name: 'Add provider' }).click();
      const dialog = page.getByRole('dialog', { name: 'Add identity provider' });
      await expect(dialog.getByRole('button', { name: 'Add provider' })).toBeVisible();
      await expectNoSidewaysScroll(page);
    });
  });
});

test.describe('Single sign-on sign-in', () => {
  test('signs in through the identity provider and lands signed in', async ({ page }) => {
    await startPublic(page, SSO_OFFERED);
    const authorizations: string[] = [];
    const callbacks: SsoCallbackRequest[] = [];
    const callbackHeaders: Record<string, string>[] = [];

    await routeApi(page, AUTHORIZATIONS_API, (route) => {
      authorizations.push(providerIdOf(route.request().url(), AUTHORIZATIONS_API));
      return route.fulfill({
        status: 201,
        json: { url: `${IDENTITY_PROVIDER}/authorize?state=${STATE}`, state: STATE },
      });
    });
    await routeApi(page, CALLBACKS_API, (route) => {
      callbacks.push(route.request().postDataJSON() as SsoCallbackRequest);
      callbackHeaders.push(route.request().headers());
      return route.fulfill({ status: 200, json: authResponse() });
    });

    await openPublic(page, '/login');
    const origin = new URL(page.url()).origin;
    const callback = `${origin}${CALLBACK_PATH}?code=${CODE}&state=${STATE}`;
    await page.route(`${IDENTITY_PROVIDER}/**`, (route) =>
      route.fulfill({
        status: 200,
        contentType: 'text/html',
        body: `<script>location.replace(${JSON.stringify(callback)})</script>`,
      })
    );

    await expect(page.locator('.auth-divider')).toBeVisible();
    await page
      .getByRole('button', { name: `Sign in with ${SSO_OFFERED.providers[0].display_name}` })
      .click();

    await expect(page).toHaveURL('/');
    await expect(page.locator('.sidebar')).toBeVisible();
    expect(authorizations).toEqual([SSO_OFFERED.providers[0].id]);
    expect(callbacks).toEqual([{ state: STATE, code: CODE }]);
    expect(callbackHeaders[0][DEVICE_HEADER.toLowerCase()]).toBeTruthy();
    expect(callbackHeaders[0][TIME_ZONE_HEADER.toLowerCase()]).toBeTruthy();
    expect(await storedAttempt(page)).toBeNull();
    expect(await page.evaluate(() => localStorage.getItem('manager_refresh_token'))).toBe(
      REFRESH_TOKEN
    );
  });

  test('keeps only the admin password reset link and explains a refused password login while required', async ({
    page,
  }) => {
    await startPublic(page, SSO_REQUIRED);
    await routeApi(page, LOGIN_API, (route) =>
      route.fulfill({
        status: 403,
        json: { error: 'Single sign-on is required.', code: SsoCode.Required },
      })
    );
    await openPublic(page, '/login');

    await expect(page.getByRole('button', { name: 'Sign in with Okta' })).toBeVisible();
    await expect(page.getByText(ADMIN_PASSWORD_NOTICE)).toBeVisible();
    await expect(
      page.getByRole('link', { name: 'Server admin? Forgot password?', exact: true })
    ).toBeVisible();
    await expect(page.getByRole('link', { name: 'Forgot password?', exact: true })).toHaveCount(0);
    await expect(page.getByRole('link', { name: 'Create one' })).toHaveCount(0);

    await page.getByLabel('Email', { exact: true }).fill('dev@example.com');
    await page.getByLabel('Password', { exact: true }).fill('Password123');
    await page.getByRole('button', { name: 'Sign In', exact: true }).click();

    await expect(page.locator('form').getByRole('alert')).toHaveText(REQUIRED_MESSAGE);
    await expect(page).toHaveURL('/login');
  });

  test('shows the identity provider refusal from the callback', async ({ page }) => {
    await startPublic(page);
    let completions = 0;
    await routeApi(page, CALLBACKS_API, (route) => {
      completions += 1;
      return route.fulfill({ status: 200, json: authResponse() });
    });
    await storeAttempt(page, { state: STATE, next: '/' });
    await page.goto(`${CALLBACK_PATH}?error=access_denied&state=${STATE}`, {
      waitUntil: 'domcontentloaded',
    });

    await expect(page.getByRole('heading', { name: FAILED_HEADING })).toBeVisible();
    await expect(
      page.getByText("Your identity provider didn't complete the sign-in (access_denied).")
    ).toBeVisible();
    expect(completions).toBe(0);

    await page.getByRole('link', { name: 'Back to sign in' }).click();
    await expect(page).toHaveURL('/login');
  });

  test('shows only the generic message when the state does not match', async ({ page }) => {
    await startPublic(page);
    const description = 'Call 555-0100 to verify your account';
    await storeAttempt(page, { state: STATE, next: '/' });
    await page.goto(
      `${CALLBACK_PATH}?error=access_denied&state=forged-state&error_description=${encodeURIComponent(description)}`,
      { waitUntil: 'domcontentloaded' }
    );

    await expect(page.getByRole('heading', { name: FAILED_HEADING })).toBeVisible();
    await expect(page.getByText(INCOMPLETE_MESSAGE)).toBeVisible();
    await expect(page.getByText(description)).toHaveCount(0);
  });

  test('strips the code and state from the callback URL after load', async ({ page }) => {
    await startPublic(page);
    await storeAttempt(page, { state: STATE, next: '/' });
    await page.goto(`${CALLBACK_PATH}?error=access_denied&state=${STATE}&code=${CODE}`, {
      waitUntil: 'domcontentloaded',
    });

    await expect(page.getByRole('heading', { name: FAILED_HEADING })).toBeVisible();
    await expect(page).toHaveURL(CALLBACK_PATH);
    expect(new URL(page.url()).search).toBe('');
    expect(await storedAttempt(page)).toBeNull();
  });

  test('shows no providers or divider when none are configured', async ({ page }) => {
    await startPublic(page);
    await openPublic(page, '/login');

    await expect(page.getByRole('button', { name: 'Sign In', exact: true })).toBeVisible();
    await expect(page.locator('.auth-sso-buttons')).toHaveCount(0);
    await expect(page.locator('.auth-divider')).toHaveCount(0);
    await expect(page.getByRole('link', { name: 'Create one' })).toBeVisible();
  });

  test('offers no account creation on an invitation while required', async ({ page }) => {
    await startPublic(page, SSO_REQUIRED);
    await routeApi(page, INVITATION_API, (route) =>
      route.fulfill({ status: 200, json: INVITATION })
    );
    await openPublic(page, INVITATION_PATH);

    await expect(page.locator('.invitation-card')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Sign In', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Create Account' })).toHaveCount(0);
  });

  test('offers account creation on an invitation when not required', async ({ page }) => {
    await startPublic(page);
    await routeApi(page, INVITATION_API, (route) =>
      route.fulfill({ status: 200, json: INVITATION })
    );
    await openPublic(page, INVITATION_PATH);

    await expect(page.getByRole('button', { name: 'Create Account' })).toBeVisible();
  });

  test.describe('on a phone', () => {
    test.use({ viewport: { width: 375, height: 812 } });

    test('keeps the provider buttons and the form inside the screen', async ({ page }) => {
      await startPublic(page, SSO_OFFERED);
      await openPublic(page, '/login');

      await expect(
        page.getByRole('button', { name: `Sign in with ${SSO_OFFERED.providers[1].display_name}` })
      ).toBeInViewport();
      await expect(page.getByRole('button', { name: 'Sign In', exact: true })).toBeVisible();
      await expectNoSidewaysScroll(page);
    });

    test('keeps the invitation sign-in button full height and its notice centred', async ({
      page,
    }) => {
      await startPublic(page, SSO_REQUIRED);
      await routeApi(page, INVITATION_API, (route) =>
        route.fulfill({ status: 200, json: INVITATION })
      );
      await openPublic(page, INVITATION_PATH);

      await expect(page.getByRole('button', { name: 'Create Account' })).toHaveCount(0);
      await expect(page.getByRole('button', { name: 'Sign In', exact: true })).toHaveCSS(
        'height',
        '36px'
      );
      await expect(page.getByText('You need to be signed in to accept this invitation')).toHaveCSS(
        'text-align',
        'center'
      );
      await expectNoSidewaysScroll(page);
    });
  });
});
