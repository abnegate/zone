import type { Locator, Page } from '@playwright/test';
import { expect, test } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

const WORKSPACE_ID = '00000000-0000-0000-0000-000000000001';
const TIMESTAMP = '2026-11-01T09:00:00Z';

const existing = {
  id: 'source-jbs',
  name: 'jbs',
  source_type: 'filesystem',
  config: { base_path: '/srv/jbs', allow_writes: true },
  description: null,
  url: null,
  is_active: true,
  created_at: TIMESTAMP,
  updated_at: TIMESTAMP,
};

interface Requests {
  sources: Record<string, unknown>[];
  projects: Record<string, unknown>[];
}

async function mockWorkspace(page: Page, role: 'member' | 'viewer'): Promise<Requests> {
  const requests: Requests = { sources: [], projects: [] };

  await routeApi(page, /\/api\/workspaces\/[^/]+\/members$/, (route) =>
    route.fulfill({
      json: {
        members: [{ id: 'member-1', user_id: 'user-1', workspace_id: WORKSPACE_ID, role }],
      },
    })
  );
  await routeApi(page, /\/api\/sources\/types$/, (route) =>
    route.fulfill({
      json: {
        types: ['github', 'gitlab', 'filesystem', 'web', 'text'].map((id) => ({
          id,
          name: id,
          category: 'file',
          enabled: true,
        })),
      },
    })
  );
  await routeApi(page, /\/api\/workspaces\/[^/]+\/sources\/?$/, (route) => {
    if (route.request().method() === 'POST') {
      const body = route.request().postDataJSON() as Record<string, unknown>;
      requests.sources.push(body);
      return route.fulfill({
        status: 201,
        json: { source: { ...existing, ...body, id: 'source-zone' } },
      });
    }
    return route.fulfill({ json: { sources: [existing] } });
  });
  await routeApi(page, /\/api\/projects(\?.*)?$/, (route) => {
    if (route.request().method() === 'POST') {
      const body = route.request().postDataJSON() as Record<string, unknown>;
      requests.projects.push(body);
      return route.fulfill({
        status: 201,
        json: {
          success: true,
          project: {
            id: 'project-1',
            name: body.name,
            description: body.description ?? null,
            status: body.status,
            github_repo_url: null,
            source_id: body.source_id ?? null,
            auto: false,
            auto_paused_reason: null,
            auto_completed_at: null,
            created_at: TIMESTAMP,
            updated_at: TIMESTAMP,
          },
        },
      });
    }
    return route.fulfill({ json: { success: true, projects: [] } });
  });

  return requests;
}

async function openSourceStep(page: Page): Promise<Locator> {
  await page.goto('/projects');
  await page.getByRole('button', { name: 'New project' }).click();
  const project = page.locator('.project-wizard');
  await project.locator('#project-name').fill('Console');
  await project.locator('#project-description').fill('Operator console');
  await project.getByRole('button', { name: 'Next' }).click();
  await expect(project.getByRole('button', { name: /jbs/ })).toBeVisible();
  return project;
}

async function addFilesystemSource(nested: Locator): Promise<void> {
  await nested.getByRole('button', { name: /Filesystem/ }).click();
  await nested.getByRole('button', { name: 'Next' }).click();
  await nested.getByLabel('Directory Path').fill('/srv/zone');
  await nested.getByRole('button', { name: 'Next' }).click();
  await nested.getByLabel(/Display Name/).fill('zone');
}

test.describe('New project wizard: new source', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await setupAuth(page);
  });

  test('creates a source in place, selects it and keeps the project fields', async ({ page }) => {
    const requests = await mockWorkspace(page, 'member');
    const project = await openSourceStep(page);
    const card = project.getByRole('button', { name: /New source/ });
    const nested = page.getByRole('dialog', { name: 'Add Source' });

    await card.click();
    await expect(nested).toBeVisible();
    await expect(project).toHaveAttribute('inert', '');
    expect(await nested.evaluate((element) => element.contains(document.activeElement))).toBe(
      true
    );

    await page.keyboard.press('Escape');
    await expect(nested).toHaveCount(0);
    await expect(project).toBeVisible();
    await expect(project).not.toHaveAttribute('inert');
    await expect(card).toBeFocused();

    await card.click();
    await addFilesystemSource(nested);
    await nested.getByRole('button', { name: 'Add Source' }).click();

    await expect(nested).toHaveCount(0);
    const created = project.getByRole('button', { name: /zone filesystem/ });
    await expect(created).toHaveClass(/selected/);
    await expect(created).toBeFocused();
    await expect(project.getByRole('button', { name: /jbs/ })).not.toHaveClass(/selected/);
    expect(requests.sources).toEqual([
      expect.objectContaining({
        name: 'zone',
        source_type: 'filesystem',
        config: { base_path: '/srv/zone', allow_writes: true },
      }),
    ]);

    await project.getByRole('button', { name: 'Previous' }).click();
    await expect(project.locator('#project-name')).toHaveValue('Console');
    await expect(project.locator('#project-description')).toHaveValue('Operator console');
    await project.getByRole('button', { name: 'Next' }).click();
    await expect(created).toHaveClass(/selected/);
    await project.getByRole('button', { name: 'Next' }).click();
    await project.getByRole('button', { name: 'Create Project', exact: true }).click();

    await expect(project).toHaveCount(0);
    expect(requests.projects).toEqual([
      expect.objectContaining({
        name: 'Console',
        description: 'Operator console',
        source_id: 'source-zone',
      }),
    ]);
  });

  test('keeps the selection when source creation is cancelled', async ({ page }) => {
    const requests = await mockWorkspace(page, 'member');
    const project = await openSourceStep(page);
    const existingCard = project.getByRole('button', { name: /jbs/ });
    await existingCard.click();

    await project.getByRole('button', { name: /New source/ }).click();
    const nested = page.getByRole('dialog', { name: 'Add Source' });
    await nested.getByRole('button', { name: 'Cancel' }).click();

    await expect(nested).toHaveCount(0);
    await expect(existingCard).toHaveClass(/selected/);
    expect(requests.sources).toEqual([]);
  });

  test('offers no new source card to a viewer', async ({ page }) => {
    await mockWorkspace(page, 'viewer');
    const project = await openSourceStep(page);

    await expect(project.getByRole('button', { name: /New source/ })).toHaveCount(0);
  });
});

for (const width of [1280, 375]) {
  for (const theme of ['light', 'dark'] as const) {
    test(`new source flow at ${width}px in ${theme}`, async ({ browserName, context, page }) => {
      test.skip(browserName !== 'chromium', 'screenshots come from chromium');
      await page.setViewportSize({ width, height: 800 });
      await page.emulateMedia({ reducedMotion: 'reduce', colorScheme: theme });
      await blockServiceWorker(context);
      await mockCommonEndpoints(page);
      await page.addInitScript((value) => localStorage.setItem('manager_theme', value), theme);
      await setupAuth(page);
      await mockWorkspace(page, 'member');

      const shoot = async (state: string) => {
        expect(
          await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
          'the page never scrolls sideways'
        ).toBe(true);
        await page.mouse.move(0, 0);
        await page.screenshot({
          path: `screenshots/project-wizard-source-${theme}-${width}-${state}.png`,
          animations: 'disabled',
        });
      };

      const project = await openSourceStep(page);
      await expect(project.getByRole('button', { name: /New source/ })).toBeVisible();
      await shoot('step');

      await project.getByRole('button', { name: /New source/ }).click();
      const nested = page.getByRole('dialog', { name: 'Add Source' });
      await expect(nested.getByRole('button', { name: /Filesystem/ })).toBeVisible();
      await shoot('nested');

      await addFilesystemSource(nested);
      await nested.getByRole('button', { name: 'Add Source' }).click();
      await expect(nested).toHaveCount(0);
      await expect(project.getByRole('button', { name: /zone filesystem/ })).toHaveClass(/selected/);
      await project.getByRole('button', { name: /zone filesystem/ }).blur();
      await shoot('created');
    });
  }
}
