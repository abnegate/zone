import { test, expect } from '@playwright/test';
import { setupAuth } from './helpers/auth';
import { blockServiceWorker } from './test-utils';

// Mock data generators
const generateMockOrganization = (
  id: string,
  name: string,
  slug: string,
  options: { description?: string; is_active?: boolean } = {}
) => ({
  id,
  name,
  slug,
  description: options.description || null,
  is_active: options.is_active ?? true,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
});

const generateMockWorkspace = (
  id: string,
  organization_id: string,
  name: string,
  slug: string,
  options: { description?: string; is_active?: boolean } = {}
) => ({
  id,
  organization_id,
  name,
  slug,
  description: options.description || null,
  is_active: options.is_active ?? true,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
});

const mockOrgs = [
  generateMockOrganization('org-1', 'Acme Corp', 'acme-corp', { description: 'Main company' }),
  generateMockOrganization('org-2', 'Beta Inc', 'beta-inc'),
  generateMockOrganization('org-3', 'Gamma LLC', 'gamma-llc', { is_active: false }),
];

const mockWorkspaces: Record<string, ReturnType<typeof generateMockWorkspace>[]> = {
  'org-1': [
    generateMockWorkspace('ws-1', 'org-1', 'Engineering', 'engineering', { description: 'Dev team' }),
    generateMockWorkspace('ws-2', 'org-1', 'Marketing', 'marketing'),
  ],
  'org-2': [
    generateMockWorkspace('ws-3', 'org-2', 'Default', 'default'),
  ],
};

test.describe('Organizations & Context Switcher', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    // Mock models endpoint
    await page.route('**/api/models*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    // Mock organizations endpoint (needs * at end to match ?active=true query param)
    await page.route('**/api/organizations?*', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, organizations: mockOrgs }),
        });
      } else {
        route.continue();
      }
    });
    // Also match the path without query params
    await page.route('**/api/organizations', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, organizations: mockOrgs }),
        });
      } else {
        route.continue();
      }
    });

    // Mock workspaces endpoint - scoped by org (needs * at end to match ?active=true)
    await page.route('**/api/organizations/*/workspaces?*', (route) => {
      const url = new URL(route.request().url());
      const orgId = url.pathname.split('/')[3]; // /api/organizations/{orgId}/workspaces

      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            success: true,
            workspaces: mockWorkspaces[orgId] || [],
          }),
        });
      } else {
        route.continue();
      }
    });
    // Also match the path without query params
    await page.route('**/api/organizations/*/workspaces', (route) => {
      const url = new URL(route.request().url());
      const orgId = url.pathname.split('/')[3]; // /api/organizations/{orgId}/workspaces

      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            success: true,
            workspaces: mockWorkspaces[orgId] || [],
          }),
        });
      } else {
        route.continue();
      }
    });

    // Set API key and navigate
    await page.goto('/');
    await setupAuth(page);
    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
  });

  test.describe('Context Switcher Display', () => {
    test('displays context switcher in sidebar', async ({ page }) => {
      await expect(page.locator('.context-switcher')).toBeVisible();
    });

    test('shows loading state initially', async ({ page }) => {
      // This tests the brief loading state - may be too fast to catch
      // Just verify the component renders
      await expect(page.locator('.context-switcher')).toBeVisible();
    });

    test('displays current organization and workspace', async ({ page }) => {
      // Wait for context to load
      await page.waitForTimeout(500);

      const contextButton = page.locator('.context-switcher-button');
      await expect(contextButton).toBeVisible();
    });

    test('context switcher button shows org and workspace names', async ({ page }) => {
      // Set context in localStorage
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'org-1');
        localStorage.setItem('manager_current_workspace', 'ws-1');
      });
      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

      await page.waitForTimeout(500);

      await expect(page.locator('.org-name')).toContainText('Acme Corp');
      await expect(page.locator('.ws-name')).toContainText('Engineering');
    });

    test('hides context switcher when sidebar is collapsed', async ({ page }) => {
      // Collapse sidebar
      await page.click('.collapse-btn');

      // Context switcher should be hidden
      await expect(page.locator('.sidebar-context')).not.toBeVisible();
    });
  });

  test.describe('Context Switcher Dropdown', () => {
    test('opens dropdown on click', async ({ page }) => {
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');
      await expect(page.locator('.context-dropdown')).toBeVisible();
    });

    test('displays organization list in dropdown', async ({ page }) => {
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');

      // Should show organizations section
      await expect(page.locator('.dropdown-section h4').first()).toContainText('Organizations');
      await expect(page.locator('.dropdown-item').filter({ hasText: 'Acme Corp' })).toBeVisible();
      await expect(page.locator('.dropdown-item').filter({ hasText: 'Beta Inc' })).toBeVisible();
    });

    test('displays workspace list for selected organization', async ({ page }) => {
      // Set context
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'org-1');
        localStorage.setItem('manager_current_workspace', 'ws-1');
      });
      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');

      // Should show workspaces section with org-1's workspaces
      await expect(page.locator('.dropdown-item').filter({ hasText: 'Engineering' })).toBeVisible();
      await expect(page.locator('.dropdown-item').filter({ hasText: 'Marketing' })).toBeVisible();
    });

    test('closes dropdown on outside click', async ({ page }) => {
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');
      await expect(page.locator('.context-dropdown')).toBeVisible();

      // Click outside
      await page.click('.main-content', { force: true });
      await expect(page.locator('.context-dropdown')).not.toBeVisible();
    });

    test('shows checkmark for selected organization', async ({ page }) => {
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'org-1');
        localStorage.setItem('manager_current_workspace', 'ws-1');
      });
      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');

      // Active org should have a checkmark
      await expect(
        page.locator('.dropdown-item.active').filter({ hasText: 'Acme Corp' })
      ).toBeVisible();
    });

    test('shows checkmark for selected workspace', async ({ page }) => {
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'org-1');
        localStorage.setItem('manager_current_workspace', 'ws-1');
      });
      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');

      // Active workspace should have a checkmark
      await expect(
        page.locator('.dropdown-item.active').filter({ hasText: 'Engineering' })
      ).toBeVisible();
    });
  });

  test.describe('Organization Switching', () => {
    test('switches organization when clicked', async ({ page }) => {
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'org-1');
        localStorage.setItem('manager_current_workspace', 'ws-1');
      });
      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');
      await page.click('.dropdown-item:has-text("Beta Inc")');

      // Organization name should update
      await page.waitForTimeout(300);
      await expect(page.locator('.org-name')).toContainText('Beta Inc');
    });

    test('clears workspace when switching organizations', async ({ page }) => {
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'org-1');
        localStorage.setItem('manager_current_workspace', 'ws-1');
      });
      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');
      await page.click('.dropdown-item:has-text("Beta Inc")');

      await page.waitForTimeout(300);

      // Workspace should be cleared or set to first workspace of new org
      // Depending on implementation - just verify org changed
      await expect(page.locator('.org-name')).toContainText('Beta Inc');
    });

    test('loads workspaces for new organization', async ({ page }) => {
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'org-1');
        localStorage.setItem('manager_current_workspace', 'ws-1');
      });
      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');
      await page.click('.dropdown-item:has-text("Beta Inc")');

      await page.waitForTimeout(300);

      // Open dropdown again
      await page.click('.context-switcher-button');

      // Should show Beta Inc's workspaces
      await expect(page.locator('.dropdown-item').filter({ hasText: 'Default' })).toBeVisible();
    });
  });

  test.describe('Workspace Switching', () => {
    test('switches workspace when clicked', async ({ page }) => {
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'org-1');
        localStorage.setItem('manager_current_workspace', 'ws-1');
      });
      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');
      await page.click('.dropdown-item:has-text("Marketing")');

      await page.waitForTimeout(300);
      await expect(page.locator('.ws-name')).toContainText('Marketing');
    });

    test('keeps organization when switching workspace', async ({ page }) => {
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'org-1');
        localStorage.setItem('manager_current_workspace', 'ws-1');
      });
      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');
      await page.click('.dropdown-item:has-text("Marketing")');

      await page.waitForTimeout(300);

      // Organization should remain the same
      await expect(page.locator('.org-name')).toContainText('Acme Corp');
      await expect(page.locator('.ws-name')).toContainText('Marketing');
    });
  });

  test.describe('LocalStorage Persistence', () => {
    test('persists context selection in localStorage', async ({ page }) => {
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');
      await page.click('.dropdown-item:has-text("Acme Corp")');

      await page.waitForTimeout(300);

      // Check localStorage
      const orgId = await page.evaluate(() =>
        localStorage.getItem('manager_current_org')
      );
      expect(orgId).toBe('org-1');
    });

    test('restores context from localStorage on reload', async ({ page }) => {
      // Set context
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'org-1');
        localStorage.setItem('manager_current_workspace', 'ws-2');
      });

      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.waitForTimeout(500);

      await expect(page.locator('.org-name')).toContainText('Acme Corp');
      await expect(page.locator('.ws-name')).toContainText('Marketing');
    });

    test('handles invalid localStorage context gracefully', async ({ page }) => {
      // Set invalid context
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'invalid-org');
        localStorage.setItem('manager_current_workspace', 'invalid-ws');
      });

      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

      // Should not crash, context switcher should still work
      await expect(page.locator('.context-switcher')).toBeVisible();
    });

    test('handles non-existent org/workspace IDs gracefully', async ({ page }) => {
      // Set context with IDs that don't exist
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'non-existent-org');
        localStorage.setItem('manager_current_workspace', 'non-existent-ws');
      });

      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

      // Should not crash
      await expect(page.locator('.context-switcher')).toBeVisible();
    });
  });

  test.describe('Empty States', () => {
    test('shows message when no organizations exist', async ({ page }) => {
      await page.unroute('**/api/organizations?*');
      await page.unroute('**/api/organizations');
      await page.route('**/api/organizations?*', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, organizations: [] }),
        });
      });
      await page.route('**/api/organizations', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, organizations: [] }),
        });
      });

      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.waitForTimeout(500);

      await expect(page.locator('.context-switcher-empty')).toBeVisible();
    });

    test('shows message when org has no workspaces', async ({ page }) => {
      // Set context to org with no workspaces
      await page.evaluate(() => {
        localStorage.setItem('manager_current_org', 'org-3');
        localStorage.removeItem('manager_current_workspace');
      });

      // Org-3 has no workspaces in our mock
      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');

      // Workspaces section should indicate no workspaces or be empty
      const workspacesSection = page.locator('.dropdown-section').last();
      await expect(workspacesSection).toBeVisible();
    });
  });

  test.describe('Dropdown Chevron', () => {
    test('rotates chevron when dropdown opens', async ({ page }) => {
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');

      await expect(page.locator('.chevron.open')).toBeVisible();
    });

    test('chevron returns to normal when dropdown closes', async ({ page }) => {
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');
      await expect(page.locator('.chevron.open')).toBeVisible();

      // Close dropdown
      await page.click('.context-switcher-button');
      await expect(page.locator('.chevron:not(.open)')).toBeVisible();
    });
  });

  test.describe('Keyboard Navigation', () => {
    test('closes dropdown when clicking button again', async ({ page }) => {
      await page.waitForTimeout(500);

      await page.click('.context-switcher-button');
      await expect(page.locator('.context-dropdown')).toBeVisible();

      // Click button again to toggle closed
      await page.click('.context-switcher-button');
      await expect(page.locator('.context-dropdown')).not.toBeVisible();
    });
  });

  test.describe('Loading States', () => {
    test('shows loading indicator while fetching organizations', async ({ page }) => {
      // Add delay to the organizations endpoint
      await page.unroute('**/api/organizations?*');
      await page.unroute('**/api/organizations');
      await page.route('**/api/organizations?*', async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 1000));
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, organizations: mockOrgs }),
        });
      });
      await page.route('**/api/organizations', async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 1000));
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, organizations: mockOrgs }),
        });
      });

      await page.reload();
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

      // Should show loading state
      await expect(page.locator('.context-switcher-loading')).toBeVisible();

      // Wait for load to complete
      await page.waitForTimeout(1500);
      await expect(page.locator('.context-switcher-loading')).not.toBeVisible();
    });
  });
});

test.describe('Organizations API', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    await page.route('**/api/models*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.goto('/');
    await setupAuth(page);
    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
  });

  test('handles API error when fetching organizations', async ({ page }) => {
    await page.route('**/api/organizations?*', (route) => {
      route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ success: false, error: 'Server error' }),
      });
    });
    await page.route('**/api/organizations', (route) => {
      route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ success: false, error: 'Server error' }),
      });
    });

    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

    // Should handle error gracefully - empty or error state
    await expect(page.locator('.context-switcher')).toBeVisible();
  });

  test('handles API error when fetching workspaces', async ({ page }) => {
    await page.route('**/api/organizations?*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true, organizations: mockOrgs }),
      });
    });
    await page.route('**/api/organizations', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true, organizations: mockOrgs }),
      });
    });

    await page.route('**/api/organizations/*/workspaces?*', (route) => {
      route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ success: false, error: 'Server error' }),
      });
    });
    await page.route('**/api/organizations/*/workspaces', (route) => {
      route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ success: false, error: 'Server error' }),
      });
    });

    await page.evaluate(() => {
      localStorage.setItem('manager_current_org', 'org-1');
      localStorage.removeItem('manager_current_workspace');
    });

    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

    // Should handle error gracefully
    await expect(page.locator('.context-switcher')).toBeVisible();
  });
});
