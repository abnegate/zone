import type { Page } from '@playwright/test';
import { expect, test } from './fixtures';
import { setupAuth } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

type Agent = 'claude' | 'codex';
type Role = 'owner' | 'admin' | 'member';

interface Reply {
  status?: number;
  json?: unknown;
}

interface Captured {
  method: string;
  path: string;
  body: unknown;
}

interface Scenario {
  role: Role;
  provider: string;
  workspaceProvider?: string;
  agents: (method: string, path: string, body: unknown) => Reply;
}

const organizationId = '00000000-0000-0000-0000-000000000001';
const workspaceId = '00000000-0000-0000-0000-000000000001';
const agentsPath = `/api/organizations/${organizationId}/agents`;
const authorize =
  'https://claude.com/cai/oauth/authorize?code=true&client_id=e2e-fake-client&response_type=code&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&scope=user%3Ainference&code_challenge=e2e-fake-challenge&code_challenge_method=S256&state=e2e-fake-state';
const fullAuthorize =
  'https://claude.com/cai/oauth/authorize?code=true&client_id=e2e-fake-client&response_type=code&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&scope=org%3Acreate_api_key+user%3Aprofile+user%3Ainference&code_challenge=e2e-fake-challenge-2&code_challenge_method=S256&state=e2e-fake-state-2';
const restartAuthorize =
  'https://claude.com/cai/oauth/authorize?code=true&client_id=e2e-fake-client&response_type=code&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&scope=user%3Ainference&code_challenge=e2e-fake-challenge-3&code_challenge_method=S256&state=e2e-fake-state-3';
const loopbackAuthorize =
  'https://claude.com/cai/oauth/authorize?code=true&client_id=e2e-fake-client&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A54545%2Fcallback&scope=user%3Ainference&code_challenge=e2e-fake-challenge-4&code_challenge_method=S256&state=e2e-fake-state-4';
const approve = 'Approve on claude.com; Zone finishes the sign-in automatically.';
const scopeRefused =
  'claude.com would not grant the access Zone asked for. Try again with full access.';
const claudeSignedIn = {
  state: 'signed_in',
  source: 'zone',
  label: 'Claude Max',
  expires_at: '2027-09-23T12:00:00Z',
};
const unreadable = 'The code could not be read. Paste the whole code claude.com shows.';
const spent = 'Claude rejected the code: Invalid authorization code. Start again.';
const callback =
  'https://platform.claude.com/oauth/code/callback?code=e2e-fake-code&state=e2e-fake-state';
const refusal =
  'Error logging in with device code: device code request failed with status 403 Forbidden';
const now = new Date('2026-09-23T04:00:00Z');
const device = {
  verification_url: 'https://auth.openai.com/codex/device',
  user_code: 'ABCD-EFGHI',
  expires_at: '2026-09-23T04:15:00Z',
};

function settings(provider: string) {
  return {
    provider,
    has_litellm_key: false,
    litellm_host: null,
    has_openai_api_key: false,
    openai_base_url: null,
    has_anthropic_api_key: false,
    anthropic_base_url: null,
    bedrock_region: null,
    bedrock_use_iam_role: false,
    has_bedrock_credentials: false,
    model_fast: null,
    model_reasoning: null,
    model_embedding: null,
    model_image: null,
    model_video: null,
    model_audio: null,
  };
}

function status(agent: Agent, changes: Record<string, unknown> = {}) {
  return {
    agent,
    provider: agent === 'claude' ? 'claude_code' : 'codex',
    state: 'signed_out',
    source: null,
    label: null,
    expires_at: null,
    models:
      agent === 'claude' ? ['sonnet', 'opus', 'haiku'] : ['gpt-6-astra', 'gpt-6-sol', 'gpt-6-luna'],
    pending: null,
    error: null,
    ...changes,
  };
}

const signedOut = { json: { agents: [status('claude'), status('codex')] } };

async function mockApi(page: Page, scenario: Scenario): Promise<Captured[]> {
  const captured: Captured[] = [];
  await routeApi(page, /\/api\//, async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    const method = request.method();
    const body = request.postData() ? request.postDataJSON() : null;
    captured.push({ method, path, body });

    if (path.startsWith(agentsPath)) {
      const reply = scenario.agents(method, path.slice(agentsPath.length), body);
      await route.fulfill(
        reply.json === undefined
          ? { status: reply.status ?? 204 }
          : { status: reply.status ?? 200, json: reply.json }
      );
      return;
    }
    if (path === '/api/organizations') {
      await route.fulfill({
        json: {
          organizations: [
            {
              id: organizationId,
              name: 'Acme Corp',
              slug: 'acme-corp',
              description: null,
              is_active: true,
              role: scenario.role,
              created_at: '2024-01-01T00:00:00Z',
              updated_at: '2024-01-01T00:00:00Z',
            },
          ],
        },
      });
      return;
    }
    if (path === `/api/organizations/${organizationId}/workspaces`) {
      await route.fulfill({
        json: {
          workspaces: [
            {
              id: workspaceId,
              organization_id: organizationId,
              name: 'Engineering',
              slug: 'engineering',
              description: null,
              is_active: true,
              created_at: '2024-01-01T00:00:00Z',
              updated_at: '2024-01-01T00:00:00Z',
            },
          ],
        },
      });
      return;
    }
    if (path === `/api/organizations/${organizationId}/settings/ai`) {
      const saved = method === 'PUT' ? (body as { provider: string }).provider : scenario.provider;
      await route.fulfill({ json: settings(saved) });
      return;
    }
    const workspaceSettings = `/api/organizations/${organizationId}/workspaces/${workspaceId}/settings/ai`;
    if (path === workspaceSettings) {
      await route.fulfill({
        json: {
          ...settings(scenario.workspaceProvider ?? 'self_hosted'),
          overrides: scenario.workspaceProvider !== undefined,
        },
      });
      return;
    }
    if (path === `${workspaceSettings}/effective`) {
      await route.fulfill({ json: settings(scenario.workspaceProvider ?? scenario.provider) });
      return;
    }
    if (path === '/api/models') {
      await route.fulfill({ json: { models: [] } });
      return;
    }
    await route.fallback();
  });
  return captured;
}

async function capture(page: Page, name: string): Promise<void> {
  const cards = page.locator('.settings-card');
  await expect(cards).toHaveCount(2);
  await expect(cards.nth(0).getByLabel('AI Provider')).toBeVisible();
  await expect(cards.nth(0).locator('.agent-sign-in')).toBeVisible();
  await expect(
    cards.nth(1).getByRole('heading', { name: 'Default Models', exact: true })
  ).toBeVisible();
  await expect(cards.nth(1).getByLabel('Fast Model')).toBeVisible();
  await expect(cards.nth(1).getByLabel('Reasoning Model')).toBeVisible();
  const layout = await page.evaluate(() => {
    const panel = document.querySelector('.agent-sign-in');
    const card = panel?.closest('.settings-card')?.getBoundingClientRect();
    if (!panel || !card) return null;
    const escaped = Array.from(panel.querySelectorAll('*'))
      .filter((element) => {
        const box = element.getBoundingClientRect();
        return box.width > 0 && (box.left < card.left - 0.5 || box.right > card.right + 0.5);
      })
      .map((element) => `${element.tagName.toLowerCase()}.${element.className}`);
    const root = document.documentElement;
    return { escaped, scrollsSideways: root.scrollWidth > root.clientWidth };
  });
  expect(layout).toEqual({ escaped: [], scrollsSideways: false });
  await page.screenshot({
    path: `screenshots/agent-sign-in-${name}.png`,
    fullPage: true,
    animations: 'disabled',
  });
}

test.use({ viewport: { width: 1280, height: 1200 }, timezoneId: 'UTC', locale: 'en-US' });

test.describe('Coding agent sign-in', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    await page.clock.setFixedTime(now);
  });

  test('an owner signs Claude Code in by pasting the callback address', async ({ page }) => {
    const captured = await mockApi(page, {
      role: 'owner',
      provider: 'claude_code',
      agents: (method, path) => {
        if (method === 'GET' && path === '') return signedOut;
        if (method === 'POST' && path === '/claude/login') {
          return {
            json: {
              agent: 'claude',
              authorize_url: authorize,
              expires_at: '2026-09-23T04:10:00Z',
              flow: 'paste',
            },
          };
        }
        if (method === 'POST' && path === '/claude/login/code') {
          return {
            json: status('claude', {
              state: 'signed_in',
              source: 'zone',
              label: 'Claude Max',
              expires_at: '2027-09-23T12:00:00Z',
            }),
          };
        }
        return { status: 404, json: { error: `unexpected ${method} ${path}` } };
      },
    });
    await setupAuth(page, { isAdmin: true });
    await page.goto('/org-settings');

    const panel = page.getByRole('region', { name: 'Claude Code sign-in' });
    await expect(panel.getByText('Not signed in')).toBeVisible();
    await expect(page.getByLabel('AI Provider')).toHaveValue('claude_code');
    await expect(page.getByLabel(/LiteLLM Host/)).toHaveCount(0);

    await panel.getByRole('button', { name: 'Sign in with Claude' }).click();
    const link = panel.getByRole('link', { name: 'Open claude.com' });
    await expect(link).toHaveAttribute('href', authorize);
    await expect(link).toHaveAttribute('target', '_blank');
    await expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    await expect(panel.getByText('The link expires at 4:10 AM.')).toBeVisible();
    await expect(panel.getByLabel('Code from claude.com')).toBeFocused();
    await expect(panel.getByRole('button', { name: 'Try again with full access' })).toBeVisible();
    await expect(panel.getByRole('button', { name: 'Submit code' })).toBeDisabled();
    await expect(panel.locator('ol')).toHaveCSS('list-style-type', 'decimal');
    await expect(page.getByLabel('Embedding Model')).toHaveAttribute(
      'placeholder',
      'Server default'
    );
    expect(captured.find((request) => request.path.endsWith('/claude/login'))?.body).toEqual({});
    await capture(page, 'claude-start');

    const field = panel.getByLabel('Code from claude.com');
    await field.fill(callback);
    await expect(panel.getByRole('button', { name: 'Submit code' })).toBeEnabled();
    await capture(page, 'claude-code');

    await field.press('Enter');
    await expect(panel.getByText('Signed in', { exact: true })).toBeVisible();
    await expect(panel.getByText('Claude Max · Expires Sep 23, 2027')).toBeVisible();
    await expect(panel.getByRole('status')).toBeFocused();
    await expect(panel.getByRole('button', { name: 'Sign out' })).toBeVisible();
    await expect(field).toHaveCount(0);
    expect(captured.find((request) => request.path.endsWith('/claude/login/code'))?.body).toEqual({
      code: callback,
    });
    expect(captured.filter((request) => request.method === 'PUT')).toHaveLength(0);

    const fast = page.getByLabel('Fast Model');
    await expect(fast.locator('option')).toHaveText(['Automatic', 'sonnet', 'opus', 'haiku']);
    await fast.selectOption('sonnet');
    await page.getByLabel('Reasoning Model').selectOption('opus');
    await expect(page.getByText("this server's own embedding engine")).toBeVisible();
    await capture(page, 'signed-in');

    await page.getByRole('button', { name: 'Save Changes' }).click();
    await expect(page.locator('.alert-success')).toContainText('Settings saved successfully');
    const saved = captured.find((request) => request.method === 'PUT')?.body as Record<
      string,
      unknown
    >;
    expect(saved.provider).toBe('claude_code');
    expect(saved.model_fast).toBe('sonnet');
    expect(saved.model_reasoning).toBe('opus');
    expect(saved.model_embedding).toBe('');
    expect(Object.keys(saved).filter((key) => !key.startsWith('model_'))).toEqual(['provider']);

    await panel.getByRole('button', { name: 'Sign out' }).click();
    const dialog = page.getByRole('dialog', { name: 'Sign out of Claude Code?' });
    await expect(dialog).toContainText(
      'This signs Claude Code out for every workspace in this organization.'
    );
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(panel.getByText('Signed in', { exact: true })).toBeVisible();
    expect(captured.filter((request) => request.method === 'DELETE')).toHaveLength(0);
  });

  test('a Claude sign-in keeps its link for a bad paste and starts again once spent', async ({
    page,
  }) => {
    const links = [authorize, restartAuthorize, fullAuthorize];
    let exchanges = 0;
    const captured = await mockApi(page, {
      role: 'admin',
      provider: 'claude_code',
      agents: (method, path) => {
        if (method === 'GET' && path === '') return signedOut;
        if (method === 'POST' && path === '/claude/login') {
          return {
            json: {
              agent: 'claude',
              authorize_url: links.shift(),
              expires_at: '2026-09-23T04:10:00Z',
              flow: 'paste',
            },
          };
        }
        if (method === 'POST' && path === '/claude/login/code') {
          exchanges += 1;
          return exchanges === 1
            ? { status: 400, json: { error: unreadable, kind: 'invalid_code' } }
            : { status: 502, json: { error: spent, kind: 'start_again' } };
        }
        return { status: 404, json: { error: `unexpected ${method} ${path}` } };
      },
    });
    await setupAuth(page, { isAdmin: true });
    await page.goto('/org-settings');

    const panel = page.getByRole('region', { name: 'Claude Code sign-in' });
    await panel.getByRole('button', { name: 'Sign in with Claude' }).click();
    const field = panel.getByLabel('Code from claude.com');
    await field.fill('half-a-code');
    await panel.getByRole('button', { name: 'Submit code' }).click();

    await expect(panel.getByRole('alert')).toHaveText(unreadable);
    await expect(field).toHaveAttribute('aria-invalid', 'true');
    await expect(field).toBeFocused();
    await expect(panel.getByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
      'href',
      authorize
    );
    await capture(page, 'claude-rejected');

    await field.fill('e2e-fake-code#e2e-fake-state');
    await panel.getByRole('button', { name: 'Submit code' }).click();

    await expect(panel.getByRole('alert')).toHaveText(spent);
    await expect(panel.getByRole('link', { name: 'Open claude.com' })).toHaveCount(0);
    await expect(field).toHaveCount(0);
    await expect(panel.getByText('Not signed in', { exact: true })).toBeVisible();
    await expect(
      panel.getByText('Start again to get a new link from claude.com.', { exact: true })
    ).toBeVisible();
    await expect(panel.getByRole('button', { name: 'Try again with full access' })).toBeVisible();
    await capture(page, 'start-again');

    await panel.getByRole('button', { name: 'Start again' }).click();
    await expect(panel.getByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
      'href',
      restartAuthorize
    );
    await expect(panel.getByRole('alert')).toHaveCount(0);

    await panel.getByRole('button', { name: 'Try again with full access' }).click();
    await expect(panel.getByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
      'href',
      fullAuthorize
    );
    await expect(
      panel.getByText(
        'This link asks for full access to your Claude account. It expires at 4:10 AM.'
      )
    ).toBeVisible();
    await expect(panel.getByRole('button', { name: 'Try again with full access' })).toHaveCount(0);
    const starts = captured.filter((request) => request.path.endsWith('/claude/login'));
    expect(starts.map((request) => request.body)).toEqual([{}, {}, { scope: 'full' }]);
  });

  test('an owner signs Claude Code in with nothing pasted once claude.com sends the browser back', async ({
    page,
  }) => {
    let polls = 0;
    const captured = await mockApi(page, {
      role: 'owner',
      provider: 'claude_code',
      agents: (method, path) => {
        if (method === 'GET' && path === '') return signedOut;
        if (method === 'POST' && path === '/claude/login') {
          return {
            json: {
              agent: 'claude',
              authorize_url: loopbackAuthorize,
              expires_at: '2026-09-23T04:10:00Z',
              flow: 'loopback',
            },
          };
        }
        if (method === 'GET' && path === '/claude') {
          polls += 1;
          return { json: polls === 1 ? status('claude') : status('claude', claudeSignedIn) };
        }
        return { status: 404, json: { error: `unexpected ${method} ${path}` } };
      },
    });
    await setupAuth(page, { isAdmin: true });
    await page.goto('/org-settings');

    const panel = page.getByRole('region', { name: 'Claude Code sign-in' });
    await panel.getByRole('button', { name: 'Sign in with Claude' }).click();
    const link = panel.getByRole('link', { name: 'Open claude.com' });
    await expect(link).toHaveAttribute('href', loopbackAuthorize);
    await expect(link).toHaveAttribute('target', '_blank');
    await expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    await expect(link).toBeFocused();
    await expect(panel.getByText('Signing in', { exact: true })).toBeVisible();
    await expect(panel.getByText(approve)).toBeVisible();
    await expect(panel.getByText('The link expires at 4:10 AM.')).toBeVisible();
    await expect(panel.getByLabel('Code from claude.com')).toHaveCount(0);
    await expect(panel.getByRole('button', { name: 'Paste a code instead' })).toBeVisible();
    await expect(panel.getByRole('button', { name: 'Try again with full access' })).toBeVisible();
    await capture(page, 'claude-loopback');

    await expect(panel.getByText('Signed in', { exact: true })).toBeVisible({ timeout: 10000 });
    await expect(panel.getByText('Claude Max · Expires Sep 23, 2027')).toBeVisible();
    await expect(panel.getByRole('status')).toBeFocused();
    await expect(link).toHaveCount(0);
    expect(polls).toBe(2);
    await page.waitForTimeout(4000);
    expect(polls).toBe(2);
    expect(captured.find((request) => request.path.endsWith('/claude/login'))?.body).toEqual({});
    expect(captured.filter((request) => request.path.endsWith('/claude/login/code'))).toHaveLength(
      0
    );
  });

  test('a Claude sign-in returned to Zone falls back to a pasted code, and says why it failed', async ({
    page,
  }) => {
    const logins = [
      { authorize_url: loopbackAuthorize, flow: 'loopback' },
      { authorize_url: authorize, flow: 'paste' },
    ];
    const captured = await mockApi(page, {
      role: 'admin',
      provider: 'claude_code',
      agents: (method, path) => {
        if (method === 'GET' && path === '') return signedOut;
        if (method === 'POST' && path === '/claude/login') {
          return {
            json: { agent: 'claude', expires_at: '2026-09-23T04:10:00Z', ...logins.shift() },
          };
        }
        if (method === 'GET' && path === '/claude') {
          return { json: status('claude', { error: scopeRefused }) };
        }
        if (method === 'POST' && path === '/claude/login/code') {
          return { json: status('claude', claudeSignedIn) };
        }
        return { status: 404, json: { error: `unexpected ${method} ${path}` } };
      },
    });
    await setupAuth(page, { isAdmin: true });
    await page.goto('/org-settings');

    const panel = page.getByRole('region', { name: 'Claude Code sign-in' });
    await panel.getByRole('button', { name: 'Sign in with Claude' }).click();
    await expect(panel.getByText(approve)).toBeVisible();

    await expect(panel.getByRole('alert')).toHaveText(scopeRefused, { timeout: 10000 });
    await expect(panel.getByRole('link', { name: 'Open claude.com' })).toHaveCount(0);
    await expect(panel.getByRole('button', { name: 'Start again' })).toBeVisible();
    await expect(panel.getByRole('button', { name: 'Try again with full access' })).toBeVisible();
    await expect(panel.getByRole('status')).toBeFocused();
    await capture(page, 'claude-loopback-failed');

    await panel.getByRole('button', { name: 'Paste a code instead' }).click();
    const field = panel.getByLabel('Code from claude.com');
    await expect(field).toBeFocused();
    await expect(panel.getByRole('link', { name: 'Open claude.com' })).toHaveAttribute(
      'href',
      authorize
    );
    await expect(panel.getByRole('alert')).toHaveCount(0);
    await expect(panel.getByRole('button', { name: 'Paste a code instead' })).toHaveCount(0);
    await capture(page, 'claude-paste-instead');

    await field.fill(callback);
    await field.press('Enter');
    await expect(panel.getByText('Signed in', { exact: true })).toBeVisible();
    const starts = captured.filter((request) => request.path.endsWith('/claude/login'));
    expect(starts.map((request) => request.body)).toEqual([{}, { flow: 'paste' }]);
  });

  test('an owner signs Codex in with a device code and polling stops once signed in', async ({
    page,
  }) => {
    let polls = 0;
    await mockApi(page, {
      role: 'owner',
      provider: 'codex',
      agents: (method, path) => {
        if (method === 'GET' && path === '') return signedOut;
        if (method === 'POST' && path === '/codex/login') {
          return { json: { agent: 'codex', ...device } };
        }
        if (method === 'GET' && path === '/codex') {
          polls += 1;
          return {
            json:
              polls === 1
                ? status('codex', { state: 'pending', pending: device })
                : status('codex', { state: 'signed_in', source: 'zone', label: 'ChatGPT Plus' }),
          };
        }
        return { status: 404, json: { error: `unexpected ${method} ${path}` } };
      },
    });
    await setupAuth(page, { isAdmin: true });
    await page.goto('/org-settings');

    const panel = page.getByRole('region', { name: 'Codex sign-in' });
    await panel.getByRole('button', { name: 'Sign in with ChatGPT' }).click();
    await expect(panel.getByText('ABCD-EFGHI')).toBeVisible();
    await expect(panel.getByText('ABCD-EFGHI')).toBeFocused();
    await expect(panel.getByText('Signing in', { exact: true })).toBeVisible();
    const link = panel.getByRole('link', { name: 'auth.openai.com/codex/device' });
    await expect(link).toHaveAttribute('href', device.verification_url);
    await expect(link).toHaveAttribute('target', '_blank');
    await expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    await expect(panel.getByText('Expires at 4:15 AM.', { exact: false })).toBeVisible();
    await capture(page, 'codex-pending');

    await expect(panel.getByText('Signed in', { exact: true })).toBeVisible({ timeout: 10000 });
    await expect(panel.getByText('ChatGPT Plus')).toBeVisible();
    await expect(panel.getByText('ABCD-EFGHI')).toHaveCount(0);
    expect(polls).toBe(2);
    await page.waitForTimeout(4000);
    expect(polls).toBe(2);
    await expect(page.getByLabel('Fast Model').locator('option')).toHaveText([
      'Automatic',
      'gpt-6-astra',
      'gpt-6-sol',
      'gpt-6-luna',
    ]);
  });

  test("OpenAI's refusal of the device code shows in the panel", async ({ page }) => {
    await mockApi(page, {
      role: 'owner',
      provider: 'codex',
      agents: (method, path) => {
        if (method === 'GET' && path === '') return signedOut;
        if (method === 'POST' && path === '/codex/login') {
          return { status: 502, json: { error: refusal } };
        }
        return { status: 404, json: { error: `unexpected ${method} ${path}` } };
      },
    });
    await setupAuth(page, { isAdmin: true });
    await page.goto('/org-settings');

    const panel = page.getByRole('region', { name: 'Codex sign-in' });
    await panel.getByRole('button', { name: 'Sign in with ChatGPT' }).click();
    await expect(panel.getByRole('alert')).toHaveText(refusal);
    await expect(panel.getByRole('button', { name: 'Sign in with ChatGPT' })).toBeEnabled();
    await expect(panel.getByText('ABCD-EFGHI')).toHaveCount(0);
    await capture(page, 'codex-refused');
  });

  test('a member sees a sign-in in progress as not signed in, with no code or button', async ({
    page,
  }) => {
    const captured = await mockApi(page, {
      role: 'member',
      provider: 'claude_code',
      workspaceProvider: 'codex',
      agents: (method, path) => {
        const hidden = status('codex', {
          state: 'pending',
          pending: { ...device, user_code: null },
        });
        if (method === 'GET' && path === '')
          return { json: { agents: [status('claude'), hidden] } };
        if (method === 'GET' && path === '/codex') return { json: hidden };
        return {
          status: 403,
          json: { error: 'Only organization admins can sign in to coding agents' },
        };
      },
    });
    await setupAuth(page);
    await page.goto('/settings');
    await page.getByRole('tab', { name: 'AI Settings' }).click();

    await expect(page.getByLabel('Override organization AI settings')).toBeChecked();
    await expect(page.getByLabel('AI Provider')).toHaveValue('codex');
    const panel = page.getByRole('region', { name: 'Codex sign-in' });
    await expect(panel.getByText('Not signed in', { exact: true })).toBeVisible();
    await expect(panel.getByText('Signing in', { exact: true })).toHaveCount(0);
    await expect(panel.getByText('Ask an organization admin to sign in.')).toBeVisible();
    await expect(panel.getByRole('button')).toHaveCount(0);
    await expect(panel.getByRole('link')).toHaveCount(0);
    await expect(panel.getByText('ABCD-EFGHI')).toHaveCount(0);
    await capture(page, 'member-neutral');
    expect(
      captured.filter((request) => request.method !== 'GET' && request.path.startsWith(agentsPath))
    ).toHaveLength(0);
  });

  test('a member sees a working sign-in without a way to end it', async ({ page }) => {
    const signedIn = status('claude', {
      state: 'signed_in',
      source: 'zone',
      label: 'Claude Max',
      expires_at: '2027-09-23T12:00:00Z',
    });
    const captured = await mockApi(page, {
      role: 'member',
      provider: 'codex',
      workspaceProvider: 'claude_code',
      agents: (method, path) => {
        if (method === 'GET' && path === '') {
          return { json: { agents: [signedIn, status('codex')] } };
        }
        return {
          status: 403,
          json: { error: 'Only organization admins can sign in to coding agents' },
        };
      },
    });
    await setupAuth(page);
    await page.goto('/settings');
    await page.getByRole('tab', { name: 'AI Settings' }).click();

    await expect(page.getByLabel('AI Provider')).toHaveValue('claude_code');
    const panel = page.getByRole('region', { name: 'Claude Code sign-in' });
    await expect(panel.getByRole('heading', { level: 3 })).toHaveText('Claude Code sign-in');
    await expect(panel.getByText('Signed in', { exact: true })).toBeVisible();
    await expect(panel.getByText('Claude Max · Expires Sep 23, 2027')).toBeVisible();
    await expect(panel.getByRole('button')).toHaveCount(0);
    await expect(panel.getByText('Save Changes to use this provider.')).toHaveCount(0);
    await capture(page, 'member-view');
    expect(
      captured.filter((request) => request.method !== 'GET' && request.path.startsWith(agentsPath))
    ).toHaveLength(0);
  });
});
