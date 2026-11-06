import type { Page, TestInfo } from '@playwright/test';
import { expect, test } from './fixtures';
import { setupAdminAuth, setupCommonRoutes } from './layout-fixtures';
import { blockServiceWorker, routeApi } from './test-utils';

const timestamp = '2026-09-05T00:00:00Z';
const narrowLayoutWidth = 768;
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
        has_runpod_api_key: false,
        bedrock_region: null,
        bedrock_use_iam_role: null,
        has_bedrock_credentials: false,
        model_fast: 'llama3.1:8b',
        model_reasoning: 'deepseek-r1:7b',
        model_embedding: 'nomic-embed-text',
        model_image: 'flux1-schnell-fp8.safetensors',
        model_video: 'wan2.2_ti2v_5B_fp16.safetensors',
        model_audio: 'ace_step_v1_3.5b.safetensors',
        completions_routed: true,
        overrides: false,
        inherits_provider: true,
        organization_keys: { litellm: false, openai: false, anthropic: false },
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
              current_period_start: timestamp,
              current_period_end: '2026-10-05T00:00:00Z',
              usage: { members: 3, workspaces: 1, chat_messages: 1200 },
            }
          : path.endsWith('/limits')
            ? {
                max_members: 10,
                max_workspaces: 5,
                max_chats_per_month: 10000,
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

interface ShellLayout {
  columns: number;
  navigation: boolean;
  back: boolean;
}

function expectedShell(width: number): ShellLayout {
  const compact = width <= narrowLayoutWidth;
  return { columns: compact ? 0 : 2, navigation: !compact, back: compact };
}

async function shell(page: Page): Promise<ShellLayout> {
  return page.locator('.settings-shell').evaluate((element) => {
    const layout = element.querySelector('.settings-layout');
    const visible = (selector: string): boolean => {
      const target = element.querySelector(selector);
      return target !== null && target.getBoundingClientRect().height > 0;
    };
    return {
      columns: layout
        ? getComputedStyle(layout).gridTemplateColumns.split(' ').length
        : 0,
      navigation: visible('.settings-nav'),
      back: visible('.settings-back'),
    };
  });
}

async function openSection(
  page: Page,
  width: number,
  group: string,
  label: string,
): Promise<void> {
  const compact = width <= narrowLayoutWidth;
  if (compact) {
    await page.getByRole('link', { name: 'Back to settings', exact: true }).click();
    await expect(page).toHaveURL(/\/settings$/);
  }
  const link = page
    .getByRole('navigation', { name: 'Settings', exact: true })
    .getByRole('group', { name: group, exact: true })
    .getByRole('link', { name: label, exact: true });
  await link.click();
  await expect(page.getByRole('region', { name: label, exact: true })).toBeVisible();
  if (compact) {
    await expect(page.locator('.settings-page .page-bar-title')).toHaveText(label);
  } else {
    await expect(link).toHaveAttribute('aria-current', 'page');
  }
  await expect.poll(() => shell(page)).toEqual(expectedShell(width));
}

async function capture(
  page: Page,
  information: TestInfo,
  name: string,
): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
  await page.locator('.settings-page .page-body').evaluateAll((elements) => {
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

async function signIn(
  page: Page,
  scenario: (typeof scenarios)[number],
  path: string,
): Promise<void> {
  await page.setViewportSize({
    width: scenario.width,
    height: scenario.height,
  });
  await prepare(page, scenario.mode);
  await page.goto('/login', { waitUntil: 'domcontentloaded' });
  await setupAdminAuth(page);
  await page.goto(path, { waitUntil: 'domcontentloaded' });
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
    await expect(input).toHaveCSS('font-size', '13px');
    await expect(button).toHaveCSS('font-size', '14px');
  });

  test(`workspace settings layouts ${scenario.name}`, async ({
    page,
  }, information) => {
    await signIn(page, scenario, '/settings/workspace/general');
    await expect(page.locator('#font-family')).toBeVisible();
    await expect.poll(() => shell(page)).toEqual(expectedShell(scenario.width));
    const workspaceTitle = page.locator('.settings-page .page-bar-title');
    const rhythm = await page
      .locator('.settings-form')
      .first()
      .evaluate((form) => {
        const sections = Array.from(form.children).filter(
          (child) => !child.classList.contains('settings-actions'),
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
    expect(rhythm.every((gap) => Math.abs(gap - 16) < 1)).toBe(true);

    await expect(workspaceTitle).toHaveCSS('font-size', '18px');
    await capture(page, information, 'workspace-general');
    for (const section of ['AI', 'Folders', 'Members']) {
      await openSection(page, scenario.width, 'Workspace', section);
      await capture(page, information, `workspace-${section.toLowerCase()}`);
      if (section === 'Members') {
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
  });

  test(`organization settings layouts ${scenario.name}`, async ({
    page,
  }, information) => {
    await signIn(page, scenario, '/settings/organization/ai');
    await expect(
      page.getByRole('region', { name: 'AI defaults', exact: true }),
    ).toBeVisible();
    await expect.poll(() => shell(page)).toEqual(expectedShell(scenario.width));
    await capture(page, information, 'organization-ai-defaults');
    for (const section of ['Members', 'Devices', 'Billing', 'Audit log']) {
      await openSection(page, scenario.width, 'Organization', section);
      await capture(
        page,
        information,
        `organization-${section.toLowerCase().replaceAll(' ', '-')}`,
      );
      if (section === 'Members') {
        await page
          .getByRole('button', { name: 'Invite Member', exact: true })
          .click();
        await expect(page.getByRole('dialog')).toBeVisible();
        await expect(
          page.getByLabel('Email Address', { exact: true }),
        ).toHaveCSS('height', '32px');
        await capture(page, information, 'invitation-dialog');
        if (scenario.width === 390) {
          const bounds = await page.getByRole('dialog').boundingBox();
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
  });

  test(`sessions and navigation layouts ${scenario.name}`, async ({
    page,
  }, information) => {
    await signIn(page, scenario, '/settings/you/sessions');
    await expect(page.locator('.settings-page .page-bar-title')).toHaveCSS(
      'font-size',
      '18px',
    );
    await expect(
      page.getByRole('heading', { name: 'Active Sessions' }),
    ).toBeVisible();
    await expect.poll(() => shell(page)).toEqual(expectedShell(scenario.width));
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
    if (scenario.width <= narrowLayoutWidth) {
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
    await expect(page.locator('.sidebar-nav')).toBeVisible();
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
  await page.goto('/settings/workspace/general', { waitUntil: 'domcontentloaded' });
  await expect(page.getByRole('combobox', { name: 'Font Family' })).toHaveText('Roboto');
  await expect(page.locator('.settings-page .page-bar-title')).toHaveCSS(
    'font-size',
    '20.25px',
  );
  await expect(page.locator('.settings-page .page-bar-title')).toHaveCSS(
    'font-family',
    /Roboto/,
  );
  await expect(page.locator('#font-family')).toHaveCSS('height', '36px');
  await capture(page, information, 'workspace-custom-font');
});
