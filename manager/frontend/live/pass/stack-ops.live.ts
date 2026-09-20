import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import type { Page } from '@playwright/test';
import { expect, record, shot, test } from './rig';

/**
 * Rows 59 and 60 on the compose stack, the console halves: the Models page
 * after ollama-init pulled into the bundled Ollama, and the chats after a
 * backup was restored onto a fresh database. The shell halves run from
 * scripts/stack-59-60.sh in the scratch directory; ZONE_STACK_STEP says which.
 */

const CONSOLE = process.env.ZONE_STACK_CONSOLE ?? 'http://manager.localhost';
const step = process.env.ZONE_STACK_STEP ?? '';
const statePath = process.env.ZONE_LIVE_STATE ?? '';
const stack = JSON.parse(readFileSync(statePath, 'utf8')) as {
  api: string;
  owner: {
    email: string;
    password: string;
    organization: { id: string };
    workspace: { id: string };
  };
};
const evidence = process.env.ZONE_LIVE_EVIDENCE ?? '';

async function signInStack(page: Page): Promise<void> {
  await page.addInitScript(
    (context) => {
      localStorage.setItem('manager_current_org', context.organization);
      localStorage.setItem('manager_current_workspace', context.workspace);
    },
    {
      organization: stack.owner.organization.id,
      workspace: stack.owner.workspace.id,
    },
  );
  await page.goto(`${CONSOLE}/login`);
  await page.evaluate(() => {
    localStorage.removeItem('manager_access_token');
    localStorage.removeItem('manager_refresh_token');
    localStorage.removeItem('manager_user');
  });
  await page.goto(`${CONSOLE}/login`);
  await page.getByLabel('Email').fill(stack.owner.email);
  await page.getByLabel('Password').fill(stack.owner.password);
  await page.getByRole('button', { name: /sign in|log in/i }).click();
  await expect(page).not.toHaveURL(/\/login/, { timeout: 60_000 });
  await page.waitForFunction(
    () => Boolean(localStorage.getItem('manager_access_token')),
    { timeout: 30_000 },
  );
}

test.describe('compose stack operations', () => {
  test.skip(
    !statePath.includes('stack'),
    'run with ZONE_LIVE_STATE pointing at the stack tenants',
  );
  test.describe.configure({ timeout: 900_000 });

  test('59: the models ollama-init pulled show on the Models page', async ({
    page,
  }) => {
    test.skip(step !== '59', 'ZONE_STACK_STEP=59');
    const initLog = readFileSync(`${evidence}/59-ollama-init.log`, 'utf8');
    const list = readFileSync(`${evidence}/59-bundled-ollama-list.txt`, 'utf8');
    await signInStack(page);
    await page.goto(`${CONSOLE}/models`);
    await expect(page.locator('.model-item').first()).toBeVisible({
      timeout: 120_000,
    });
    const models = await page
      .locator('.model-item .model-name')
      .allInnerTexts();
    await shot(page, '59-models-page-bundled-ollama');
    const pulled = list
      .split('\n')
      .filter((l) => /llama3\.2:1b|nomic-embed-text/.test(l));
    record(59, {
      result:
        pulled.length >= 2 &&
        models.some((m) => /llama3\.2:1b/.test(m)) &&
        models.some((m) => /nomic-embed-text/.test(m))
          ? 'WORKS'
          : 'FAILS',
      ollama_init_log_tail: initLog
        .split('\n')
        .filter(Boolean)
        .slice(-6)
        .map((l) => l.slice(0, 160)),
      bundled_ollama_list: pulled,
      models_page: models,
      note: 'The stack ran with the host Ollama (no bundled-ollama profile), so ollama-init does not exist there; for this row the bundled Ollama and ollama-init were started with small configured models and the manager was pointed at that Ollama for the Models page, then pointed back',
      screenshots: [
        '59-models-page-bundled-ollama.png',
        '59-ollama-init.log',
        '59-bundled-ollama-list.txt',
      ],
    });
    expect(models.some((m) => /llama3\.2:1b/.test(m))).toBe(true);
  });

  test('60: after backup, a fresh database and restore, the chats are back', async ({
    page,
  }) => {
    test.skip(step !== '60', 'ZONE_STACK_STEP=60');
    const before = JSON.parse(
      readFileSync(`${evidence}/60-chats-before.json`, 'utf8'),
    ) as { chats: { id: string; title: string }[] };
    await signInStack(page);
    await page.goto(`${CONSOLE}/chats`);
    await page.waitForTimeout(3_000);
    const titles = await page.locator('.chat-item .chat-title').allInnerTexts();
    await shot(page, '60-chats-after-restore');
    const response = await fetch(`${stack.api}/api/auth/login`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        email: stack.owner.email,
        password: stack.owner.password,
      }),
    });
    const token = ((await response.json()) as { access_token: string })
      .access_token;
    const listed = await fetch(
      `${stack.api}/api/chats?workspace_id=${stack.owner.workspace.id}`,
      { headers: { authorization: `Bearer ${token}` } },
    );
    const after = (await listed.json()) as {
      chats?: { id: string; title: string }[];
    };
    const restored = before.chats.filter((c) =>
      (after.chats ?? []).some((a) => a.id === c.id),
    );
    const backupLog = execFileSync(
      'sh',
      [
        '-c',
        `grep -E '== 60' ${process.env.ZONE_STACK_LOG ?? '/dev/null'} | tail -6`,
      ],
      { encoding: 'utf8' },
    )
      .trim()
      .split('\n');
    record(60, {
      result:
        restored.length === before.chats.length && before.chats.length > 0
          ? 'WORKS'
          : 'FAILS',
      chats_before_backup: before.chats.map((c) => c.title),
      chats_after_restore: (after.chats ?? []).map((c) => c.title),
      titles_on_page: titles,
      restored_by_id: restored.length,
      steps: backupLog,
      screenshots: ['60-chats-after-restore.png'],
    });
    expect(restored.length).toBe(before.chats.length);
  });
});
