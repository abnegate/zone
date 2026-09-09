import { expect, signIn, state, test } from './harness';

test.describe('the console against a real server', () => {
  test('signs in with a real account and renders the shell', async ({ page, consoleErrors }) => {
    await signIn(page);

    await expect(page.locator('nav, .sidebar, [role="navigation"]').first()).toBeVisible();
    await expect(page.getByRole('link', { name: /chats/i }).first()).toBeVisible();
    await expect(page.getByRole('link', { name: /models/i }).first()).toBeVisible();
    expect(consoleErrors).toEqual([]);
  });

  test('serves the seeded organization and workspace, not a fixture', async ({ page }) => {
    await signIn(page);

    const organizations = await page.evaluate(async () => {
      const token = localStorage.getItem('manager_access_token');
      const response = await fetch('/api/organizations', {
        headers: { authorization: `Bearer ${token}` },
      });
      return response.json();
    });

    const names = (organizations.organizations as { id: string; name: string }[]).map(
      (organization) => organization.id
    );
    expect(names).toContain(state.owner.organization.id);
  });

  test('reaches every workspace page without a console error', async ({ page, consoleErrors }) => {
    await signIn(page);

    for (const path of ['/chats', '/models', '/tasks', '/projects', '/sources', '/settings']) {
      await page.goto(path);
      await expect(page.locator('main, .app-content, .workspace-content').first()).toBeVisible();
    }

    expect(consoleErrors).toEqual([]);
  });
});
