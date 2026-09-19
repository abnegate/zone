import { expect, type Locator, type Page } from '@playwright/test';

/**
 * The scripted model behind the real model paths (`scripts/live-verify/model-stub.py`).
 *
 * A lane registers what the "model" will say for a message it is about to send,
 * drives the console as a person would, and then reads back what the server
 * actually sent the model: which tools were offered, what the system prompt
 * carried, and what each tool returned. The model's judgement is out of scope
 * here by construction; everything on the far side of it is in.
 */

export const modelStub = process.env.ZONE_MODEL_STUB || 'http://127.0.0.1:11435';

export interface Call {
  name: string;
  /** `$re:<pattern>` strings are filled from this turn's tool results by the stand-in. */
  arguments?: Record<string, unknown>;
  id?: string;
}

export interface Round {
  text?: string;
  reasoning?: string;
  calls?: Call[];
  status?: number;
  body?: string;
  delay?: number;
}

export interface StubRequest {
  n: number;
  kind: 'round' | 'aside';
  model: string | null;
  stream: boolean;
  trigger: string | null;
  round: number | null;
  last_user: string;
  system: string;
  tools: string[];
  messages: number;
  tool_results: { tool_call_id: string | null; content: string }[];
  exhausted: boolean;
  forwarded: boolean;
  /** What a real model answered on a forwarded round; null for scripted ones. */
  answer: { content: string; calls: { name: string; arguments: string }[]; seconds: number } | null;
}

/** The forwarded rounds of every turn whose text carried `marker`, oldest first. */
export async function forwardedFor(marker: string): Promise<StubRequest[]> {
  return (await requests()).filter(
    (r) => r.kind === 'round' && r.forwarded && r.last_user.includes(marker)
  );
}

/**
 * Wait until a real model's turn is over: its newest forwarded round answered
 * with no tool call, or with one that ends the turn (a question or a plan).
 * The screen alone cannot say this, since a tool running between rounds shows
 * no status indicator.
 */
export async function turnDone(marker: string, timeout = 1_800_000): Promise<StubRequest[]> {
  await expect
    .poll(
      async () => {
        const rounds = await forwardedFor(marker);
        const last = rounds[rounds.length - 1];
        if (!last || !last.answer) return 'open';
        const names = last.answer.calls.map((c) => c.name);
        if (names.length === 0) return 'done';
        return names.some((n) => n === 'ask_user' || n === 'submit_plan') ? 'done' : 'open';
      },
      { timeout, intervals: [5_000] }
    )
    .toBe('done');
  return forwardedFor(marker);
}

/** Every tool a real model called across the forwarded rounds of a turn. */
export function calledTools(rounds: StubRequest[]): string[] {
  return rounds.flatMap((r) => (r.answer?.calls ?? []).map((c) => c.name));
}

export async function script(trigger: string, rounds: Round[], aside?: string): Promise<void> {
  const response = await fetch(`${modelStub}/_stub/script`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ trigger, rounds, aside }),
  });
  if (!response.ok) {
    throw new Error(`could not register a script: ${response.status} ${await response.text()}`);
  }
}

export async function requests(): Promise<StubRequest[]> {
  const response = await fetch(`${modelStub}/_stub/requests`);
  if (!response.ok) {
    throw new Error(`the stand-in refused its request log: ${response.status}`);
  }
  const body = (await response.json()) as { requests?: unknown };
  if (!Array.isArray(body.requests)) {
    throw new Error(`the stand-in's request log is not a list: ${JSON.stringify(body).slice(0, 200)}`);
  }
  return body.requests as StubRequest[];
}

/**
 * The sequence number of the stand-in's newest request, or 0 before any: a
 * lane whose trigger is not stamped (the server's own "Plan approval" turn)
 * takes it first and reads only the rounds that came after.
 */
export async function latest(): Promise<number> {
  const all = await requests();
  return all.length ? all[all.length - 1].n : 0;
}

/** The agent-loop rounds a trigger answered, in order, after request `since`. */
export async function roundsFor(trigger: string, since = 0): Promise<StubRequest[]> {
  return (await requests()).filter((r) => r.kind === 'round' && r.trigger === trigger && r.n > since);
}

/** Wait until a trigger has answered `count` rounds after request `since`. */
export async function settledRounds(trigger: string, count: number, timeout = 120_000, since = 0) {
  await expect
    .poll(async () => (await roundsFor(trigger, since)).length, { timeout, intervals: [500, 1000] })
    .toBeGreaterThanOrEqual(count);
  return roundsFor(trigger, since);
}

/** A short token that keeps one test's messages apart from every other's. */
export function stamp(): string {
  return `${Date.now().toString(36)}${Math.floor(Math.random() * 1e4).toString(36)}`;
}

/** Type a message and send it, without waiting for the turn to finish. */
export async function send(page: Page, message: string): Promise<void> {
  const box = page.getByPlaceholder(/type a message/i).first();
  await box.fill(message);
  await box.press('Enter');
  await expect(page.locator('.message-user').filter({ hasText: message })).toBeVisible({
    timeout: 30_000,
  });
}

/** Wait for the open turn to finish: a reply present and no status indicator left. */
export async function settled(page: Page, replies: number, timeout = 180_000): Promise<void> {
  await expect(page.locator('.message-assistant')).toHaveCount(replies, { timeout });
  await expect(page.locator('.message-status')).toHaveCount(0, { timeout });
}

/** Approve the pending tool call if the console asks for a decision. */
export async function approveIfAsked(page: Page, timeout = 15_000): Promise<boolean> {
  const approve = page.locator('[data-testid="tool-approve"]').first();
  try {
    await approve.waitFor({ state: 'visible', timeout });
  } catch {
    return false;
  }
  await approve.click();
  return true;
}

/** Put a checkbox, native or ARIA, into the wanted state. */
export async function setChecked(box: Locator, wanted: boolean): Promise<void> {
  await expect(box).toBeVisible();
  const checked = await box.getAttribute('aria-checked');
  const current = checked === null ? await box.isChecked() : checked === 'true';
  if (current !== wanted) await box.click();
  if (checked === null) {
    await expect(box).toBeChecked({ checked: wanted });
  } else {
    await expect(box).toHaveAttribute('aria-checked', String(wanted));
  }
}

/**
 * Create a chat the way a person does: the New Chat modal, the installed
 * model, and the agent switches. Returns the chat id from the URL.
 */
export async function newChat(
  page: Page,
  options: { agent?: boolean; autoApprove?: boolean; model?: string } = {}
): Promise<string> {
  await page.goto('/chats');
  await page.getByRole('button', { name: /new chat/i }).first().click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  const model = options.model ?? process.env.ZONE_LIVE_MODEL ?? 'stand-in:latest';
  // The console's select is a listbox behind a combobox button, not a <select>.
  await dialog.getByRole('combobox', { name: /select model/i }).click();
  await page.getByRole('option', { name: model, exact: true }).click();
  if (options.agent) {
    await setChecked(dialog.getByRole('checkbox', { name: 'Agent mode' }), true);
    if (options.autoApprove) {
      await setChecked(dialog.getByRole('checkbox', { name: /auto-approve/i }), true);
    }
  }
  await dialog.getByRole('button', { name: 'Create Chat' }).click();
  await expect(page).toHaveURL(/\/chats\?id=/, { timeout: 30_000 });
  await expect(page.getByPlaceholder(/type a message/i).first()).toBeVisible();
  const id = new URL(page.url()).searchParams.get('id');
  if (!id) throw new Error('the new chat has no id in the URL');
  return id;
}
