import { execFileSync } from 'node:child_process';
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  statSync,
} from 'node:fs';
import { join } from 'node:path';
import type { Page } from '@playwright/test';
import {
  api,
  expect,
  signIn,
  state,
  test as base,
  tokenFor,
  type Tenant,
} from '../harness';
import {
  approveIfAsked,
  newChat,
  send,
  setChecked,
  settled,
  stamp,
} from '../stub';

/**
 * Helpers for the real pass: every row leaves a screenshot and a line of
 * evidence in the directory the report names, and the server log and database
 * are read directly so a row is judged on what the server did, not on prose.
 *
 * Opt-in with ZONE_LIVE_REAL_PASS=1: these lanes need a real tool-calling model
 * and real weights behind the rig, and say nothing when driven by a stand-in.
 */

export const enabled = process.env.ZONE_LIVE_REAL_PASS === '1';
export const model =
  process.env.ZONE_LIVE_AGENT_MODEL ??
  process.env.ZONE_LIVE_MODEL ??
  'qwen3.8:27b';
export const evidenceDir =
  process.env.ZONE_LIVE_EVIDENCE ??
  join(__dirname, '..', '..', '..', '..', 'docs', 'live-real-pass-2026-09-20');
const work = process.env.ZONE_LIVE_WORK ?? '/tmp/zone-live-verify';
export const serverLogPath = join(work, 'server.log');

mkdirSync(evidenceDir, { recursive: true });

export async function shot(page: Page, name: string): Promise<string> {
  const file = `${name}.png`;
  await page.screenshot({ path: join(evidenceDir, file), fullPage: false });
  return file;
}

export function record(row: number, entry: Record<string, unknown>): void {
  const line = JSON.stringify({ row, at: new Date().toISOString(), ...entry });
  appendFileSync(join(evidenceDir, 'evidence.jsonl'), `${line}\n`);
  console.log(`evidence ${line}`);
}

/** The server log's current length, so a check reads only what a step produced. */
export function logMark(): number {
  return existsSync(serverLogPath) ? statSync(serverLogPath).size : 0;
}

export function logSince(mark: number): string {
  if (!existsSync(serverLogPath)) return '';
  return readFileSync(serverLogPath).subarray(mark).toString('utf8');
}

const ansi = new RegExp(`${String.fromCharCode(27)}\\[[0-9;]*m`, 'g');

export function logLines(mark: number, pattern: RegExp): string[] {
  return logSince(mark)
    .split('\n')
    .map((line) => line.replace(ansi, ''))
    .filter((line) => pattern.test(line));
}

export async function logEventually(
  mark: number,
  pattern: RegExp,
  timeout = 60_000,
): Promise<string[]> {
  await expect
    .poll(() => logLines(mark, pattern).length, { timeout, intervals: [1_000] })
    .toBeGreaterThan(0);
  return logLines(mark, pattern);
}

/** One SQL statement against the rig's database, rows as tab-separated lines. */
export function sql(query: string): string[] {
  const url = process.env.DATABASE_URL;
  if (!url) throw new Error('DATABASE_URL must name the rig database');
  const out = execFileSync('psql', [url, '-Atc', query], { encoding: 'utf8' });
  return out.split('\n').filter(Boolean);
}

export async function ownerToken(): Promise<string> {
  return tokenFor(state.owner);
}

/** Tool calls the console rendered in the open thread, in order. */
export async function renderedToolCalls(page: Page): Promise<string[]> {
  return page.locator('[data-testid="tool-call"]').allInnerTexts();
}

/**
 * Type a message, approve anything the console asks to approve, and wait for
 * the turn to end: the reply present and no status indicator left, or a
 * failure alert with no reply. A turn that runs tools shows no status between
 * rounds, so the reply count is what says the turn is over.
 */
export async function ask(
  page: Page,
  message: string,
  options: { replies: number; approve?: boolean; timeout?: number } = {
    replies: 1,
  },
): Promise<string> {
  await send(page, message);
  const deadline = Date.now() + (options.timeout ?? 900_000);
  const assistant = page.locator('.message-assistant');
  for (;;) {
    if (options.approve !== false) {
      const approve = page.locator('[data-testid="tool-approve"]').first();
      if (await approve.isVisible().catch(() => false)) {
        await approve.click();
      }
    }
    const replies = await assistant.count();
    const status = await page.locator('.message-status').count();
    const alert = await page.getByRole('alert').count();
    if (replies >= options.replies && status === 0) break;
    if (alert > 0 && replies < options.replies && status === 0) break;
    if (Date.now() > deadline) {
      throw new Error(`turn did not finish within the timeout: ${message}`);
    }
    await page.waitForTimeout(1_000);
  }
  const last = assistant.last();
  return (await last.count()) ? last.innerText() : '';
}

export const test = base;
export {
  api,
  approveIfAsked,
  expect,
  newChat,
  send,
  setChecked,
  settled,
  signIn,
  stamp,
  state,
  tokenFor,
};
export type { Tenant };
