import type { Locator, Page } from '@playwright/test';
import { expect, test } from './fixtures';
import { chooseSelect } from './helpers/select';
import {
  cratesLines,
  failedRelease,
  githubNow,
  prerelease,
  releaseOptions,
  releaseVerdict,
  routeWorkspaceRole,
  runningRelease,
  setupAdminAuth,
  setupCommonRoutes,
  yesterdayRelease,
} from './layout-fixtures';
import { blockServiceWorker, routeApi } from './test-utils';

const PAGE = '/releases';
const CARD = 'article.release-card';
const NEW_RELEASE = 'New release';
const REVIEW = 'Review';
const PUBLISH = 'Publish';
const RELOAD_OPTIONS = 'Reload options';
const LINE = 'Line';
const OPEN_RELEASES = 'Open releases on GitHub';
const STABLE_TAG = 'Publishes as a stable release';
const PRERELEASE_TAG = 'Publishes as a pre-release, because the tag has a pre-release suffix';
const VERSION_PROBLEM = 'Tags must be a version such as v1.2.3, v1.2.3-rc.1 or pkg/v1.2.3';
const [zoneOptions, , cratesOptions] = releaseOptions.sources;
const [dominantLine, otherLine] = cratesLines;
const NEXT_TAG = zoneOptions.next_tag;
const DIGEST = 'c'.repeat(64);
const TARGET = 'b'.repeat(40);
const MOVED = 'The latest tag moved since these options were loaded';
const INCLUDED_PULL = { number: 815, title: 'Show the release wizard notes' };
const PIPELINE_POLL_MS = 15_000;
const SETTLE_MS = 500;
const CREATE_PERMISSION = 'releases:create';
const REFRESH_LABEL = 'Refresh releases';

const listPath = /\/api\/workspaces\/[^/]+\/releases$/;
const optionsPath = /\/api\/workspaces\/[^/]+\/releases\/options$/;
const previewPattern = /\/api\/workspaces\/[^/]+\/releases\/preview$/;
const pipelinePattern = /\/api\/workspaces\/[^/]+\/releases\/[^/]+\/\d+\/pipeline$/;

const preview = {
  source_id: zoneOptions.source_id,
  repository: zoneOptions.repository,
  tag: NEXT_TAG,
  name: NEXT_TAG,
  body: `## What's Changed\n* ${INCLUDED_PULL.title}`,
  base_tag: zoneOptions.latest_tag,
  target: TARGET,
  prerelease: false,
  pulls: [{ ...INCLUDED_PULL, url: `${zoneOptions.repository_url}/pull/${INCLUDED_PULL.number}` }],
  digest: DIGEST,
};

const MARKER = '0b9f1c52-6d4e-4a8b-9f2e-1c3d5e7f9a0b';

const reconciliation = {
  success: false,
  error: 'The release was only partly created and needs reconciling on GitHub',
  code: 'release_manual_reconciliation_required',
  tag: NEXT_TAG,
  tag_object: 'e'.repeat(40),
  reference_created: true,
  release_id: 77,
  release_exists: false,
  marker: MARKER,
};

const created = {
  source_id: zoneOptions.source_id,
  id: 5,
  tag: NEXT_TAG,
  name: NEXT_TAG,
  url: `${zoneOptions.repository_url}/releases/tag/${NEXT_TAG}`,
  published_at: githubNow.toISOString(),
};

type Signed = 'admin' | 'viewer';

async function openReleases(
  page: Page,
  signed: Signed = 'admin',
  withheld: readonly string[] = []
): Promise<void> {
  await page.clock.setFixedTime(githubNow);
  await setupCommonRoutes(page, true);
  if (signed === 'viewer') await routeWorkspaceRole(page, signed);
  await page.goto('/login');
  await page.evaluate(() => localStorage.clear());
  await setupAdminAuth(page, withheld);
  await page.goto(PAGE);
  await expect(page.getByRole('heading', { name: 'Releases', exact: true })).toBeVisible();
  await expect(page.locator(CARD).first()).toBeVisible();
}

function count(page: Page, method: string, pattern: RegExp): () => number {
  let seen = 0;
  page.on('request', (request) => {
    if (request.method() === method && pattern.test(new URL(request.url()).pathname)) seen += 1;
  });
  return () => seen;
}

function releaseCard(page: Page, repository: string, tag: string): Locator {
  return page.getByRole('article', { name: `${repository} ${tag}`, exact: true });
}

async function openWizard(page: Page): Promise<Locator> {
  await page.getByRole('button', { name: NEW_RELEASE, exact: true }).click();
  const wizard = page.getByRole('dialog', { name: NEW_RELEASE });
  await expect(wizard).toBeVisible();
  await expect(wizard.getByLabel('Tag', { exact: true })).toHaveValue(NEXT_TAG);
  return wizard;
}

function footerButton(wizard: Locator, name: string): Locator {
  return wizard.locator('footer').getByRole('button', { name, exact: true });
}

test.describe('Releases', () => {
  test.use({ timezoneId: 'UTC' });

  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  test('groups releases by the day they were published', async ({ page }) => {
    await openReleases(page);

    await expect(page.locator('.release-day-label')).toHaveText(['Today', 'Yesterday', 'Sunday']);
    const days = page.locator('section.release-day');
    await expect(days.nth(0).locator(CARD)).toHaveCount(2);
    await expect(days.nth(0).getByText('2 releases', { exact: true })).toBeAttached();
    await expect(days.nth(1).locator(CARD)).toHaveCount(1);
    await expect(days.nth(2).locator(CARD)).toHaveCount(1);
    await expect(page.locator(`${CARD} .release-card-repository`)).toHaveText([
      runningRelease.repository,
      prerelease.repository,
      yesterdayRelease.repository,
      failedRelease.repository,
    ]);

    const beta = releaseCard(page, prerelease.repository, prerelease.tag);
    await expect(beta.getByText('Pre-release', { exact: true })).toBeVisible();
    await expect(beta.getByText(prerelease.tag, { exact: true })).toBeVisible();
    await expect(
      releaseCard(page, yesterdayRelease.repository, yesterdayRelease.tag).getByText('+1', {
        exact: true,
      })
    ).toBeVisible();
    await expect(releaseCard(page, failedRelease.repository, failedRelease.tag)).toContainText(
      failedRelease.blockers[0].detail
    );
  });

  test('expands a card to list the pull requests it shipped', async ({ page }) => {
    await openReleases(page);
    const running = releaseCard(page, runningRelease.repository, runningRelease.tag);
    const toggle = running.getByRole('button', { name: '3 PRs' });
    const pulls = running.locator('.release-card-pulls');

    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(pulls).toBeHidden();

    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    await expect(pulls.getByRole('link')).toHaveText(
      runningRelease.pulls.map((pull) => `#${pull.number}${pull.title}`)
    );

    await toggle.click();
    await expect(pulls).toBeHidden();
    await expect(
      releaseCard(page, failedRelease.repository, failedRelease.tag).getByText('No pull requests', {
        exact: true,
      })
    ).toBeVisible();
  });

  test('polls an active pipeline until it settles, then stops', async ({ page }) => {
    const settled = {
      ...runningRelease,
      pipeline: {
        ...runningRelease.pipeline,
        runs: runningRelease.pipeline.runs.map((run) => ({ ...run, state: 'succeeded' as const })),
      },
      state: 'succeeded' as const,
      succeeded: true,
      complete: true,
      blockers: [],
    };
    const pipelines = count(page, 'GET', pipelinePattern);
    let finished = false;
    await page.clock.install({ time: githubNow });
    await setupCommonRoutes(page, true);
    await routeApi(page, pipelinePattern, (route) =>
      route.fulfill({
        status: 200,
        json: releaseVerdict(finished ? settled : runningRelease),
      })
    );
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);
    await page.goto(PAGE);

    const running = releaseCard(page, runningRelease.repository, runningRelease.tag);
    await expect.poll(pipelines).toBeGreaterThan(0);
    await expect(running.locator('.release-card-summary')).toHaveText(
      runningRelease.blockers[0].detail
    );
    const watched = pipelines();
    await page.clock.fastForward(PIPELINE_POLL_MS);
    await expect.poll(pipelines).toBeGreaterThan(watched);

    finished = true;
    await page.clock.fastForward(PIPELINE_POLL_MS);
    await expect(running.locator('.release-card-summary')).toHaveText('Pipeline succeeded');
    await expect(running.getByRole('link', { name: 'Release: Succeeded' })).toBeVisible();
    const polled = pipelines();
    expect(polled).toBeGreaterThan(1);

    await page.clock.fastForward(PIPELINE_POLL_MS * 4);
    await page.waitForTimeout(SETTLE_MS);
    expect(pipelines()).toBe(polled);
  });

  test('previews the notes and then publishes the release', async ({ page }) => {
    await openReleases(page);
    const previews: unknown[] = [];
    const creations: unknown[] = [];
    await routeApi(page, previewPattern, (route) => {
      previews.push(route.request().postDataJSON());
      return route.fulfill({ status: 200, json: { preview } });
    });
    await routeApi(page, listPath, (route) => {
      if (route.request().method() !== 'POST') return route.fallback();
      creations.push(route.request().postDataJSON());
      return route.fulfill({ status: 201, json: { release: created } });
    });
    const listings = count(page, 'GET', listPath);

    const wizard = await openWizard(page);
    await expect(wizard).toContainText(`Previous tags: ${zoneOptions.previous_tags.join(', ')}`);
    await footerButton(wizard, REVIEW).click();

    await expect(wizard.getByRole('heading', { name: 'Notes' })).toBeVisible();
    await expect(wizard.locator('.release-wizard-facts')).toContainText(NEXT_TAG);
    await expect(wizard.locator('.release-wizard-facts')).toContainText(TARGET.slice(0, 7));
    await expect(wizard.locator('.release-wizard-pulls')).toContainText(INCLUDED_PULL.title);
    expect(previews).toEqual([
      {
        source_id: zoneOptions.source_id,
        tag: NEXT_TAG,
        expected_latest_tag: zoneOptions.latest_tag,
      },
    ]);
    expect(creations).toHaveLength(0);

    await footerButton(wizard, PUBLISH).click();
    await expect(page.getByText(`Published ${NEXT_TAG}`)).toBeVisible();
    await expect(wizard).toHaveCount(0);
    expect(creations).toEqual([
      {
        source_id: zoneOptions.source_id,
        tag: NEXT_TAG,
        expected_latest_tag: zoneOptions.latest_tag,
        prerelease: false,
        digest: DIGEST,
      },
    ]);
    await expect.poll(listings).toBeGreaterThan(0);
  });

  test('offers to reload the options when GitHub refuses a stale preview', async ({ page }) => {
    await openReleases(page);
    await routeApi(page, previewPattern, (route) =>
      route.fulfill({ status: 409, json: { error: MOVED, code: 'latest_tag_moved' } })
    );
    const options = count(page, 'GET', optionsPath);

    const wizard = await openWizard(page);
    const loaded = options();
    await footerButton(wizard, REVIEW).click();

    const alert = wizard.getByRole('alert');
    await expect(alert).toContainText(MOVED);
    const reload = alert.getByRole('button', { name: RELOAD_OPTIONS, exact: true });
    await expect(reload).toBeVisible();
    await expect(wizard.getByRole('heading', { name: 'Notes' })).toHaveCount(0);

    await reload.click();
    await expect.poll(options).toBeGreaterThan(loaded);
    await expect(wizard.getByRole('alert')).toHaveCount(0);
    await expect(wizard.getByLabel('Tag', { exact: true })).toHaveValue(NEXT_TAG);
  });

  test('offers each line of a prefixed repository and previews against its own latest tag', async ({
    page,
  }) => {
    await openReleases(page);
    const previews: unknown[] = [];
    await routeApi(page, previewPattern, (route) => {
      const request = route.request().postDataJSON() as { tag: string };
      previews.push(request);
      return route.fulfill({
        status: 200,
        json: {
          preview: {
            ...preview,
            source_id: cratesOptions.source_id,
            repository: cratesOptions.repository,
            tag: request.tag,
            name: request.tag,
          },
        },
      });
    });

    const wizard = await openWizard(page);
    await chooseSelect(
      wizard.getByRole('combobox', { name: 'Repository' }),
      cratesOptions.repository
    );
    const line = wizard.getByRole('combobox', { name: LINE });
    await expect(line).toHaveText(dominantLine.prefix ?? '');
    await expect(wizard.getByLabel('Tag', { exact: true })).toHaveValue(dominantLine.next_tag);
    await line.click();
    const listbox = page.getByRole('listbox');
    await expect(listbox.getByRole('option')).toHaveText(
      cratesLines.map((each) => each.prefix ?? '')
    );
    await listbox.getByRole('option', { name: otherLine.prefix ?? '', exact: true }).click();
    await expect(line).toHaveText(otherLine.prefix ?? '');
    await expect(wizard.getByLabel('Tag', { exact: true })).toHaveValue(otherLine.next_tag);
    await expect(wizard).toContainText(`Previous tags: ${otherLine.previous_tags.join(', ')}`);
    await expect(wizard.getByText(STABLE_TAG)).toBeVisible();
    await footerButton(wizard, REVIEW).click();

    await expect(wizard.getByRole('heading', { name: 'Notes' })).toBeVisible();
    expect(previews).toEqual([
      {
        source_id: cratesOptions.source_id,
        tag: otherLine.next_tag,
        expected_latest_tag: otherLine.latest_tag,
      },
    ]);
  });

  test('publishes a pre-release tag as a pre-release', async ({ page }) => {
    await openReleases(page);
    const candidate = 'v1.5.0-rc.1';
    const creations: unknown[] = [];
    await routeApi(page, previewPattern, (route) =>
      route.fulfill({
        status: 200,
        json: { preview: { ...preview, tag: candidate, name: candidate, prerelease: true } },
      })
    );
    await routeApi(page, listPath, (route) => {
      if (route.request().method() !== 'POST') return route.fallback();
      creations.push(route.request().postDataJSON());
      return route.fulfill({ status: 201, json: { release: { ...created, tag: candidate } } });
    });

    const wizard = await openWizard(page);
    await expect(wizard.getByRole('checkbox', { name: 'Pre-release' })).toHaveCount(0);
    await wizard.getByLabel('Tag', { exact: true }).fill(candidate);
    await expect(wizard.getByText(PRERELEASE_TAG)).toBeVisible();
    await footerButton(wizard, REVIEW).click();
    await expect(wizard.locator('.release-wizard-facts dd').last()).toHaveText('Yes');

    await footerButton(wizard, PUBLISH).click();
    await expect(wizard).toHaveCount(0);
    expect(creations).toEqual([
      {
        source_id: zoneOptions.source_id,
        tag: candidate,
        expected_latest_tag: zoneOptions.latest_tag,
        prerelease: true,
        digest: DIGEST,
      },
    ]);
  });

  test('refuses a tag that is not a version before asking the server', async ({ page }) => {
    await openReleases(page);
    const previews = count(page, 'POST', previewPattern);

    const wizard = await openWizard(page);
    await wizard.getByLabel('Tag', { exact: true }).fill('release-2026');

    await expect(wizard.getByText(VERSION_PROBLEM)).toBeVisible();
    await expect(footerButton(wizard, REVIEW)).toBeDisabled();
    expect(previews()).toBe(0);
  });

  test('explains a release that needs reconciling and reads the list again', async ({ page }) => {
    await openReleases(page);
    await routeApi(page, previewPattern, (route) =>
      route.fulfill({ status: 200, json: { preview } })
    );
    await routeApi(page, listPath, (route) => {
      if (route.request().method() !== 'POST') return route.fallback();
      return route.fulfill({ status: 409, json: reconciliation });
    });
    const listings = count(page, 'GET', listPath);

    const wizard = await openWizard(page);
    await footerButton(wizard, REVIEW).click();
    await footerButton(wizard, PUBLISH).click();

    const alert = wizard.getByRole('alert');
    await expect(alert).toContainText('The release was only partly created');
    await expect(alert.getByRole('listitem')).toHaveText([
      `Tag object eeeeeee was written for ${NEXT_TAG}.`,
      `refs/tags/${NEXT_TAG} points at it.`,
      'Draft release 77 exists but was not confirmed published.',
      `Search GitHub for zone-release:${MARKER} to find what was written.`,
    ]);
    await expect(alert.getByRole('link', { name: OPEN_RELEASES })).toHaveAttribute(
      'href',
      `${zoneOptions.repository_url}/releases`
    );
    await expect.poll(listings).toBeGreaterThan(0);
  });

  test('asks for GitHub to be checked when a publish stops without an answer', async ({ page }) => {
    await openReleases(page);
    await routeApi(page, previewPattern, (route) =>
      route.fulfill({ status: 200, json: { preview } })
    );
    await routeApi(page, listPath, (route) => {
      if (route.request().method() !== 'POST') return route.fallback();
      return route.fulfill({
        status: 500,
        json: { success: false, error: 'The release stopped before it reported back.' },
      });
    });
    const listings = count(page, 'GET', listPath);

    const wizard = await openWizard(page);
    await footerButton(wizard, REVIEW).click();
    await footerButton(wizard, PUBLISH).click();

    const alert = wizard.getByRole('alert');
    await expect(alert).toContainText('Check the releases on GitHub before trying again.');
    await expect(alert.getByRole('link', { name: OPEN_RELEASES })).toBeVisible();
    await expect.poll(listings).toBeGreaterThan(0);
  });

  test('offers a viewer no New release button', async ({ page }) => {
    await openReleases(page, 'viewer');

    await expect(page.locator(CARD)).toHaveCount(4);
    await expect(page.getByRole('button', { name: NEW_RELEASE })).toHaveCount(0);
    await expect(page.getByRole('dialog')).toHaveCount(0);
  });

  test('offers no New release button without the create permission in a writable workspace', async ({
    page,
  }) => {
    await openReleases(page, 'admin', [CREATE_PERMISSION]);

    await expect(page.locator(CARD)).toHaveCount(4);
    await expect(page.getByRole('button', { name: REFRESH_LABEL })).toBeVisible();
    await expect(page.getByRole('button', { name: NEW_RELEASE })).toHaveCount(0);
  });
});
