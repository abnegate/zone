import { test, expect } from './fixtures';
import type { Page } from '@playwright/test';
import { blockServiceWorker, routeApi } from './test-utils';
import { setupAdminAuth, setupCommonRoutes } from './layout-fixtures';

const runScreenshots = process.env.RUN_SCREENSHOTS === 'true';
const describeScreenshots = runScreenshots ? test.describe : test.describe.skip;

async function capturePage(
  page: Page,
  options: Parameters<Page['screenshot']>[0]
): Promise<void> {
  const populated: Record<string, string> = {
    'screenshots/models-populated.png': '.model-item',
    'screenshots/chats-populated.png': '.chat-item',
    'screenshots/projects-populated.png': '.project-card',
    'screenshots/tasks-populated.png': '.task-card:not(.skeleton-card)',
    'screenshots/sources-populated.png': '.source-card',
    'screenshots/wiki-populated.png': '.knowledge-card',
  };
  const content = populated[options?.path ?? ''];
  if (content) {
    await expect(page.locator(content).first()).toBeVisible();
    await expect(page.locator('.skeleton-card')).toHaveCount(0);
  }
  if (options?.path?.includes('modal-'))
    await expect(page.getByRole('dialog')).toBeVisible();
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
    await page.waitForTimeout(500);
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
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/tasks');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
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

  test('Sessions page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/sessions');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/sessions-empty.png',
      fullPage: true,
    });
  });

  test('Organization settings (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/org-settings');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/org-settings-empty.png',
      fullPage: true,
    });
  });

  test('Workspace settings (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/settings');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
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
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/tasks');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/tasks-populated.png',
      fullPage: true,
    });
  });

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

  test('Sessions page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/sessions');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/sessions-populated.png',
      fullPage: true,
    });
  });

  test('Organization settings (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/org-settings');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/org-settings-populated.png',
      fullPage: true,
    });
  });

  test('Workspace settings (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/settings');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/workspace-settings-populated.png',
      fullPage: true,
    });
  });

  test('Search page', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/search');
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
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/tasks');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
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
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/tasks');
    await page.waitForLoadState('domcontentloaded');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });

    // Click new task button
    const newTaskBtn = page.locator('button').filter({ hasText: /New Task/i });
    await expect(newTaskBtn).toBeVisible();
    await newTaskBtn.click();
    await page.waitForTimeout(300);
    await capturePage(page, {
      path: 'screenshots/modal-new-task.png',
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
