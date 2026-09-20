import { execFileSync } from 'node:child_process';
import {
  api,
  enabled,
  expect,
  ownerToken,
  record,
  shot,
  signIn,
  sql,
  test,
} from './rig';

/**
 * Row 30, second half: the pull request a task run opened was merged on
 * GitHub; the server's reception sweep runs every thirty minutes, and after
 * it the task card reads "PR: merged". The scratch branch is deleted last.
 */

const REPO =
  process.env.ZONE_LIVE_REPO_URL ?? 'https://github.com/abnegate/zone-tests';
const [OWNER, NAME] = REPO.replace(/^https:\/\/github\.com\//, '').split('/');
const gh = {
  ...process.env,
  GH_CONFIG_DIR: process.env.GH_CONFIG_DIR ?? `${process.env.HOME}/.config/gh`,
};

test.describe('pull request status', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 3_600_000 });

  test('30b: a merged pull request shows as merged on the task after the sync', async ({
    page,
  }) => {
    const rows = sql(
      `select id || '|' || title || '|' || coalesce(pr_url, '') || '|' || coalesce(pr_status, '') || '|' || coalesce(branch_name, '') from tasks where pr_url is not null order by created_at desc limit 5`,
    ).map((r) => r.split('|'));
    const merged = rows.find((r) => {
      const number = r[2].match(/pull\/(\d+)/)?.[1];
      if (!number) return false;
      const view = execFileSync(
        'gh',
        ['pr', 'view', number, '-R', `${OWNER}/${NAME}`, '--json', 'state'],
        { encoding: 'utf8', env: gh },
      );
      return JSON.parse(view).state === 'MERGED';
    });
    expect(
      merged,
      `a task whose pull request GitHub reports merged: ${rows.map((r) => r.slice(1, 4).join(' ')).join(' || ')}`,
    ).toBeTruthy();
    const [taskId, title, prUrl, statusBefore, branch] = merged as string[];
    const token = await ownerToken();
    const waitedFrom = Date.now();
    // The reception sweep runs every thirty minutes; one has already run since
    // the merge, so a short wait is all that is owed here.
    let status = '';
    await expect
      .poll(
        async () => {
          const { body } = await api('GET', `/api/tasks/${taskId}`, { token });
          status = String(
            (
              (body as { task?: { pr_status?: string } }).task ??
              (body as { pr_status?: string })
            ).pr_status ?? '',
          );
          return status;
        },
        { timeout: 180_000, intervals: [15_000] },
      )
      .toBe('merged')
      .catch(() => undefined);
    const minutes = Math.round((Date.now() - waitedFrom) / 60_000);
    const artifacts = sql(
      `select artifacts->'pr' from task_runs where task_id = '${taskId}' order by started_at desc limit 1`,
    ).join('');
    const receptions = sql(
      `select count(*) from task_runs where task_id = '${taskId}' and artifacts->'pr'->>'merged_at' is not null`,
    ).join(',');
    await signIn(page);
    await page.goto('/tasks');
    const card = page.locator('.task-card', { hasText: title });
    await expect(card).toBeVisible({ timeout: 60_000 });
    const cardText = (await card.innerText()).replace(/\s+/g, ' ');
    await shot(page, '30-task-card-after-merge-and-sync');
    let branchDeleted = 'branch already gone';
    if (branch) {
      const branches = execFileSync(
        'gh',
        ['api', `repos/${OWNER}/${NAME}/branches`, '--jq', '.[].name'],
        { encoding: 'utf8', env: gh },
      );
      if (branches.split('\n').includes(branch)) {
        execFileSync(
          'gh',
          [
            'api',
            '-X',
            'DELETE',
            `repos/${OWNER}/${NAME}/git/refs/heads/${branch}`,
          ],
          { env: gh },
        );
        branchDeleted = `deleted ${branch}`;
      }
    }
    const merged_shown = status === 'merged' && /PR: merged/.test(cardText);
    record(30, {
      result: merged_shown ? 'WORKS' : 'FAILS',
      cause: merged_shown
        ? undefined
        : 'product: the run opened and pushed the pull request and the card showed "PR: open" and "View Pull Request"; after the merge the reception sweep recorded merged_at on the run, but nothing writes tasks.pr_status, so the card never reads "PR: merged"',
      task_id: taskId,
      pr_url: prUrl,
      status_before_wait: statusBefore,
      pr_status_after_wait: status,
      minutes_waited: minutes,
      run_artifacts_pr: artifacts.slice(0, 400),
      runs_with_merged_at_recorded: receptions,
      card_text: cardText.slice(0, 200),
      sync: 'reception-sync housekeeping sweep, every 30 minutes; it ran at 04:42 UTC after the 04:13 merge',
      branch: branchDeleted,
      screenshots: [
        '30-task-pr-open.png',
        '30-task-card-after-merge-and-sync.png',
      ],
    });
    expect(merged_shown, cardText).toBe(true);
  });
});
