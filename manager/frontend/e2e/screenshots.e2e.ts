import { test, expect } from './fixtures';
import type { Page } from '@playwright/test';
import { blockServiceWorker, routeApi } from './test-utils';
import { mockTasks, setupAdminAuth, setupCommonRoutes, tasksNow } from './layout-fixtures';

const runScreenshots = process.env.RUN_SCREENSHOTS === 'true';
const describeScreenshots = runScreenshots ? test.describe : test.describe.skip;
const PNG_BASE64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
const TASK_CARD = '.task-card:not(.board-skeleton-card)';
const SKELETON_CARD = '.skeleton-card, .board-skeleton-card';
const PHONE = { width: 375, height: 812 };

const CardAction = {
  rename: 'Rename',
  remove: 'Delete',
} as const;

type CardAction = (typeof CardAction)[keyof typeof CardAction];

async function openTasks(page: Page, populated: boolean): Promise<void> {
  await page.clock.setFixedTime(tasksNow);
  await setupCommonRoutes(page, populated);
  await page.goto('/login');
  await page.evaluate(() => localStorage.clear());
  await setupAdminAuth(page);

  await page.goto('/tasks');
  await page.waitForLoadState('domcontentloaded');
  await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
}

async function chooseCardAction(page: Page, title: string, action: CardAction): Promise<void> {
  const trigger = page.getByRole('button', { name: `Actions for ${title}`, exact: true });
  await trigger.hover();
  await trigger.click();
  await page.getByRole('menuitem', { name: action, exact: true }).click();
}

async function fontsLoaded(page: Page): Promise<void> {
  await page.waitForFunction(() =>
    Boolean(
      document.querySelector<HTMLLinkElement>('link[rel="stylesheet"][href*="fonts.googleapis.com"]')?.sheet
    )
  );
  await page.evaluate(async () => {
    await document.fonts.load('16px Nunito');
    await new Promise(requestAnimationFrame);
    await document.fonts.ready;
  });
}

async function capturePage(
  page: Page,
  options: Parameters<Page['screenshot']>[0]
): Promise<void> {
  const populated: Record<string, string> = {
    'screenshots/models-populated.png': '.model-item',
    'screenshots/chats-populated.png': '.chat-item',
    'screenshots/projects-populated.png': '.project-card',
    'screenshots/tasks-populated.png': TASK_CARD,
    'screenshots/tasks-populated-end.png': TASK_CARD,
    'screenshots/tasks-populated-375.png': TASK_CARD,
    'screenshots/dark-tasks.png': TASK_CARD,
    'screenshots/sources-populated.png': '.source-card',
    'screenshots/wiki-populated.png': '.knowledge-card',
  };
  const content = populated[options?.path ?? ''];
  if (content) {
    await expect(page.locator(content).first()).toBeVisible();
    await expect(page.locator(SKELETON_CARD)).toHaveCount(0);
  }
  if (options?.path?.includes('modal-'))
    await expect(page.getByRole('dialog')).toBeVisible();
  await fontsLoaded(page);
  await page.screenshot({ ...options, animations: 'disabled' });
}

// Helper to verify no errors on page
async function verifyNoErrors(page: Page) {
  // Check for validation error text
  const validationError = page.locator('text=/Validation failed/i');
  await expect(validationError).not.toBeVisible({ timeout: 1000 });

  // Check for unexpected token errors
  const tokenError = page.locator('text=/Unexpected token/i');
  await expect(tokenError).not.toBeVisible({ timeout: 1000 });

  // Check for generic error messages
  const genericError = page
    .locator('[class*="error"]')
    .filter({ hasText: /failed|error/i });
  const errorCount = await genericError.count();
  if (errorCount > 0) {
    const errorText = await genericError.first().textContent();
    if (
      errorText &&
      (errorText.includes('Validation failed') ||
        errorText.includes('Unexpected token'))
    ) {
      throw new Error(`Page has error: ${errorText}`);
    }
  }
}

describeScreenshots('Screenshots - Public Pages', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    // Clear localStorage before page loads
    await page.addInitScript(() => {
      localStorage.clear();
    });
    // Mock auth refresh to fail immediately so the page doesn't hang
    await routeApi(page, '**/api/auth/refresh', (route) => {
      route.fulfill({
        status: 401,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Unauthorized' }),
      });
    });
  });

  test('Login page', async ({ page }) => {
    await page.goto('/login');
    await page.waitForLoadState('domcontentloaded');

    await expect(page.locator('input[type="email"]')).toBeVisible({
      timeout: 10000,
    });
    await capturePage(page, { path: 'screenshots/login.png', fullPage: true });
  });

  test('Register page', async ({ page }) => {
    await page.goto('/register');
    await page.waitForLoadState('domcontentloaded');

    await expect(page.locator('input[type="email"]')).toBeVisible({
      timeout: 10000,
    });
    await capturePage(page, {
      path: 'screenshots/register.png',
      fullPage: true,
    });
  });

  test('Forgot password page', async ({ page }) => {
    await page.goto('/forgot-password');
    await page.waitForLoadState('domcontentloaded');

    await expect(page.locator('input[type="email"]')).toBeVisible({
      timeout: 10000,
    });
    await capturePage(page, {
      path: 'screenshots/forgot-password.png',
      fullPage: true,
    });
  });

  test('Unauthorized page', async ({ page }) => {
    await page.goto('/unauthorized');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByText('Access Denied')).toBeVisible();
    await capturePage(page, {
      path: 'screenshots/unauthorized.png',
      fullPage: true,
    });
  });
});

describeScreenshots('Screenshots - Empty States', () => {
  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  test('Models page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/models');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/models-empty.png',
      fullPage: true,
    });
  });

  test('Chats page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/chats');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/chats-empty.png',
      fullPage: true,
    });
  });

  test('Projects page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/projects');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/projects-empty.png',
      fullPage: true,
    });
  });

  test('Tasks page (empty)', async ({ page }) => {
    await openTasks(page, false);
    await expect(page.locator(SKELETON_CARD)).toHaveCount(0);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/tasks-empty.png',
      fullPage: true,
    });
  });

  test('Sources page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/sources');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/sources-empty.png',
      fullPage: true,
    });
  });

  test('Wiki page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/wiki');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/wiki-empty.png',
      fullPage: true,
    });
  });

  test('Settings sessions (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/settings/you/sessions');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation', { name: 'Settings', exact: true })).toBeVisible({
      timeout: 10000,
    });
    await expect(page.getByRole('region', { name: 'Sessions', exact: true })).toBeVisible();
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/sessions-empty.png',
      fullPage: true,
    });
  });

  test('Settings organization AI defaults (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/settings/organization/ai');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation', { name: 'Settings', exact: true })).toBeVisible({
      timeout: 10000,
    });
    await expect(page.getByRole('region', { name: 'AI defaults', exact: true })).toBeVisible();
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/org-settings-empty.png',
      fullPage: true,
    });
  });

  test('Settings workspace general (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/settings/workspace/general');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation', { name: 'Settings', exact: true })).toBeVisible({
      timeout: 10000,
    });
    await expect(page.getByRole('region', { name: 'General', exact: true })).toBeVisible();
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/workspace-settings-empty.png',
      fullPage: true,
    });
  });
});

describeScreenshots('Screenshots - Populated States', () => {
  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  test('Models page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/models');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/models-populated.png',
      fullPage: true,
    });
  });

  test('Qwen training pairs in light, dark, and narrow layouts', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await routeApi(page, '**/api/models/train/bases', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([
          { id: 'qwen-image-edit', label: 'Qwen Image Edit', edit: true },
          { id: 'flux-schnell', label: 'FLUX.1 Schnell', edit: false },
        ]),
      });
    });
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/models');
    await page.waitForLoadState('domcontentloaded');
    await page.getByRole('tab', { name: 'Train' }).click();
    const panel = page
      .locator('.card')
      .filter({ has: page.getByRole('heading', { name: 'Train a LoRA' }) });
    await expect(panel.getByLabel('Base')).toContainText('Qwen Image Edit');
    await panel.getByLabel('Name', { exact: true }).fill('studio-edit');
    await panel.getByLabel('Trigger word').fill('zne subject');
    await panel.getByLabel('Target images').setInputFiles([
      {
        name: 'blue-studio-target.png',
        mimeType: 'image/png',
        buffer: Buffer.from(PNG_BASE64, 'base64'),
      },
      {
        name: 'garden-target.png',
        mimeType: 'image/png',
        buffer: Buffer.from(PNG_BASE64, 'base64'),
      },
    ]);
    await panel
      .getByLabel('Reference image for target 1: blue-studio-target.png')
      .setInputFiles({
        name: 'blue-studio-reference.png',
        mimeType: 'image/png',
        buffer: Buffer.from(PNG_BASE64, 'base64'),
      });
    await panel
      .getByLabel('Instruction for target 1: blue-studio-target.png')
      .fill('Change the background to a blue studio');
    await expect(panel.getByText('Reference: blue-studio-reference.png')).toBeVisible();
    await expect(panel.getByText('Choose one reference image for this target.')).toBeVisible();

    await page.setViewportSize({ width: 1440, height: 1000 });
    await panel.evaluate((element) => element.scrollTo({ top: 0 }));
    await page.evaluate(() => {
      window.scrollTo({ top: 0 });
      document.documentElement.setAttribute('data-theme', 'light');
    });
    await capturePage(page, {
      path: 'screenshots/models-train-qwen-light.png',
      fullPage: true,
    });

    await page.evaluate(() => {
      document.documentElement.setAttribute('data-theme', 'dark');
    });
    await capturePage(page, {
      path: 'screenshots/models-train-qwen-dark.png',
      fullPage: true,
    });

    await page.setViewportSize({ width: 390, height: 1600 });
    await panel.evaluate((element) => element.scrollTo({ top: 0 }));
    await page.evaluate(() => {
      window.scrollTo({ top: 0 });
      document.documentElement.setAttribute('data-theme', 'light');
    });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
      true
    );
    await capturePage(page, {
      path: 'screenshots/models-train-qwen-narrow.png',
      fullPage: true,
    });
  });

  test('Qwen training form while a run is in progress', async ({ page }) => {
    let announceStart = (): void => {};
    let finishTraining = (): void => {};
    const started = new Promise<void>((resolve) => {
      announceStart = resolve;
    });
    const finish = new Promise<void>((resolve) => {
      finishTraining = resolve;
    });
    await setupCommonRoutes(page, true);
    await routeApi(page, '**/api/models/train/bases', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([{ id: 'qwen-image-edit', label: 'Qwen Image Edit', edit: true }]),
      });
    });
    await routeApi(page, /\/api\/models\/train\/uploads(\/[^/?]+)?$/, (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ id: 'studio-upload' }),
      });
    });
    await routeApi(page, /\/api\/models\/train(\?.*)?$/, async (route) => {
      if (route.request().method() === 'GET') {
        await route.fulfill({ status: 204, body: '' });
        return;
      }
      announceStart();
      await finish;
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ filename: 'studio-edit.safetensors', quality: null }),
      });
    });
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/models');
    await page.waitForLoadState('domcontentloaded');
    await page.getByRole('tab', { name: 'Train' }).click();
    const panel = page
      .locator('.card')
      .filter({ has: page.getByRole('heading', { name: 'Train a LoRA' }) });
    await panel.getByLabel('Name', { exact: true }).fill('studio-edit');
    await panel.getByLabel('Target images').setInputFiles({
      name: 'blue-studio-target.png',
      mimeType: 'image/png',
      buffer: Buffer.from(PNG_BASE64, 'base64'),
    });
    await panel
      .getByLabel('Reference image for target 1: blue-studio-target.png')
      .setInputFiles({
        name: 'blue-studio-reference.png',
        mimeType: 'image/png',
        buffer: Buffer.from(PNG_BASE64, 'base64'),
      });
    await panel
      .getByLabel('Instruction for target 1: blue-studio-target.png')
      .fill('Change the background to a blue studio');
    await panel.getByRole('button', { name: 'Train' }).click();
    await started;
    await expect(panel.locator('form')).toHaveAttribute('aria-busy', 'true');

    await page.setViewportSize({ width: 1440, height: 1000 });
    await panel.evaluate((element) => element.scrollTo({ top: 0 }));
    await page.evaluate(() => {
      window.scrollTo({ top: 0 });
      document.documentElement.setAttribute('data-theme', 'light');
    });
    await capturePage(page, {
      path: 'screenshots/models-train-qwen-busy.png',
      fullPage: true,
    });

    finishTraining();
    await expect(panel.locator('form')).toHaveAttribute('aria-busy', 'false');
  });

  test('Chats page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/chats');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/chats-populated.png',
      fullPage: true,
    });
  });

  test('Projects page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/projects');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/projects-populated.png',
      fullPage: true,
    });
  });

  test('Tasks page (populated)', async ({ page }) => {
    await openTasks(page, true);
    await verifyNoErrors(page);
    await capturePage(page, {
      path: 'screenshots/tasks-populated.png',
      fullPage: true,
    });

    const board = page.getByRole('region', { name: 'Tasks' });
    await board.evaluate((element) => element.scrollTo({ left: element.scrollWidth }));
    await expect(board.locator('[data-stage="cancelled"]')).toBeInViewport();
    await capturePage(page, {
      path: 'screenshots/tasks-populated-end.png',
      fullPage: true,
    });
  });

  test('Tasks page (populated, 375)', async ({ page }) => {
    await page.setViewportSize(PHONE);
    await openTasks(page, true);
    await verifyNoErrors(page);
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)
    ).toBe(true);
    await capturePage(page, {
      path: 'screenshots/tasks-populated-375.png',
      fullPage: true,
    });
  });

  const TASK_TITLE_VIEWS = [
    { name: 'light', theme: 'light', width: 1280, height: 800 },
    { name: 'dark', theme: 'dark', width: 1280, height: 800 },
    { name: '375', theme: 'light', width: 375, height: 812 },
  ] as const;

  for (const view of TASK_TITLE_VIEWS) {
    test(`Task title, optional in the wizard and renamed inline (${view.name})`, async ({
      page,
    }) => {
      await routeApi(page, '**/api/tasks/task-1', (route) =>
        route.fulfill({ status: 200, json: { task: mockTasks[0] } })
      );
      await page.setViewportSize({ width: view.width, height: view.height });
      await openTasks(page, true);
      await page.evaluate((theme) => {
        document.documentElement.setAttribute('data-theme', theme);
      }, view.theme);
      await expect(page.locator(TASK_CARD).first()).toBeVisible();

      await page.getByRole('button', { name: /New task/ }).click();
      const wizard = page.getByRole('dialog', { name: 'New Task' });
      await wizard.locator('.project-selection-option').first().click();
      await wizard.getByRole('button', { name: 'Next' }).click();
      await wizard
        .getByLabel('Description')
        .fill('Users on Safari are sent back to the login page every time they sign in.');
      await expect(wizard.getByRole('button', { name: 'Next' })).toBeEnabled();
      await capturePage(page, { path: `screenshots/task-title-wizard-${view.name}.png` });
      await wizard.getByRole('button', { name: 'Cancel' }).click();
      await expect(wizard).toHaveCount(0);

      await page.goto('/tasks?task=task-1');
      const rename = page.getByRole('button', { name: `Rename ${mockTasks[0].title}` });
      await expect(rename).toBeVisible();
      await page.evaluate((theme) => {
        document.documentElement.setAttribute('data-theme', theme);
      }, view.theme);
      await rename.click();
      const title = page.getByRole('textbox', { name: 'Task title' });
      await expect(title).toBeFocused();
      await title.fill('Sign in with Google and GitHub');
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)
      ).toBe(true);
      await capturePage(page, { path: `screenshots/task-title-rename-${view.name}.png` });
      await title.press('Escape');

      await page.getByRole('button', { name: 'Close' }).click();
      await expect(page.getByRole('region', { name: 'Tasks' })).toBeVisible();
      await chooseCardAction(page, mockTasks[0].title, CardAction.rename);
      await expect(page.getByRole('textbox', { name: 'Task title' })).toBeFocused();
      await capturePage(page, { path: `screenshots/task-title-rename-list-${view.name}.png` });
    });
  }

  test('Sources page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/sources');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/sources-populated.png',
      fullPage: true,
    });
  });

  test('Wiki page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/wiki');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/wiki-populated.png',
      fullPage: true,
    });
  });

  test('Settings sessions (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/settings/you/sessions');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation', { name: 'Settings', exact: true })).toBeVisible({
      timeout: 10000,
    });
    await expect(page.getByRole('region', { name: 'Sessions', exact: true })).toBeVisible();
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/sessions-populated.png',
      fullPage: true,
    });
  });

  test('Settings organization AI defaults (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/settings/organization/ai');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation', { name: 'Settings', exact: true })).toBeVisible({
      timeout: 10000,
    });
    await expect(page.getByRole('region', { name: 'AI defaults', exact: true })).toBeVisible();
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/org-settings-populated.png',
      fullPage: true,
    });
  });

  test('Settings workspace general (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/settings/workspace/general');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation', { name: 'Settings', exact: true })).toBeVisible({
      timeout: 10000,
    });
    await expect(page.getByRole('region', { name: 'General', exact: true })).toBeVisible();
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/workspace-settings-populated.png',
      fullPage: true,
    });
  });

  test('Wiki search', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/wiki');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, { path: 'screenshots/search.png', fullPage: true });
  });
});

describeScreenshots('Screenshots - Dark Mode', () => {
  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  async function enableDarkMode(page: Page) {
    await expect(page.locator('#root')).not.toBeEmpty();
    await expect(page.locator('.initial-loader')).toHaveCount(0);
    await page.evaluate(() => {
      document.documentElement.setAttribute('data-theme', 'dark');
    });
    await page.waitForTimeout(100);
  }

  test('Login page (dark)', async ({ page }) => {
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await enableDarkMode(page);

    await expect(page.locator('input[type="email"]')).toBeVisible({
      timeout: 10000,
    });
    await capturePage(page, {
      path: 'screenshots/dark-login.png',
      fullPage: true,
    });
  });

  test('Models page (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/models');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await enableDarkMode(page);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/dark-models.png',
      fullPage: true,
    });
  });

  test('Projects page (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/projects');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await enableDarkMode(page);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/dark-projects.png',
      fullPage: true,
    });
  });

  test('Tasks page (dark)', async ({ page }) => {
    await openTasks(page, true);
    await enableDarkMode(page);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/dark-tasks.png',
      fullPage: true,
    });
  });

  test('Sources page (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/sources');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await enableDarkMode(page);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/dark-sources.png',
      fullPage: true,
    });
  });

  test('Wiki page (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/wiki');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await enableDarkMode(page);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/dark-wiki.png',
      fullPage: true,
    });
  });

  test('Chats page (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/chats');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await enableDarkMode(page);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/dark-chats.png',
      fullPage: true,
    });
  });
});

describeScreenshots('Screenshots - Modals', () => {
  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  test('New Project modal', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/projects');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });

    // Click new project button
    const newProjectBtn = page
      .locator('button')
      .filter({ hasText: /New Project/i });
    await expect(newProjectBtn).toBeVisible();
    await newProjectBtn.click();
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/modal-new-project.png',
      fullPage: true,
    });
  });

  test('New Task modal', async ({ page }) => {
    await openTasks(page, true);
    await expect(page.locator(TASK_CARD).first()).toBeVisible();

    const newTask = page.locator('button').filter({ hasText: /New Task/i });
    await expect(newTask).toBeVisible();
    await newTask.click();
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/modal-new-task.png',
      fullPage: true,
    });
  });

  test('Delete Task modal', async ({ page }) => {
    await openTasks(page, true);
    const [task] = mockTasks;
    await chooseCardAction(page, task.title, CardAction.remove);
    const dialog = page.getByRole('dialog', { name: `Delete "${task.title}"?` });
    await expect(dialog.getByRole('button', { name: 'Delete task' })).toBeVisible();
    await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeVisible();
    await capturePage(page, {
      path: 'screenshots/modal-delete-task.png',
      fullPage: true,
    });
  });

  test('Add Source modal', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/sources');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });

    // Click add source button
    const addSourceBtn = page
      .locator('button')
      .filter({ hasText: /Add Source/i });
    await expect(addSourceBtn).toBeVisible();
    await addSourceBtn.click();
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/modal-add-source.png',
      fullPage: true,
    });
  });

  test('Add Knowledge modal', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/wiki');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });

    // Click add knowledge button
    const addKnowledgeBtn = page
      .locator('button')
      .filter({ hasText: /Add Knowledge/i });
    await expect(addKnowledgeBtn).toBeVisible();
    await addKnowledgeBtn.click();
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/modal-add-knowledge.png',
      fullPage: true,
    });
  });

  test('New Project modal (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/projects');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });

    // Enable dark mode
    await page.evaluate(() => {
      document.documentElement.setAttribute('data-theme', 'dark');
    });
    await page.waitForTimeout(100);

    // Click new project button
    const newProjectBtn = page
      .locator('button')
      .filter({ hasText: /New Project/i });
    await expect(newProjectBtn).toBeVisible();
    await newProjectBtn.click();
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/dark-modal-new-project.png',
      fullPage: true,
    });
  });
});
