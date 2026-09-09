import { readFileSync } from 'node:fs';
import { test as base, expect, type Page } from '@playwright/test';

/**
 * The live suite talks to a real server. Nothing here mocks a route, forges a
 * token, or answers for the API: a test that passes has seen the console render
 * what the server actually sent.
 */

export interface Tenant {
  email: string;
  password: string;
  access_token: string;
  user: { id: string; email: string; display_name: string };
  organization: { id: string; name: string; slug: string };
  workspace: { id: string; name: string; slug: string };
}

export interface LiveState {
  api: string;
  owner: Tenant;
  intruder: Tenant;
}

const statePath = process.env.ZONE_LIVE_STATE;

export const state: LiveState = (() => {
  if (!statePath) {
    throw new Error('ZONE_LIVE_STATE must point at the file scripts/live-verify.sh writes');
  }
  return JSON.parse(readFileSync(statePath, 'utf8'));
})();

export const comfyStub = process.env.ZONE_COMFY_STUB || 'http://127.0.0.1:8188';

/** Sign in through the real login form and land on the seeded workspace. */
export async function signIn(page: Page, who: Tenant = state.owner): Promise<void> {
  await page.addInitScript((context) => {
    localStorage.setItem('manager_current_org', context.organization);
    localStorage.setItem('manager_current_workspace', context.workspace);
  }, { organization: who.organization.id, workspace: who.workspace.id });

  // An already-signed-in page is bounced off /login, so the session in hand is
  // dropped first: a test may sign in as a second account on the same page.
  await page.goto('/login');
  await page.evaluate(() => {
    localStorage.removeItem('manager_access_token');
    localStorage.removeItem('manager_refresh_token');
    localStorage.removeItem('manager_user');
  });
  await page.goto('/login');
  await page.getByLabel('Email').fill(who.email);
  await page.getByLabel('Password').fill(who.password);
  await page.getByRole('button', { name: /sign in|log in/i }).click();
  await expect(page).not.toHaveURL(/\/login/, { timeout: 30_000 });
  await page.waitForFunction(() => Boolean(localStorage.getItem('manager_access_token')), {
    timeout: 30_000,
  });
}

/** A fresh access token straight from the API, for the checks driven outside the UI. */
export async function tokenFor(who: Tenant): Promise<string> {
  const response = await fetch(`${state.api}/api/auth/login`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ email: who.email, password: who.password }),
  });
  if (!response.ok) {
    throw new Error(`login for ${who.email} failed: ${response.status}`);
  }
  return (await response.json()).access_token;
}

export async function api(
  method: string,
  path: string,
  options: { token?: string; body?: unknown } = {}
): Promise<{ status: number; body: unknown }> {
  const response = await fetch(`${state.api}${path}`, {
    method,
    headers: {
      'content-type': 'application/json',
      ...(options.token ? { authorization: `Bearer ${options.token}` } : {}),
    },
    ...(options.body === undefined ? {} : { body: JSON.stringify(options.body) }),
  });
  const text = await response.text();
  let body: unknown = {};
  try {
    body = text ? JSON.parse(text) : {};
  } catch {
    body = { raw: text };
  }
  return { status: response.status, body };
}

export async function resetStub(): Promise<void> {
  await fetch(`${comfyStub}/_stub/reset`, { method: 'POST' }).catch(() => undefined);
}

export async function stubState(): Promise<{
  prompts: { id: string; lane: string }[];
  uploads: string[];
  cancelled: string[];
}> {
  return (await fetch(`${comfyStub}/_stub/state`)).json();
}

/** Create a chat the way the console's own client does. */
export async function createChat(
  token: string,
  options: { title: string; agent?: boolean; model?: string } = { title: 'live' }
): Promise<string> {
  const { status, body } = await api('POST', '/api/chats', {
    token,
    body: {
      workspace_id: state.owner.workspace.id,
      title: options.title,
      model_name: options.model ?? process.env.ZONE_LIVE_MODEL ?? 'llama3.2:3b',
      agent_enabled: Boolean(options.agent),
    },
  });
  if (status !== 200 && status !== 201) {
    throw new Error(`could not create a chat: ${status} ${JSON.stringify(body)}`);
  }
  const chat = (body as { chat?: { id: string }; id?: string }).chat;
  return (chat ?? (body as { id: string })).id;
}

/**
 * Send one message in the open thread and wait for the turn to finish. Media
 * lanes take a while even against the stand-in, so the wait is generous.
 */
export async function sendAndSettle(page: Page, message: string, timeout = 240_000) {
  const assistant = page.locator('.message-assistant');
  const before = await assistant.count();
  const box = page.getByPlaceholder(/type a message/i).first();
  await box.fill(message);
  await box.press('Enter');
  await expect(page.locator('.message-user').filter({ hasText: message })).toBeVisible({
    timeout: 30_000,
  });
  // Waiting only for `.message-status` to reach zero passes before the turn has
  // started, because `Generation` renders it on `message_start`. The reply
  // arriving is the real signal, so that is waited for first and the indicator
  // going is what says the turn is finished rather than still streaming.
  await expect(assistant).toHaveCount(before + 1, { timeout });
  await expect(page.locator('.message-status')).toHaveCount(0, { timeout });
}

/** Every test asserts a clean console, so a broken render cannot pass as a pass. */
export const test = base.extend<{ consoleErrors: string[] }>({
  consoleErrors: async ({ page }, use) => {
    const errors: string[] = [];
    // "Failed to load resource" carries no URL, so the failing response is
    // recorded instead: a console error a reader cannot act on is not a report.
    page.on('response', (response) => {
      const status = response.status();
      if (status < 400) return;
      const { pathname } = new URL(response.url());
      if (/favicon|\/@vite\/|\.map$/.test(pathname)) return;
      // A workspace with no theme override has no row, and the console reads
      // that 404 as "no override". Absence is not a failure.
      if (status === 404 && /^\/api\/workspaces\/[^/]+\/theme$/.test(pathname)) return;
      errors.push(`${status} ${pathname}`);
    });
    page.on('console', (message) => {
      if (message.type() !== 'error') return;
      const text = message.text();
      // Vite's HMR client and favicon noise say nothing about the app, and a
      // failed response is already recorded above with its URL.
      if (/favicon|\[vite\]|Download the React DevTools|Failed to load resource/i.test(text)) {
        return;
      }
      errors.push(text);
    });
    page.on('pageerror', (error) => errors.push(`pageerror: ${error.message}`));
    await use(errors);
  },
});

export { expect };
