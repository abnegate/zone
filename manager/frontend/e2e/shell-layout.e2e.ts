import type { Page, TestInfo } from '@playwright/test';
import { expect, test } from './fixtures';
import { setupAdminAuth, setupCommonRoutes } from './layout-fixtures';
import { blockServiceWorker, routeApi } from './test-utils';

const timestamp = '2026-09-05T00:00:00Z';
const scenarios = [
  { name: 'desktop-light', width: 1440, height: 1000, mode: 'light' },
  { name: 'mobile-light', width: 390, height: 844, mode: 'light' },
  { name: 'tablet-light', width: 768, height: 1024, mode: 'light' },
  { name: 'desktop-dark', width: 1440, height: 1000, mode: 'dark' },
  { name: 'mobile-dark', width: 390, height: 844, mode: 'dark' },
] as const;

async function prepare(page: Page, mode: 'light' | 'dark'): Promise<void> {
  await blockServiceWorker(page.context());
  await page.addInitScript(
    (value) => localStorage.setItem('manager_theme', value),
    mode,
  );
  await setupCommonRoutes(page, true);
  await routeApi(
    page,
    /\/api\/auth\/(verify-email|forgot-password)/,
    async (route) => {
      await route.fulfill({
        json: { success: true, message: 'Request completed successfully' },
      });
    },
  );
  await routeApi(
    page,
    /\/api\/invitations\/layout-invitation-token/,
    async (route) => {
      await route.fulfill({
        json: {
          organization_name: 'Acme Corp',
          org_role: 'member',
          workspace_name: 'Engineering',
          workspace_role: 'member',
          invited_by_email: 'admin@example.com',
          expires_at: '2099-01-01T00:00:00Z',
        },
      });
    },
  );
  await routeApi(page, /\/settings\/ai/, async (route) => {
    await route.fulfill({
      json: {
        provider: 'self_hosted',
        has_litellm_key: false,
        litellm_host: null,
        has_openai_api_key: false,
        openai_base_url: null,
        has_anthropic_api_key: false,
        anthropic_base_url: null,
        bedrock_region: null,
        bedrock_use_iam_role: false,
        has_bedrock_credentials: false,
        model_fast: 'llama3.1:8b',
        model_reasoning: 'deepseek-r1:7b',
        model_embedding: 'nomic-embed-text',
        model_image: 'flux1-schnell-fp8.safetensors',
        override_ai_settings: false,
      },
    });
  });
  await routeApi(
    page,
    /\/organizations\/[^/]+\/(subscription|usage|limits|audit-logs|invitations)(?:\?|$)/,
    async (route) => {
      const path = new URL(route.request().url()).pathname;
      const body = path.endsWith('/subscription')
        ? {
            subscription: {
              id: 'subscription-1',
              organization_id: 'org-1',
              plan_id: 'pro',
              plan_name: 'Pro',
              status: 'active',
              current_period_start: timestamp,
              current_period_end: '2026-10-05T00:00:00Z',
              cancel_at_period_end: false,
            },
          }
        : path.endsWith('/usage')
          ? {
              users: 3,
              workspaces: 1,
              projects: 4,
              storage_gb: 2.4,
              api_calls: 1200,
              period_start: timestamp,
              period_end: '2026-10-05T00:00:00Z',
            }
          : path.endsWith('/limits')
            ? {
                max_users: 10,
                max_workspaces: 5,
                max_projects: 20,
                max_storage_gb: 50,
                max_api_calls_monthly: 10000,
              }
            : path.endsWith('/audit-logs')
              ? {
                  logs: [
                    {
                      id: 'log-1',
                      organization_id: 'org-1',
                      actor_id: 'user-1',
                      actor_email: 'admin@example.com',
                      action: 'create',
                      resource_type: 'project',
                      resource_id: 'project-1',
                      metadata: { name: 'Zone Platform' },
                      created_at: timestamp,
                    },
                  ],
                  total: 1,
                }
              : { invitations: [] };
      await route.fulfill({ json: body });
    },
  );
}

async function tabs(page: Page): Promise<{ height: number; gap: number }> {
  return page.locator('.settings-page-body').evaluate((element) => {
    const list = element
      .querySelector('[role="tablist"]')
      ?.getBoundingClientRect();
    const panel = element
      .querySelector('[role="tabpanel"]:not([hidden])')
      ?.getBoundingClientRect();
    if (!list || !panel) throw new Error('Settings tabs must be visible');
    return {
      height: Math.round(list.height),
      gap: Math.round(panel.y - list.y - list.height),
    };
  });
}

async function capture(
  page: Page,
  information: TestInfo,
  name: string,
): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
  await page.locator('.settings-page-body').evaluateAll((elements) => {
    for (const element of elements) element.scrollTop = 0;
  });
  await expect
    .poll(() =>
      page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    )
    .toBe(true);
  await expect(
    page.getByText(/Validation failed|Unexpected token/),
  ).toHaveCount(0);
  await page.screenshot({
    path: information.outputPath(`${name}.png`),
    fullPage: true,
    animations: 'disabled',
  });
}

for (const scenario of scenarios) {
  test(`authentication layouts ${scenario.name}`, async ({
    page,
  }, information) => {
    await page.setViewportSize({
      width: scenario.width,
      height: scenario.height,
    });
    await prepare(page, scenario.mode);
    for (const route of [
      '/login',
      '/register',
      '/forgot-password',
      '/reset-password',
      '/verify-email',
      '/invitations',
      '/unauthorized',
      '/reset-password?token=layout-password-reset-token',
      '/verify-email?token=layout-email-verification-token',
      '/invitations?token=layout-invitation-token',
    ]) {
      await page.goto(route, { waitUntil: 'domcontentloaded' });
      await expect(
        page.locator('.auth-container, .auth-card, .invitation-card'),
      ).toBeVisible();
      await capture(
        page,
        information,
        route.slice(1).replaceAll(/[^a-z-]+/g, '-'),
      );
    }
    await page.goto('/login', { waitUntil: 'domcontentloaded' });
    const input = page.getByLabel('Email', { exact: true });
    const button = page.getByRole('button', { name: 'Sign In', exact: true });
    await expect(input).toHaveCSS('height', '36px');
    await expect(button).toHaveCSS('height', '36px');
    await expect(input).toHaveCSS('font-size', '14px');
    await expect(button).toHaveCSS('font-size', '14px');
  });

  test(`settings and sessions layouts ${scenario.name}`, async ({
    page,
  }, information) => {
    await page.setViewportSize({
      width: scenario.width,
      height: scenario.height,
    });
    await prepare(page, scenario.mode);
    await page.goto('/login', { waitUntil: 'domcontentloaded' });
    await setupAdminAuth(page);
    await page.goto('/settings', { waitUntil: 'domcontentloaded' });
    await expect(page.locator('#font-family')).toBeVisible();
    const workspaceTabs = await tabs(page);
    expect(workspaceTabs).toEqual({ height: 36, gap: 24 });
    const workspaceTitle = page.locator('.settings-page-header h1');
    const rhythm = await page
      .locator('.settings-form')
      .first()
      .evaluate((form) => {
        const sections = Array.from(
          form.querySelectorAll(
            ':scope > .settings-section > .settings-grid, :scope > .settings-section > .settings-card',
          ),
        );
        return sections
          .slice(1)
          .map(
            (section, index) =>
              section.getBoundingClientRect().top -
              sections[index].getBoundingClientRect().bottom,
          );
      });
    expect(rhythm.length).toBeGreaterThan(1);
    expect(rhythm.every((gap) => Math.abs(gap - 24) < 1)).toBe(true);

    await expect(workspaceTitle).toHaveCSS('font-size', '20px');
    for (const tab of ['Theme', 'AI Settings', 'Members']) {
      await page.getByRole('tab', { name: tab, exact: true }).click();
      await expect(page.getByRole('tabpanel')).toBeVisible();
      await capture(
        page,
        information,
        `workspace-${tab.toLowerCase().replaceAll(' ', '-')}`,
      );
      if (tab === 'Members') {
        await page
          .getByRole('button', { name: 'Add Member', exact: true })
          .click();
        await expect(page.getByRole('dialog')).toBeVisible();
        await page.getByRole('combobox', { name: 'Role', exact: true }).click();
        await expect(page.getByRole('listbox')).toBeVisible();
        const stacking = await page.evaluate(() => ({
          popover: Number(
            getComputedStyle(
              document.querySelector('.ui-select-content') as Element,
            ).zIndex,
          ),
          modal: Number(
            getComputedStyle(
              document.querySelector('.ui-dialog-content') as Element,
            ).zIndex,
          ),
        }));
        expect(stacking.popover).toBeGreaterThan(stacking.modal);
        await capture(page, information, 'workspace-member-role-menu');
        await page.keyboard.press('Escape');
        await page.getByRole('button', { name: 'Cancel', exact: true }).click();
      }
    }
    await page.goto('/org-settings', { waitUntil: 'domcontentloaded' });
    for (const tab of [
      'AI Settings',
      'Members',
      'Invitations',
      'Billing',
      'Audit Logs',
    ]) {
      await page.getByRole('tab', { name: tab, exact: true }).click();
      await expect(page.getByRole('tabpanel')).toBeVisible();
      await expect(
        page.getByRole('tab', { name: tab, exact: true }),
      ).toHaveAttribute('aria-selected', 'true');
      expect.soft(await tabs(page)).toEqual(workspaceTabs);
      await capture(
        page,
        information,
        `organization-${tab.toLowerCase().replaceAll(' ', '-')}`,
      );
      if (tab === 'Invitations') {
        await page
          .getByRole('button', { name: 'Invite Member', exact: true })
          .click();
        await expect(page.getByRole('dialog')).toBeVisible();
        await expect(
          page.getByLabel('Email Address', { exact: true }),
        ).toHaveCSS('height', '36px');
        await capture(page, information, 'invitation-dialog');
        if (scenario.width === 390) {
          const bounds = await page
            .locator('.invitation-dialog .modal-content')
            .boundingBox();
          expect.soft(bounds ? Math.round(bounds.x) : null).toBe(16);
          expect
            .soft(
              bounds
                ? Math.round(scenario.width - bounds.x - bounds.width)
                : null,
            )
            .toBe(16);
        }
        await page.getByRole('button', { name: 'Cancel', exact: true }).click();
      }
    }
    await page.goto('/sessions', { waitUntil: 'domcontentloaded' });
    await expect(
      page.getByRole('heading', { name: 'Active Sessions' }),
    ).toHaveCSS('font-size', '20px');
    await expect(
      page.getByRole('table', { name: 'Active sessions' }),
    ).toBeVisible();
    await capture(page, information, 'sessions');
    await page
      .getByRole('button', { name: 'Revoke All Other Sessions' })
      .click();
    await expect(page.getByRole('dialog')).toBeVisible();
    const layers = await page.evaluate(() => {
      const level = (selector: string): number =>
        Number(
          getComputedStyle(document.querySelector(selector) as Element).zIndex,
        );
      return {
        overlay: level('.ui-dialog-overlay'),
        dialog: level('.ui-dialog-content'),
        sidebar: level('.sidebar'),
      };
    });
    expect(layers.overlay).toBeGreaterThan(layers.sidebar);
    expect(layers.dialog).toBeGreaterThanOrEqual(layers.overlay);

    await expect(
      page.getByRole('button', { name: 'Cancel', exact: true }),
    ).toBeVisible();
    await capture(page, information, 'session-confirmation');
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();
    if (scenario.width <= 768) {
      await page.getByRole('button', { name: /Toggle menu/i }).click();
      await expect
        .poll(async () => (await page.locator('.sidebar').boundingBox())?.x)
        .toBe(0);
      const header = await page.evaluate(() => {
        const bounds = (selector: string): DOMRect =>
          document.querySelector(selector)!.getBoundingClientRect();
        return {
          menuRight: bounds('.mobile-menu-btn').right,
          logoLeft: bounds('.sidebar-header .zone-logo').left,
          logoRight: bounds('.sidebar-header .zone-logo').right,
          themeLeft: bounds('.sidebar-header .theme-toggle').left,
        };
      });
      expect(header.logoLeft - header.menuRight).toBeGreaterThanOrEqual(8);
      expect(header.themeLeft - header.logoRight).toBeGreaterThanOrEqual(8);
    }
    await expect(page.getByRole('navigation')).toBeVisible();
    await expect(page.getByRole('button', { name: /Logout/i })).toBeVisible();
    await capture(page, information, 'navigation');
  });
}

test('workspace custom typography preserves shared proportions', async ({
  page,
}, information) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await prepare(page, 'dark');
  await routeApi(page, /\/workspaces\/[^/]+\/theme(?:\?|$)/, async (route) => {
    await route.fulfill({
      json: {
        theme: {
          workspace_id: 'ws-1',
          primary_color_light: null,
          primary_color_dark: null,
          secondary_color_light: null,
          secondary_color_dark: null,
          font_family: 'roboto',
          font_size_base: '18px',
          border_radius: 'medium',
          created_at: timestamp,
          updated_at: timestamp,
        },
      },
    });
  });
  await page.goto('/login', { waitUntil: 'domcontentloaded' });
  await setupAdminAuth(page);
  await page.goto('/settings', { waitUntil: 'domcontentloaded' });
  await expect(page.locator('#font-family')).toHaveValue('roboto');
  await expect(page.locator('.settings-page-header h1')).toHaveCSS(
    'font-size',
    '22.5px',
  );
  await expect(page.locator('.settings-page-header h1')).toHaveCSS(
    'font-family',
    /Roboto/,
  );
  await expect(page.locator('#font-family')).toHaveCSS('height', '40.5px');
  await capture(page, information, 'workspace-custom-font');
});
