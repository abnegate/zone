import type { Page } from '@playwright/test';
import { expect, test } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

const workspace = '00000000-0000-0000-0000-000000000001';
const timestamp = '2026-09-05T00:00:00Z';
const theme = {
  workspace_id: workspace,
  primary_color_light: '#b03040',
  secondary_color_light: '#306090',
  primary_color_dark: '#e08040',
  secondary_color_dark: '#70b090',
  font_family: 'roboto',
  font_size_base: '18px',
  border_radius: 'none',
  created_at: timestamp,
  updated_at: timestamp,
};
const nullableTheme = {
  ...theme,
  primary_color_light: null,
  secondary_color_light: null,
  primary_color_dark: null,
  secondary_color_dark: null,
  font_family: null,
  font_size_base: null,
  border_radius: null,
};

async function prepare(
  page: Page,
  initial: typeof theme | typeof nullableTheme | null,
): Promise<string[]> {
  await blockServiceWorker(page.context());
  await mockCommonEndpoints(page);
  await setupAuth(page);
  await page.addInitScript(() =>
    localStorage.setItem('manager_theme', 'light'),
  );
  let saved = initial;
  const writes: string[] = [];
  await routeApi(
    page,
    /\/api\/workspaces\/[^/]+\/theme(?:\?|$)/,
    async (route) => {
      const method = route.request().method();
      if (method === 'PUT') {
        writes.push('theme:PUT');
        saved = { ...theme, ...route.request().postDataJSON() };
      }
      if (method === 'DELETE') {
        writes.push('theme:DELETE');
        const existed = saved !== null;
        saved = null;
        await route.fulfill({ status: existed ? 204 : 404 });
        return;
      }
      await route.fulfill({
        status: saved ? 200 : 404,
        contentType: 'application/json',
        body: JSON.stringify(
          saved ? { theme: saved } : { error: 'Theme not found' },
        ),
      });
    },
  );
  await routeApi(page, /\/settings\/ai/, async (route) => {
    if (route.request().method() !== 'GET')
      writes.push(`ai:${route.request().method()}`);
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        provider: 'openai',
        has_litellm_key: false,
        litellm_host: null,
        has_openai_api_key: true,
        openai_base_url: null,
        has_anthropic_api_key: false,
        anthropic_base_url: null,
        bedrock_region: null,
        bedrock_use_iam_role: false,
        has_bedrock_credentials: false,
        model_fast: 'gpt-4o-mini',
        model_reasoning: 'gpt-4o',
        model_embedding: 'text-embedding-3-small',
        model_image: 'flux1-schnell-fp8.safetensors',
      }),
    });
  });
  return writes;
}

async function openSettings(page: Page): Promise<void> {
  await page.goto('/settings');
  await expect(page.locator('#font-family')).toBeVisible();
}

async function verifyTheme(page: Page, mode: 'light' | 'dark'): Promise<void> {
  await expect(
    page.getByRole('button', { name: 'Primary Button', exact: true }),
  ).toHaveCSS(
    'background-color',
    mode === 'light' ? 'rgb(176, 48, 64)' : 'rgb(224, 128, 64)',
  );
  await expect(
    page.getByRole('button', { name: 'Secondary Button', exact: true }),
  ).toHaveCSS(
    'background-color',
    mode === 'light' ? 'rgb(48, 96, 144)' : 'rgb(112, 176, 144)',
  );
  for (const name of ['Primary Button', 'Secondary Button']) {
    await expect(page.getByRole('button', { name, exact: true })).toHaveCSS(
      'color',
      mode === 'light' ? 'rgb(255, 255, 255)' : 'rgb(0, 0, 0)',
    );
  }
  for (const name of ['Theme', 'AI Settings', 'Members']) {
    await expect(page.getByRole('tab', { name, exact: true })).toBeVisible();
  }
  await expect(page.locator('html')).toHaveCSS('font-size', '18px');
  await expect(page.locator('body')).toHaveCSS('font-family', /Roboto/i);
  await expect(page.locator('.page-title')).toHaveCSS('font-family', /Roboto/i);
  expect(
    await page.evaluate(
      async () => (await document.fonts.load('18px Roboto')).length,
    ),
  ).toBeGreaterThan(0);
  await expect(
    page.getByRole('button', { name: 'Primary Button', exact: true }),
  ).toHaveCSS('border-radius', '0px');
}

test('saved API theme controls actual light and dark component styles', async ({
  page,
}, testInfo) => {
  await prepare(page, theme);
  await openSettings(page);
  await verifyTheme(page, 'light');
  await page.screenshot({
    path: testInfo.outputPath('theme-light.png'),
    fullPage: true,
  });
  await page
    .locator('.preview-box')
    .screenshot({ path: testInfo.outputPath('theme-light-preview.png') });
  await page.locator('.page-title').scrollIntoViewIfNeeded();
  await page.getByRole('button', { name: 'Switch to dark mode' }).click();
  await verifyTheme(page, 'dark');
  await page.screenshot({
    path: testInfo.outputPath('theme-dark.png'),
    fullPage: true,
  });
  await page
    .locator('.preview-box')
    .screenshot({ path: testInfo.outputPath('theme-dark-preview.png') });
});

test('first save survives reload outside settings and unsaved edits are discarded', async ({
  page,
}) => {
  const writes = await prepare(page, null);
  await openSettings(page);
  await expect(page.locator('.alert-error')).not.toBeVisible();
  await page.locator('#primary-light').fill(theme.primary_color_light);
  await page.locator('#font-family').selectOption('roboto');
  await page.locator('#font-size').fill('18');
  await page.getByRole('button', { name: 'Save Changes', exact: true }).click();
  await expect(page.locator('.alert-success')).toBeVisible();
  expect(writes).toEqual(['theme:PUT']);
  await page.locator('#primary-light').fill('#00ff00');
  await page.locator('#font-family').selectOption('lato');
  await page.locator('.sidebar a[href="/chats"]').click();
  await expect(page.locator('body')).toHaveCSS('font-family', /Roboto/i);
  await page.reload();
  await expect(page.locator('body')).toHaveCSS('font-family', /Roboto/i);
  await expect(page.locator('html')).toHaveCSS('font-size', '18px');
  await page.locator('.sidebar a[href="/settings"]').click();
  await expect(page.locator('#primary-light')).toHaveValue(
    theme.primary_color_light,
  );
});

test('reset accepts 204 and absent 404 without changing AI settings', async ({
  page,
}) => {
  const writes = await prepare(page, theme);
  await openSettings(page);
  await expect(page.locator('html')).toHaveCSS('font-size', '18px');
  await page
    .getByRole('button', { name: 'Reset to Defaults', exact: true })
    .click();
  await expect(page.locator('.alert-success')).toBeVisible();
  await expect(page.locator('html')).toHaveCSS('font-size', '16px');
  await expect(page.locator('body')).not.toHaveCSS('font-family', /Roboto/i);
  await page
    .getByRole('button', { name: 'Reset to Defaults', exact: true })
    .click();
  await expect(page.locator('.alert-error')).not.toBeVisible();
  expect(writes).toEqual(['theme:DELETE', 'theme:DELETE']);
});

test('nullable persisted fields resolve to usable defaults', async ({
  page,
}) => {
  await prepare(page, nullableTheme);
  await openSettings(page);
  await expect(page.locator('.alert-error')).not.toBeVisible();
  await expect(page.locator('#font-family')).toHaveValue('');
  await expect(page.locator('html')).toHaveCSS('font-size', '16px');
  await page.locator('#primary-light').fill('#b03040');
  await expect(
    page.getByRole('button', { name: 'Primary Button', exact: true }),
  ).toHaveCSS('background-color', 'rgb(176, 48, 64)');
});

test('switching workspace clears saved theme outside settings', async ({
  page,
}) => {
  await prepare(page, theme);
  const second = '00000000-0000-0000-0000-000000000002';
  await routeApi(
    page,
    new RegExp(`/api/workspaces/${second}/theme`),
    async (route) => {
      await route.fulfill({
        status: 404,
        contentType: 'application/json',
        body: '{}',
      });
    },
  );
  await routeApi(
    page,
    /\/api\/organizations\/[^/]+\/workspaces(?:\?|$)/,
    async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          workspaces: [workspace, second].map((id, index) => ({
            id,
            organization_id: workspace,
            name: index === 0 ? 'Default Workspace' : 'Second Workspace',
            slug: index === 0 ? 'default' : 'second',
            description: null,
            is_active: true,
            created_at: timestamp,
            updated_at: timestamp,
          })),
        }),
      });
    },
  );
  await page.goto('/');
  await expect(page.locator('body')).toHaveCSS('font-family', /Roboto/i);
  await page.locator('.context-switcher-button').click();
  await page
    .getByRole('option', { name: 'Second Workspace', exact: true })
    .click();
  await expect(page.locator('html')).toHaveCSS('font-size', '16px');
  await expect(page.locator('body')).not.toHaveCSS('font-family', /Roboto/i);
  await page.locator('.sidebar a[href="/settings"]').click();
  await expect(page.locator('#primary-light')).not.toHaveValue(
    theme.primary_color_light,
  );
});
