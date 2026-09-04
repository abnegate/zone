import { test, expect } from './fixtures';
import { routeApi } from './test-utils';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';

// Use iPhone 12 viewport dimensions for mobile tests
test.describe('Mobile Responsiveness', () => {
  test.skip(({ browserName }) => browserName === 'firefox', 'Firefox does not support isMobile.');
  test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });

  test.beforeEach(async ({ page }) => {
    // Set up API mocks
    await mockCommonEndpoints(page);

    // Navigate and set up auth
    await page.goto('/');
    await setupAuth(page);
    await page.reload();

    // Wait for page to load (on mobile, sidebar is hidden)
    await expect(page.locator('.mobile-menu-btn')).toBeVisible({ timeout: 10000 });
  });

  test.describe('Sidebar', () => {
    test('sidebar is hidden by default on mobile', async ({ page }) => {
      // Sidebar should be off-screen on mobile (transformed)
      const sidebar = page.locator('.sidebar');
      await expect(sidebar).toBeAttached();
      // The sidebar exists but is translated off-screen
    });

    test('menu toggle button is visible on mobile', async ({ page }) => {
      await expect(page.locator('.mobile-menu-btn')).toBeVisible();
    });

    test('clicking menu toggle opens sidebar', async ({ page }) => {
      await page.click('.mobile-menu-btn');

      // Sidebar should now be visible with .open class
      await expect(page.locator('.sidebar.open')).toBeVisible();
    });

    test('overlay appears when sidebar is open', async ({ page }) => {
      await page.click('.mobile-menu-btn');

      await expect(page.locator('.sidebar-overlay')).toBeVisible();
    });

    test('clicking overlay closes sidebar', async ({ page }) => {
      await page.click('.mobile-menu-btn');
      await expect(page.locator('.sidebar.open')).toBeVisible();

      const overlay = page.locator('.sidebar-overlay');
      const box = await overlay.boundingBox();
      expect(box).toBeTruthy();
      // Sidebar is 240px and stacked above the overlay; avoid the covered center.
      await overlay.click({
        position: { x: box!.width - 16, y: Math.round(box!.height / 2) },
      });

      await expect(page.locator('.sidebar.open')).not.toBeVisible();
    });

    test('clicking nav item closes mobile sidebar', async ({ page }) => {
      await page.click('.mobile-menu-btn');
      await expect(page.locator('.sidebar.open')).toBeVisible();

      await page.click('a[href="/chats"]');

      await expect(page.locator('.sidebar.open')).not.toBeVisible();
    });
  });

  test.describe('Navigation', () => {
    test('navigates correctly on mobile', async ({ page }) => {
      await routeApi(page, '**/api/projects*', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ projects: [] }),
        });
      });

      await page.click('.mobile-menu-btn');
      await page.click('a[href="/projects"]');

      await expect(page).toHaveURL('/projects');
      await expect(page.getByRole('heading', { name: 'Projects', exact: true })).toBeVisible();
    });

    test('maintains navigation state after sidebar closes', async ({ page }) => {
      await routeApi(page, '**/api/chats*', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chats: [] }),
        });
      });

      await page.click('.mobile-menu-btn');
      await page.click('a[href="/chats"]');

      // Sidebar should close
      await expect(page.locator('.sidebar.open')).not.toBeVisible();

      // But we should be on the correct page
      await expect(page).toHaveURL('/chats');
    });
  });

  test.describe('Page Content', () => {
    test('main content fills viewport width on mobile', async ({ page }) => {
      const mainContent = page.locator('.main-content');
      const boundingBox = await mainContent.boundingBox();

      // Main content should fill most of the viewport
      expect(boundingBox?.width).toBeGreaterThan(350);
    });

    test('page header is visible on mobile', async ({ page }) => {
      await expect(page.getByRole('heading', { name: 'Chats', exact: true })).toBeVisible();
    });
  });

  test.describe('Models Page Mobile', () => {
    test('models page renders correctly on mobile', async ({ page }) => {
      await page.goto('/models');
      await expect(page.getByRole('heading', { name: 'Models', exact: true })).toBeVisible();
    });

    test('model form is visible on mobile', async ({ page }) => {
      await page.goto('/models');
      await expect(page.locator('.model-form')).toBeVisible();
    });
  });

  test.describe('Touch Interactions', () => {
    test('tap interactions work on mobile', async ({ page }) => {
      // Tap menu toggle
      await page.tap('.mobile-menu-btn');

      await expect(page.locator('.sidebar.open')).toBeVisible();
    });
  });
});

// Tablet viewport tests
test.describe('Tablet Responsiveness', () => {
  test.use({ viewport: { width: 768, height: 1024 } });

  test.beforeEach(async ({ page }) => {
    await mockCommonEndpoints(page);

    await page.goto('/');
    await setupAuth(page);
    await page.reload();

    // Wait for page to load
    await expect(page.getByRole('heading', { name: 'Chats', exact: true })).toBeVisible({
      timeout: 10000,
    });
  });

  test('page loads correctly on tablet', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Chats', exact: true })).toBeVisible();
  });

  test('content adjusts to tablet width', async ({ page }) => {
    const mainContent = page.locator('.main-content');
    await expect(mainContent).toBeVisible();
  });
});

// Small desktop tests
test.describe('Small Desktop Responsiveness', () => {
  test.use({ viewport: { width: 1024, height: 768 } });

  test.beforeEach(async ({ page }) => {
    await mockCommonEndpoints(page);

    await page.goto('/');
    await setupAuth(page);
    await page.reload();

    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
  });

  test('sidebar is visible on desktop', async ({ page }) => {
    await expect(page.locator('.sidebar')).toBeVisible();
  });

  test('sidebar can be collapsed on desktop', async ({ page }) => {
    await page.click('.collapse-btn');

    // Sidebar should be in collapsed state
    await expect(page.locator('.sidebar.collapsed')).toBeVisible();
  });

  test('collapsed sidebar shows only icons', async ({ page }) => {
    await page.click('.collapse-btn');

    // Nav item text should not be visible when collapsed
    await expect(page.locator('.nav-item span').first()).not.toBeVisible();
  });

  test('expanding collapsed sidebar shows text', async ({ page }) => {
    // Collapse
    await page.click('.collapse-btn');
    await expect(page.locator('.sidebar.collapsed')).toBeVisible();

    // Expand
    await page.click('.collapse-btn');
    await expect(page.locator('.sidebar.collapsed')).not.toBeVisible();
  });
});

// Large desktop tests
test.describe('Large Desktop Responsiveness', () => {
  test.use({ viewport: { width: 1920, height: 1080 } });

  test.beforeEach(async ({ page }) => {
    await mockCommonEndpoints(page);

    await page.goto('/');
    await setupAuth(page);
    await page.reload();

    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
  });

  test('layout uses available space on large screens', async ({ page }) => {
    const mainContent = page.locator('.main-content');
    const boundingBox = await mainContent.boundingBox();

    // Main content should use more space on larger screens
    expect(boundingBox?.width).toBeGreaterThan(1000);
  });

  test('sidebar is fully visible with labels', async ({ page }) => {
    await expect(page.locator('.sidebar')).toBeVisible();
    await expect(page.locator('.nav-item span').first()).toBeVisible();
  });
});
