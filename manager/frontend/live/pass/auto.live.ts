import type { Page } from '@playwright/test';
import { ProjectAutomationSchema } from '../../src/features/projects/schemas';
import type { ProjectAutomation } from '../../src/features/projects/types';
import {
  api,
  enabled,
  expect,
  logLines,
  logMark,
  ownerToken,
  record,
  send,
  shot,
  signIn,
  sql,
  stamp,
  test,
} from './rig';

/**
 * Features 112 and 114: an auto project from a short brief. The planner chat
 * is opened from Projects, its questions are answered the way a person would
 * (the recommended choice, or the repository and "no deployment" when those
 * are asked), and once finalize_project has made the project the driver is
 * left alone: task runs, pull requests, the wait for CI, the review, the
 * squash-merge and the summary in the updates chat. A create_repository
 * approval is denied: this project uses the existing scratch repository.
 */

const INTERVIEW_ROUNDS = 14;
const DRIVE_TIMEOUT_MS = 4 * 3_600_000;
const QUIET_POLLS = 3;

let plannedProjectId = '';

const BRIEF = (s: string) =>
  [
    `Add a greeting helper to the existing GitHub repository abnegate/zone-tests, which is connected in this workspace as the "Scratch repo" source.`,
    `Put a greet(name) function in src/greet_${s}.py that returns "Hello, <name>!", and check it from check.sh so the repository's existing CI (.github/workflows/ci.yml runs ./check.sh) covers it.`,
    'It is a tiny Python change: no new tooling, no framework, and it is not deployed anywhere (deployment target: none).',
    'Plan as few tasks as your rules allow.',
  ].join(' ');

const SETTLE =
  'Use the existing abnegate/zone-tests repository through the Scratch repo source, with its existing CI, tests run by check.sh, and deployment target none. That settles every decision and I confirm the plan: call finalize_project now.';

async function turnOver(
  page: Page,
): Promise<'question' | 'approval' | 'reply'> {
  let quiet = 0;
  for (;;) {
    const approval = page.locator('[data-testid="tool-deny"]').first();
    if (await approval.isVisible().catch(() => false)) return 'approval';
    const submit = page
      .locator(
        '[data-testid="question-card"] [data-testid="question-submit"]:enabled',
      )
      .last();
    if (await submit.isVisible().catch(() => false)) return 'question';
    const status = await page.locator('.message-status').count();
    const replied = await page.evaluate(() => {
      const users = document.querySelectorAll('.message-user');
      const last = users[users.length - 1];
      return Array.from(
        document.querySelectorAll('.message-assistant .message-content'),
      ).some(
        (reply) =>
          (!last ||
            Boolean(
              last.compareDocumentPosition(reply) &
                Node.DOCUMENT_POSITION_FOLLOWING,
            )) &&
          (reply.textContent ?? '').trim().length > 0,
      );
    });
    quiet = replied && status === 0 ? quiet + 1 : 0;
    if (quiet >= QUIET_POLLS) return 'reply';
    await page.waitForTimeout(2_000);
  }
}

async function answer(page: Page): Promise<string> {
  const card = page.locator('[data-testid="question-card"]').last();
  const text = (await card.innerText()).replace(/\s+/g, ' ');
  const groups = card.locator('.question-card-question');
  const count = await groups.count();
  for (let index = 0; index < count; index += 1) {
    const group = groups.nth(index);
    const prompt = (await group.innerText()).replace(/\s+/g, ' ');
    const radios = group.getByRole('radio');
    const boxes = group.getByRole('checkbox');
    const preferred = /repositor/i.test(prompt)
      ? /zone-tests|scratch|existing/i
      : /deploy|host|ship/i.test(prompt)
        ? /none|no deploy|not deploy/i
        : null;
    const choices = (await radios.count()) ? radios : boxes;
    let picked = false;
    if (preferred) {
      const match = choices.filter({ hasText: preferred });
      const labelled = group.getByLabel(preferred);
      if (await labelled.count()) {
        await labelled.first().check();
        picked = true;
      } else if (await match.count()) {
        await match.first().check();
        picked = true;
      }
    }
    if (!picked) {
      const recommended = group.locator('.question-card-choice', {
        has: page.locator('[data-testid="question-recommended"]'),
      });
      if (await recommended.count()) {
        await recommended.first().locator('input').first().check();
        picked = true;
      }
    }
    if (!picked && (await choices.count())) {
      const other = group.getByRole('radio', { name: 'Other' });
      if (await other.count()) {
        await other.check();
        await group.locator('[data-testid="question-free-text"]').fill(SETTLE);
      } else {
        await choices.first().check();
      }
      picked = true;
    }
    if (!picked) {
      const free = group.locator('[data-testid="question-free-text"]');
      if (await free.count()) await free.fill(SETTLE);
    }
  }
  await card.locator('[data-testid="question-submit"]').click();
  return text.slice(0, 400);
}

test.describe('auto project', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: DRIVE_TIMEOUT_MS + 3_600_000 });

  test('112: a brief becomes a planned project through the planner interview', async ({
    page,
  }) => {
    const s = stamp();
    const mark = logMark();
    await signIn(page);
    await page.goto('/projects');
    await page.getByRole('button', { name: /new project/i }).click();
    await page.locator('[data-testid="auto-project-button"]').click();
    const modal = page.locator('[data-testid="auto-project-modal"]');
    await expect(modal).toBeVisible();
    await modal.locator('[data-testid="auto-project-brief"]').fill(BRIEF(s));
    await shot(page, '112-auto-project-brief');
    await modal.getByRole('button', { name: 'Start the interview' }).click();
    await expect(page).toHaveURL(/\/chats\?id=/, { timeout: 60_000 });
    const chatId = new URL(page.url()).searchParams.get('id') ?? '';

    const interview: string[] = [];
    let projectId = '';
    for (let round = 0; round < INTERVIEW_ROUNDS && !projectId; round += 1) {
      const outcome = await turnOver(page);
      projectId =
        sql(
          `select coalesce(project_id::text, '') from chats where id = '${chatId}'`,
        )[0] ?? '';
      if (projectId) break;
      if (outcome === 'approval') {
        const call = (
          await page.locator('[data-testid="tool-call"]').last().innerText()
        ).replace(/\s+/g, ' ');
        interview.push(`denied a tool call: ${call.slice(0, 200)}`);
        await page.locator('[data-testid="tool-deny"]').first().click();
      } else if (outcome === 'question') {
        interview.push(`answered: ${await answer(page)}`);
      } else {
        const last = (
          await page
            .locator('.message-assistant')
            .last()
            .innerText()
            .catch(() => '')
        ).replace(/\s+/g, ' ');
        interview.push(`replied to: ${last.slice(0, 300)}`);
        await send(page, SETTLE);
      }
      await page.waitForTimeout(3_000);
    }
    plannedProjectId = projectId;
    await shot(page, '112-planner-chat-finalized');
    const planner = logLines(
      mark,
      /finalize_project|create_repository|Auto project/,
    ).slice(0, 10);
    const tasks = projectId
      ? sql(
          `select a.kind || ' | ' || t.title || ' | deps ' || coalesce(t.dependencies::text, '[]') from tasks t join task_automation a on a.task_id = t.id where a.project_id = '${projectId}' order by t.created_at`,
        )
      : [];
    record(112, {
      list: 'features',
      feature:
        'Auto project from a brief (planner interview and finalize_project)',
      result: projectId ? 'WORKS' : 'FAILS',
      cause: projectId
        ? undefined
        : `the planner did not call finalize_project within ${INTERVIEW_ROUNDS} rounds`,
      chat_id: chatId,
      project_id: projectId,
      brief: BRIEF(s),
      interview,
      planned_tasks: tasks,
      project_row: projectId
        ? sql(
            `select name || ' | auto ' || auto || ' | ' || coalesce(github_repo_url, 'no repo') || ' | token ' || (github_access_token is not null) from projects where id = '${projectId}'`,
          )
        : [],
      server_log: planner,
      screenshots: [
        '112-auto-project-brief.png',
        '112-planner-chat-finalized.png',
      ],
    });
    expect(projectId, interview.join(' || ')).not.toBe('');
  });

  test('114: the planned project runs, merges and reports unattended', async ({
    page,
  }) => {
    const projectId =
      plannedProjectId || process.env.ZONE_AUTO_PROJECT_ID || '';
    test.skip(
      !projectId,
      'run 112 first, or set ZONE_AUTO_PROJECT_ID to the project it planned',
    );
    await signIn(page);
    const read = async (): Promise<ProjectAutomation> => {
      const { status, body } = await api(
        'GET',
        `/api/projects/${projectId}/automation`,
        { token: await ownerToken() },
      );
      expect(status, JSON.stringify(body).slice(0, 300)).toBe(200);
      return ProjectAutomationSchema.parse(body);
    };
    const started = Date.now();
    const timeline: string[] = [];
    let last = '';
    let automation = await read();
    while (Date.now() - started < DRIVE_TIMEOUT_MS) {
      automation = await read();
      const line = `${automation.completed_at ? 'complete' : automation.paused_reason ? `paused: ${automation.paused_reason}` : 'running'} | ${automation.tasks
        .map(
          (t) =>
            `${t.title.slice(0, 40)}=${t.stage ?? t.status}${t.reason ? ` (${t.reason.slice(0, 80)})` : ''}`,
        )
        .join('; ')}`;
      if (line !== last) {
        timeline.push(
          `${Math.round((Date.now() - started) / 60_000)}m ${line}`,
        );
        last = line;
      }
      if (automation.completed_at || automation.paused_reason) break;
      if (
        automation.tasks.length &&
        automation.tasks.every((t) => t.stage === 'paused')
      )
        break;
      await page.waitForTimeout(30_000);
    }
    await page.goto('/projects');
    await page
      .getByText(/Greet|greet|greeting/)
      .first()
      .click()
      .catch(() => undefined);
    await page.waitForTimeout(2_000);
    await shot(page, '114-automation-panel');
    let updates: string[] = [];
    if (automation.updates_chat_id) {
      await page.goto(`/chats?id=${automation.updates_chat_id}`);
      await page.waitForTimeout(4_000);
      await shot(page, '114-updates-chat');
      const { body } = await api(
        'GET',
        `/api/chats/${automation.updates_chat_id}/messages`,
        {
          token: await ownerToken(),
        },
      );
      updates = (
        (body as { messages?: { role: string; content: string }[] }).messages ??
        []
      ).map(
        (m) => `${m.role}: ${m.content.replace(/\s+/g, ' ').slice(0, 400)}`,
      );
    }
    const ids = [
      projectId,
      ...sql(
        `select task_id from task_automation where project_id = '${projectId}'`,
      ),
    ];
    const driver = logLines(
      0,
      /Auto project|Repaired|[Cc]onflict|review|merged a pull request|checks/,
    )
      .filter((line) => !/DEBUG/.test(line))
      .filter((line) => ids.some((id) => line.includes(id)))
      .slice(0, 60)
      .map((line) => line.slice(0, 260));
    const merged = automation.tasks.filter(
      (t) => t.stage === 'merged' || t.status === 'complete',
    );
    const pulled = merged.filter((t) => t.pr_url);
    const works = Boolean(
      automation.completed_at && updates.length > 0 && pulled.length > 0,
    );
    const cause = automation.completed_at
      ? updates.length === 0
        ? 'the project completed but its updates chat is empty'
        : pulled.length === 0
          ? 'the project completed without merging a pull request'
          : undefined
      : automation.paused_reason
        ? `paused: ${automation.paused_reason}`
        : `not complete after ${Math.round((Date.now() - started) / 60_000)} minutes`;
    record(114, {
      list: 'features',
      feature:
        'Auto project driver: runs, PRs, CI wait, review, squash-merge, summary',
      result: works ? 'WORKS' : 'FAILS',
      cause,
      project_id: projectId,
      minutes: Math.round((Date.now() - started) / 60_000),
      counts: automation.counts,
      tasks: automation.tasks.map((t) => ({
        title: t.title,
        stage: t.stage,
        reason: t.reason,
        runs: t.runs,
        pr_url: t.pr_url,
      })),
      merged: merged.length,
      timeline,
      updates_chat: updates,
      server_log: driver,
      screenshots: ['114-automation-panel.png', '114-updates-chat.png'],
    });
    expect(works, `${cause} || ${timeline.join(' || ')}`).toBe(true);
  });
});
