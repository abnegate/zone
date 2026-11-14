import type { Locator, Page } from '@playwright/test';
import { pullRequestKey } from '../src/features/pull-requests/utils/identity';
import { expect, test } from './fixtures';
import { chooseSelect } from './helpers/select';
import {
  checkLog,
  githubNow,
  inProgressPullRequest,
  notReadyPullRequest,
  populatedPullRequests,
  readyPullRequest,
  routeWorkspaceRole,
  setupAdminAuth,
  setupCommonRoutes,
} from './layout-fixtures';
import { blockServiceWorker, routeApi } from './test-utils';

const PAGE = '/pull-requests';
const CARD = 'pull-request-card';
const MERGE = 'Merge';
const SQUASH = 'Squash and merge';
const ALL_REPOSITORIES = 'All repositories';
const ALL_AUTHORS = 'All authors';
const FAILED_CHECK = notReadyPullRequest.check_details[0].name;
const MERGED_SHA = 'f'.repeat(40);
const REFUSAL = 'GitHub refused the merge because the pull request is no longer ready';
const REFUSAL_BLOCKER = 'A required review is missing';
const ALREADY_MERGED = 'The pull request was already merged.';
const PUSHED = '9'.repeat(40);
const HEAD_MOVED = 'New commits were pushed. Review them before merging.';
const UPDATE_PERMISSION = 'pull_requests:update';
const SHORT_SHA_LENGTH = 7;

function shortHead(head: string | null): string {
  return (head ?? '').slice(0, SHORT_SHA_LENGTH);
}

const detailPath = /\/pull-requests\/[^/]+\/\d+$/;
const diffPath = /\/pull-requests\/[^/]+\/\d+\/diff$/;
const commitsPath = /\/pull-requests\/[^/]+\/\d+\/commits$/;
const logPath = /\/pull-requests\/[^/]+\/\d+\/jobs\/\d+\/log$/;
const mergePattern = /\/api\/workspaces\/[^/]+\/pull-requests\/[^/]+\/\d+\/merge$/;
const listPattern = /\/api\/workspaces\/[^/]+\/pull-requests$/;

type Signed = 'admin' | 'viewer';

async function openPullRequests(
  page: Page,
  path = PAGE,
  signed: Signed = 'admin',
  withheld: readonly string[] = []
): Promise<void> {
  await page.clock.setFixedTime(githubNow);
  await setupCommonRoutes(page, true);
  if (signed === 'viewer') await routeWorkspaceRole(page, signed);
  await page.goto('/login');
  await page.evaluate(() => localStorage.clear());
  await setupAdminAuth(page, withheld);
  await page.goto(path);
  await expect(page.getByRole('heading', { name: 'Pull Requests', exact: true })).toBeVisible();
  await expect(page.getByTestId(CARD).first()).toBeVisible();
}

function track(page: Page, pattern: RegExp): string[] {
  const seen: string[] = [];
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (pattern.test(path)) seen.push(path);
  });
  return seen;
}

function card(page: Page, title: string): Locator {
  return page.getByTestId(CARD).filter({ hasText: title });
}

function details(page: Page, number: number): Locator {
  return page.getByRole('complementary', { name: `Pull request #${number}` });
}

async function openMerge(page: Page): Promise<Locator> {
  await card(page, readyPullRequest.title).click();
  const pane = details(page, readyPullRequest.number);
  await pane.getByRole('button', { name: MERGE, exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Merge pull request' });
  await expect(dialog).toBeVisible();
  return dialog;
}

test.describe('Pull requests', () => {
  test.use({ timezoneId: 'UTC' });

  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  test('groups open pull requests by readiness', async ({ page }) => {
    await openPullRequests(page);

    await expect(page.locator('.pull-request-group-label')).toHaveText([
      'Ready',
      'In progress',
      'Not ready',
    ]);
    for (const [group, pullRequest] of [
      ['Ready', readyPullRequest],
      ['In progress', inProgressPullRequest],
      ['Not ready', notReadyPullRequest],
    ] as const) {
      const section = page.getByRole('region', { name: group, exact: true });
      await expect(section.getByTestId(CARD)).toHaveCount(1);
      await expect(section.getByTestId(CARD)).toContainText(pullRequest.title);
      await expect(section.getByTestId(CARD)).toContainText(
        `${pullRequest.repository} #${pullRequest.number}`
      );
    }
    await expect(card(page, notReadyPullRequest.title)).toContainText('1 failed');
    await expect(page.getByText('Select a pull request', { exact: true })).toBeVisible();
  });

  test('narrows the list by readiness tab, repository and author', async ({ page }) => {
    await openPullRequests(page);
    const cards = page.getByTestId(CARD);
    const repository = page.getByRole('combobox', { name: 'Filter by repository' });
    const author = page.getByRole('combobox', { name: 'Filter by author' });

    const readiness = page.getByRole('radiogroup', { name: 'Filter by readiness' });
    await readiness.getByRole('radio', { name: 'Ready', exact: true }).click();
    await expect(cards).toHaveCount(1);
    await expect(cards).toContainText(readyPullRequest.title);

    await readiness.getByRole('radio', { name: 'All', exact: true }).click();
    await expect(cards).toHaveCount(3);

    await chooseSelect(repository, inProgressPullRequest.repository);
    await expect(cards).toHaveCount(2);
    await expect(card(page, readyPullRequest.title)).toHaveCount(0);

    await chooseSelect(author, readyPullRequest.author ?? '');
    await expect(cards).toHaveCount(0);
    await expect(page.getByText('No matching pull requests', { exact: true })).toBeVisible();

    await page.getByRole('button', { name: 'Clear filters', exact: true }).click();
    await expect(cards).toHaveCount(3);
    await expect(repository).toHaveText(ALL_REPOSITORIES);
    await expect(author).toHaveText(ALL_AUTHORS);
  });

  test('loads the detail first and the diff and logs only when asked', async ({ page }) => {
    const detailRequests = track(page, detailPath);
    const diffRequests = track(page, diffPath);
    const commitRequests = track(page, commitsPath);
    const logRequests = track(page, logPath);
    await openPullRequests(page);
    expect(detailRequests).toHaveLength(0);

    await card(page, notReadyPullRequest.title).click();
    const pane = details(page, notReadyPullRequest.number);
    await expect(pane).toBeVisible();
    await expect(card(page, notReadyPullRequest.title)).toHaveAttribute('aria-current', 'true');
    await expect(page).toHaveURL(`${PAGE}?id=${encodeURIComponent(pullRequestKey(notReadyPullRequest))}`);
    await expect(pane.getByRole('region', { name: 'Unresolved threads' })).toContainText(
      'Handle the error here.'
    );
    await expect(pane.getByRole('region', { name: 'Failed checks' })).toContainText(FAILED_CHECK);
    expect(detailRequests.length).toBeGreaterThan(0);
    expect(diffRequests).toHaveLength(0);
    expect(commitRequests).toHaveLength(0);
    expect(logRequests).toHaveLength(0);

    await pane.getByRole('tab', { name: 'Diff', exact: true }).click();
    await expect(pane.locator('.pull-request-diff-path')).toHaveText('src/main.rs');
    await expect(pane.getByText('new();')).toBeVisible();
    expect(diffRequests.length).toBeGreaterThan(0);
    expect(logRequests).toHaveLength(0);

    await pane.getByRole('tab', { name: 'Blockers', exact: true }).click();
    const showLogs = pane.getByRole('button', { name: 'Show logs', exact: true });
    await expect(showLogs).toHaveAttribute('aria-expanded', 'false');
    expect(logRequests).toHaveLength(0);
    await showLogs.click();
    await expect(pane.locator('.pull-request-log-text')).toHaveText(checkLog.log.text);
    await expect(pane.getByRole('button', { name: 'Hide logs', exact: true })).toHaveAttribute(
      'aria-expanded',
      'true'
    );
    expect(logRequests.length).toBeGreaterThan(0);
    expect(commitRequests).toHaveLength(0);
  });

  test('opens the pull request named by the id link and forgets it on close', async ({ page }) => {
    await openPullRequests(page, `${PAGE}?id=${pullRequestKey(inProgressPullRequest)}`);

    const pane = details(page, inProgressPullRequest.number);
    await expect(pane).toBeVisible();
    await expect(pane.getByRole('heading', { name: inProgressPullRequest.title })).toBeVisible();
    await expect(card(page, inProgressPullRequest.title)).toHaveAttribute('aria-current', 'true');

    await pane.getByRole('button', { name: 'Close pull request', exact: true }).click();
    await expect(pane).toHaveCount(0);
    await expect(page).toHaveURL(PAGE);
    await expect(page.getByText('Select a pull request', { exact: true })).toBeVisible();
  });

  test('ignores an id link that names no open pull request', async ({ page }) => {
    await openPullRequests(page, `${PAGE}?id=${readyPullRequest.source_id}:9999`);

    await expect(page.getByText('Select a pull request', { exact: true })).toBeVisible();
    await expect(page.getByRole('complementary', { name: /^Pull request #/ })).toHaveCount(0);
    await expect(page).toHaveURL(PAGE);
  });

  test('merges a ready pull request at the head it showed', async ({ page }) => {
    const merges: unknown[] = [];
    await openPullRequests(page);
    await routeApi(page, mergePattern, (route) => {
      merges.push(route.request().postDataJSON());
      return route.fulfill({
        status: 200,
        json: {
          merged: {
            number: readyPullRequest.number,
            sha: MERGED_SHA,
            admin: false,
            url: readyPullRequest.url,
          },
        },
      });
    });
    const merging = page.waitForRequest(
      (request) => request.method() === 'POST' && mergePattern.test(request.url())
    );

    const dialog = await openMerge(page);
    await expect(dialog).toContainText(`#${readyPullRequest.number}`);
    await expect(dialog).toContainText(readyPullRequest.head_ref);
    await chooseSelect(dialog.getByRole('combobox', { name: 'Merge method' }), SQUASH);
    await dialog.getByRole('button', { name: MERGE, exact: true }).click();
    await merging;

    await expect(
      page.getByText(`Merged ${readyPullRequest.repository} #${readyPullRequest.number}`)
    ).toBeVisible();
    await expect(dialog).toHaveCount(0);
    expect(merges).toEqual([{ expected_head: readyPullRequest.head, method: 'squash' }]);
  });

  test('holds the merge at the reviewed head when new commits land while the dialog is open', async ({
    page,
  }) => {
    const merges: unknown[] = [];
    await openPullRequests(page);
    await routeApi(page, mergePattern, (route) => {
      merges.push(route.request().postDataJSON());
      return route.fulfill({ status: 500, json: { error: 'Unexpected merge' } });
    });
    const dialog = await openMerge(page);
    await expect(dialog).toContainText(shortHead(readyPullRequest.head));

    const pushed = {
      ...populatedPullRequests,
      pull_requests: populatedPullRequests.pull_requests.map((pullRequest) =>
        pullRequest.number === readyPullRequest.number ? { ...pullRequest, head: PUSHED } : pullRequest
      ),
    };
    await routeApi(page, listPattern, (route) =>
      route.request().method() === 'GET'
        ? route.fulfill({ status: 200, json: pushed })
        : route.fallback()
    );
    const relisted = page.waitForResponse(
      (response) => listPattern.test(new URL(response.url()).pathname)
    );
    await page.evaluate(() => window.dispatchEvent(new Event('visibilitychange')));
    await relisted;

    await expect(dialog.getByRole('status')).toHaveText(HEAD_MOVED);
    await expect(dialog).toContainText(shortHead(readyPullRequest.head));
    const merge = dialog.getByRole('button', { name: MERGE, exact: true });
    await expect(merge).toBeDisabled();
    await merge.click({ force: true });
    await expect(dialog).toBeVisible();
    expect(merges).toEqual([]);
  });

  test('moves focus to the page heading after a merge', async ({ page }) => {
    await openPullRequests(page);
    await routeApi(page, mergePattern, (route) =>
      route.fulfill({
        status: 200,
        json: {
          merged: {
            number: readyPullRequest.number,
            sha: MERGED_SHA,
            admin: false,
            url: readyPullRequest.url,
          },
        },
      })
    );

    const dialog = await openMerge(page);
    await dialog.getByRole('button', { name: MERGE, exact: true }).click();

    await expect(dialog).toHaveCount(0);
    await expect(page.getByRole('heading', { name: 'Pull Requests', exact: true })).toBeFocused();
  });

  test('keeps the merge dialog open with the refusal when GitHub says no', async ({ page }) => {
    await openPullRequests(page);
    await routeApi(page, mergePattern, (route) =>
      route.fulfill({
        status: 409,
        json: {
          error: REFUSAL,
          code: 'not_ready',
          current_head: readyPullRequest.head,
          blockers: [{ code: 'review_missing', detail: REFUSAL_BLOCKER }],
        },
      })
    );

    const dialog = await openMerge(page);
    await dialog.getByRole('button', { name: MERGE, exact: true }).click();

    const alert = dialog.getByRole('alert');
    await expect(alert).toContainText(REFUSAL);
    await expect(alert.getByRole('listitem')).toHaveText([REFUSAL_BLOCKER]);
    await expect(dialog.getByRole('button', { name: MERGE, exact: true })).toBeEnabled();

    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await expect(details(page, readyPullRequest.number)).toBeVisible();
  });

  test('says the pull request was already merged and reads the list again', async ({ page }) => {
    await openPullRequests(page);
    await routeApi(page, mergePattern, (route) =>
      route.fulfill({
        status: 409,
        json: { success: false, error: ALREADY_MERGED, code: 'merged' },
      })
    );
    let listings = 0;
    page.on('request', (request) => {
      if (request.method() === 'GET' && listPattern.test(new URL(request.url()).pathname)) {
        listings += 1;
      }
    });

    const dialog = await openMerge(page);
    await dialog.getByRole('button', { name: MERGE, exact: true }).click();

    await expect(dialog.getByRole('alert')).toContainText(ALREADY_MERGED);
    await expect(dialog.getByRole('button', { name: MERGE, exact: true })).toBeDisabled();
    await expect.poll(() => listings).toBeGreaterThan(0);
  });

  test('asks for GitHub to be checked when a merge stops without an answer', async ({ page }) => {
    await openPullRequests(page);
    await routeApi(page, mergePattern, (route) =>
      route.fulfill({
        status: 500,
        json: { success: false, error: 'The merge stopped before it reported back.' },
      })
    );

    const dialog = await openMerge(page);
    await dialog.getByRole('button', { name: MERGE, exact: true }).click();

    const alert = dialog.getByRole('alert');
    await expect(alert).toContainText('Check the pull request on GitHub before trying again.');
    await expect(alert.getByRole('link', { name: 'Open the pull request on GitHub' })).toHaveAttribute(
      'href',
      readyPullRequest.url
    );
    await expect(dialog.getByRole('button', { name: MERGE, exact: true })).toBeDisabled();
  });

  test('offers only a ready pull request to merge', async ({ page }) => {
    await openPullRequests(page);

    await card(page, notReadyPullRequest.title).click();
    await expect(
      details(page, notReadyPullRequest.number).getByRole('button', { name: MERGE, exact: true })
    ).toBeDisabled();
  });

  test('offers a viewer no Merge button', async ({ page }) => {
    await openPullRequests(page, PAGE, 'viewer');

    await card(page, readyPullRequest.title).click();
    const pane = details(page, readyPullRequest.number);
    await expect(pane.getByRole('link', { name: 'Open on GitHub' })).toBeVisible();
    await expect(pane.getByRole('button', { name: MERGE, exact: true })).toHaveCount(0);
  });

  test('offers no Merge button without the update permission in a writable workspace', async ({
    page,
  }) => {
    await openPullRequests(page, PAGE, 'admin', [UPDATE_PERMISSION]);

    await card(page, readyPullRequest.title).click();
    const pane = details(page, readyPullRequest.number);
    await expect(pane.getByRole('link', { name: 'Open on GitHub' })).toBeVisible();
    await expect(pane.getByRole('button', { name: MERGE, exact: true })).toHaveCount(0);
  });
});
