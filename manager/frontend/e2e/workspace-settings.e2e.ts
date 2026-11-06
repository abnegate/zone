import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { chooseSelect, expectSelectOptions } from './helpers/select';
import { blockServiceWorker, routeApiContext } from './test-utils';

const mockTheme = {
  workspace_id: '00000000-0000-0000-0000-000000000001',
  primary_color_light: '#3b82f6',
  secondary_color_light: '#6366f1',
  primary_color_dark: '#3b82f6',
  secondary_color_dark: '#6366f1',
  font_family: 'system',
  font_size_base: '16px',
  border_radius: 'medium',
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
};

const themeRoutePattern = /\/api\/workspaces\/[^/]+\/theme/;

const GENERAL_PATH = '/settings/workspace/general';
const AI_PATH = '/settings/workspace/ai';

const IMAGE_MODEL = 'flux1-schnell-fp8.safetensors';
const VIDEO_MODEL = 'wan2.2_ti2v_5B_fp16.safetensors';
const AUDIO_MODEL = 'ace_step_v1_3.5b.safetensors';

const INHERITED = 'Inherited from organization';
const OVERRIDDEN = 'Set for this workspace';

const MODEL_KEYS = [
  'model_fast',
  'model_reasoning',
  'model_embedding',
  'model_image',
  'model_video',
  'model_audio',
] as const;

type ModelKey = (typeof MODEL_KEYS)[number];

type Models = Record<ModelKey, string | null>;

const CHAT_MODEL_KEYS: readonly ModelKey[] = ['model_fast', 'model_reasoning', 'model_embedding'];

const ENDPOINTS = [
  { url: 'litellm_host', key: 'litellm_key', saved: 'has_litellm_key' },
  { url: 'openai_base_url', key: 'openai_api_key', saved: 'has_openai_api_key' },
  { url: 'anthropic_base_url', key: 'anthropic_api_key', saved: 'has_anthropic_api_key' },
] as const;

type UrlKey = (typeof ENDPOINTS)[number]['url'];
type SecretKey = (typeof ENDPOINTS)[number]['key'] | 'runpod_api_key';
type SavedKey = (typeof ENDPOINTS)[number]['saved'];

type AiRequest = Partial<Record<ModelKey | UrlKey | SecretKey, string>> & { provider?: string };

interface AiWrite {
  method: string;
  body: AiRequest | null;
}

interface WorkspaceRow {
  provider: string | null;
  models: Models;
  urls: Record<UrlKey, string | null>;
  keys: Record<SavedKey, boolean>;
  runpod: boolean;
}

const ORGANIZATION_PROVIDER = 'openai';

const organizationModels: Models = {
  model_fast: 'gpt-4o-mini',
  model_reasoning: 'gpt-4o',
  model_embedding: 'text-embedding-3-small',
  model_image: IMAGE_MODEL,
  model_video: VIDEO_MODEL,
  model_audio: AUDIO_MODEL,
};

const organizationSettings = {
  ...organizationModels,
  provider: ORGANIZATION_PROVIDER,
  has_litellm_key: false,
  litellm_host: null,
  has_openai_api_key: true,
  openai_base_url: null,
  has_anthropic_api_key: false,
  anthropic_base_url: null,
  has_runpod_api_key: false,
  bedrock_region: null,
  bedrock_use_iam_role: false,
  has_bedrock_credentials: false,
  completions_routed: true,
};

const inheritedModels: Models = {
  model_fast: null,
  model_reasoning: null,
  model_embedding: null,
  model_image: null,
  model_video: null,
  model_audio: null,
};

function emptyRow(): WorkspaceRow {
  return {
    provider: null,
    models: { ...inheritedModels },
    urls: { litellm_host: null, openai_base_url: null, anthropic_base_url: null },
    keys: { has_litellm_key: false, has_openai_api_key: false, has_anthropic_api_key: false },
    runpod: false,
  };
}

function cleared(value: string): string | null {
  return value.trim() === '' ? null : value;
}

function overrides(row: WorkspaceRow): boolean {
  return (
    row.provider !== null ||
    MODEL_KEYS.some((key) => row.models[key] !== null) ||
    ENDPOINTS.some((endpoint) => row.urls[endpoint.url] !== null || row.keys[endpoint.saved]) ||
    row.runpod
  );
}

function applied(row: WorkspaceRow, request: AiRequest): WorkspaceRow {
  const next: WorkspaceRow = {
    provider: request.provider ?? null,
    models: { ...row.models },
    urls: { ...row.urls },
    keys: { ...row.keys },
    runpod: request.runpod_api_key === undefined ? row.runpod : cleared(request.runpod_api_key) !== null,
  };
  for (const key of MODEL_KEYS) {
    const value = request[key];
    if (value !== undefined) next.models[key] = cleared(value);
  }
  for (const endpoint of ENDPOINTS) {
    const url = request[endpoint.url];
    const key = request[endpoint.key];
    if (url !== undefined) next.urls[endpoint.url] = cleared(url);
    if (key !== undefined) next.keys[endpoint.saved] = cleared(key) !== null;
  }
  return next;
}

function workspaceAiSettings(row: WorkspaceRow) {
  return {
    ...organizationSettings,
    ...row.models,
    ...row.urls,
    ...row.keys,
    provider: row.provider ?? 'self_hosted',
    has_runpod_api_key: row.runpod,
    bedrock_use_iam_role: null,
    overrides: overrides(row),
    inherits_provider: row.provider === null,
    organization_keys: { litellm: false, openai: false, anthropic: false },
  };
}

function effectiveAiSettings(row: WorkspaceRow) {
  const effective = { ...organizationSettings, has_runpod_api_key: row.runpod };
  if (row.provider !== null) {
    if (row.provider !== ORGANIZATION_PROVIDER) {
      for (const key of CHAT_MODEL_KEYS) effective[key] = null;
    }
    effective.provider = row.provider;
  }
  for (const key of MODEL_KEYS) {
    const value = row.models[key];
    if (value !== null) effective[key] = value;
  }
  return effective;
}

test.describe('Workspace Settings Page', () => {
  let aiWrites: AiWrite[] = [];
  let workspaceRow: WorkspaceRow = emptyRow();

  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);

    await mockCommonEndpoints(page);

    aiWrites = [];
    workspaceRow = { ...emptyRow(), models: { ...inheritedModels, model_video: VIDEO_MODEL } };
    await routeApiContext(
      context,
      '**/**/api/organizations/**/workspaces/**/settings/ai/effective**',
      (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(effectiveAiSettings(workspaceRow)),
        });
      }
    );
    await routeApiContext(
      context,
      '**/**/api/organizations/**/workspaces/**/settings/ai',
      (route) => {
        const method = route.request().method();
        if (method === 'PUT') {
          const request: AiRequest = route.request().postDataJSON();
          aiWrites.push({ method, body: request });
          workspaceRow = applied(workspaceRow, request);
        } else if (method === 'DELETE') {
          aiWrites.push({ method, body: null });
          workspaceRow = emptyRow();
          route.fulfill({ status: 204 });
          return;
        }
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(workspaceAiSettings(workspaceRow)),
        });
      }
    );

    await routeApiContext(context, themeRoutePattern, (route) => {
      const method = route.request().method();

      if (method === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ theme: mockTheme }),
        });
      } else if (method === 'PUT') {
        const body = route.request().postDataJSON();
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            theme: { ...mockTheme, ...body },
          }),
        });
      } else if (method === 'DELETE') {
        route.fulfill({ status: 204 });
      }
    });

    await page.goto('/');
    await setupAuth(page);
    await page.reload();

    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

    await page.click('a[href="/settings"]');
    await expect(page).toHaveURL(GENERAL_PATH);
  });

  test.describe('Page Header', () => {
    test('displays page title', async ({ page }) => {
      await expect(page.locator('.page-bar-title')).toHaveText('Settings');
    });
  });

  test.describe('Navigation', () => {
    test('lists the server sections under This server', async ({ page }) => {
      const server = page
        .getByRole('navigation', { name: 'Settings' })
        .getByRole('group', { name: 'This server' });

      await expect(server.getByRole('link', { name: 'Features' })).toHaveAttribute(
        'href',
        '/settings/server/features'
      );
      await expect(server.getByRole('link', { name: 'Connect a device' })).toHaveAttribute(
        'href',
        '/settings/server/connect'
      );

      await server.getByRole('link', { name: 'Features' }).click();
      await expect(page).toHaveURL('/settings/server/features');
      await expect(page.getByRole('heading', { name: 'Features', exact: true })).toBeVisible();

      await server.getByRole('link', { name: 'Connect a device' }).click();
      await expect(page).toHaveURL('/settings/server/connect');
      await expect(page.getByRole('heading', { name: 'Connect a device' })).toBeVisible();
    });

    test('redirects the legacy features tab to the server features section', async ({ page }) => {
      await page.goto('/settings?tab=features');

      await expect(page).toHaveURL('/settings/server/features');
      await expect(page.getByRole('heading', { name: 'Features', exact: true })).toBeVisible();
    });

    test('opens a section from the list and goes back at 375px', async ({ page }) => {
      await page.setViewportSize({ width: 375, height: 812 });
      await page.goto('/settings');

      const list = page.getByRole('navigation', { name: 'Settings' });
      await expect(page).toHaveURL('/settings');
      await expect(page.locator('.page-bar-title')).toHaveText('Settings');
      await expect(list.getByRole('group', { name: 'This server' })).toBeVisible();

      await list.getByRole('link', { name: 'Folders' }).click();
      await expect(page).toHaveURL('/settings/workspace/folders');
      await expect(page.locator('.page-bar-title')).toHaveText('Folders');
      await expect(page.getByRole('heading', { name: 'Host folders' })).toBeVisible();
      await expect(list).toHaveCount(0);

      await page.getByRole('link', { name: 'Back to settings' }).click();
      await expect(page).toHaveURL('/settings');
      await expect(page.locator('.page-bar-title')).toHaveText('Settings');
      await expect(list.getByRole('link', { name: 'Folders' })).toBeVisible();
      await expect(page.getByRole('heading', { name: 'Host folders' })).toHaveCount(0);
    });
  });

  test.describe('Folders', () => {
    test('saves a host folder', async ({ page }) => {
      await page
        .getByRole('navigation', { name: 'Settings' })
        .getByRole('link', { name: 'Folders' })
        .click();
      await expect(page).toHaveURL('/settings/workspace/folders');
      await expect(page.getByRole('heading', { name: 'Host folders' })).toBeVisible();
      await page.getByLabel('Folder 1').fill('/Users/you/Local/jbs');
      await page.getByRole('button', { name: 'Save Changes' }).click();
      await expect(page.getByText('Folders saved')).toBeVisible();
    });
  });

  test.describe('Theme Configuration Section', () => {
    test('displays theme configuration section', async ({ page }) => {
      await expect(page.locator('.section-title')).toContainText('Theme Configuration');
    });

    test('displays light mode color inputs', async ({ page }) => {
      await expect(page.getByRole('heading', { name: 'Light Mode Colors' })).toBeVisible();
      await expect(page.locator('#primary-light')).toBeVisible();
      await expect(page.locator('#secondary-light')).toBeVisible();
    });

    test('displays dark mode color inputs', async ({ page }) => {
      await expect(page.getByRole('heading', { name: 'Dark Mode Colors' })).toBeVisible();
      await expect(page.locator('#primary-dark')).toBeVisible();
      await expect(page.locator('#secondary-dark')).toBeVisible();
    });

    test('loads current theme values', async ({ page }) => {
      await page.waitForTimeout(500);

      const primaryLightInput = page.locator('.color-text-input').first();
      await expect(primaryLightInput).toHaveValue('#3b82f6');
    });
  });

  test.describe('Typography Section', () => {
    test('displays typography settings', async ({ page }) => {
      await expect(page.locator('.card-title').filter({ hasText: 'Typography' })).toBeVisible();
    });

    test('displays font family dropdown', async ({ page }) => {
      await expect(page.locator('#font-family')).toBeVisible();
      await expectSelectOptions(page.getByRole('combobox', { name: 'Font Family' }), [
        'App Default', 'System Default', 'Inter', 'Roboto', 'Open Sans', 'Lato', 'Nunito',
      ]);
    });

    test('displays font size slider', async ({ page }) => {
      await expect(page.locator('#font-size')).toBeVisible();
      await expect(page.locator('.slider-value')).toContainText('16px');
    });

    test('changes font family', async ({ page }) => {
      await chooseSelect(page.getByRole('combobox', { name: 'Font Family' }), 'Inter');
    });

    test('adjusts font size with slider', async ({ page }) => {
      const slider = page.locator('#font-size');

      await slider.fill('18');

      await expect(page.locator('.slider-value')).toContainText('18px');
    });
  });

  test.describe('Appearance Section', () => {
    test('displays corner radius options', async ({ page }) => {
      await expect(page.getByText('Corner Radius', { exact: true })).toBeVisible();
      await expect(page.locator('.radio-option')).toHaveCount(4);
    });

    test('radio buttons for corner radius work', async ({ page }) => {
      await page.click('.radio-option:has-text("Large") input');

      const largeRadio = page.locator('.radio-option:has-text("Large") input');
      await expect(largeRadio).toBeChecked();
    });
  });

  test.describe('Preview Section', () => {
    test('displays preview section', async ({ page }) => {
      await expect(
        page.locator('.preview-box').getByRole('heading', { name: 'Preview' })
      ).toBeVisible();
    });

    test('shows preview text', async ({ page }) => {
      await expect(page.locator('.preview-text')).toContainText('preview of your theme');
    });

    test('shows preview buttons', async ({ page }) => {
      await expect(page.getByRole('button', { name: 'Primary Button' })).toBeVisible();
      await expect(page.getByRole('button', { name: 'Secondary Button' })).toBeVisible();
    });

    test('shows sample preview card', async ({ page }) => {
      await expect(page.locator('.preview-card strong')).toContainText('Sample Card');
    });
  });

  test.describe('Color Picker', () => {
    test('color picker and text input are synced', async ({ page }) => {
      const colorInput = page.locator('#primary-light');
      const textInput = page.locator('.color-input-wrapper').first().locator('.color-text-input');

      await colorInput.fill('#ff0000');

      await expect(textInput).toHaveValue('#ff0000');
    });

    test('text input updates color picker', async ({ page }) => {
      const textInput = page.locator('.color-input-wrapper').first().locator('.color-text-input');

      await textInput.fill('#00ff00');

      await expect(textInput).toHaveValue('#00ff00');
    });
  });

  test.describe('Save Changes', () => {
    test('saves theme successfully', async ({ page }) => {
      await page.locator('.color-input-wrapper').first().locator('.color-text-input').fill('#ff5500');

      await page.click('button:has-text("Save Changes")');

      await expect(page.locator('.alert-success')).toContainText('Settings saved successfully');
    });

    test('shows loading state while saving', async ({ context, page }) => {
      await context.unroute(themeRoutePattern);
      await routeApiContext(
        context,
        themeRoutePattern,
        async (route) => {
          const method = route.request().method();
          if (method === 'PUT') {
            await new Promise((resolve) => setTimeout(resolve, 500));
            route.fulfill({
              status: 200,
              contentType: 'application/json',
              body: JSON.stringify({ theme: mockTheme }),
            });
          } else {
            route.fulfill({
              status: 200,
              contentType: 'application/json',
              body: JSON.stringify({ theme: mockTheme }),
            });
          }
        }
      );

      await page.click('button:has-text("Save Changes")');

      await expect(page.locator('button:has-text("Saving...")')).toBeVisible();
    });

    test('shows error when save fails', async ({ context, page }) => {
      await context.unroute(themeRoutePattern);
      await routeApiContext(
        context,
        themeRoutePattern,
        (route) => {
          const method = route.request().method();
          if (method === 'GET') {
            route.fulfill({
              status: 200,
              contentType: 'application/json',
              body: JSON.stringify({ theme: mockTheme }),
            });
          } else if (method === 'PUT') {
            route.fulfill({
              status: 500,
              contentType: 'application/json',
              body: JSON.stringify({ error: 'Failed to save' }),
            });
          }
        }
      );

      await page.click('button:has-text("Save Changes")');

      await expect(page.locator('.alert-error')).toBeVisible();
    });
  });

  test.describe('Reset to Defaults', () => {
    test('resets theme to defaults', async ({ page }) => {
      await page.locator('.color-input-wrapper').first().locator('.color-text-input').fill('#ff5500');

      await page.click('button:has-text("Reset to Defaults")');

      await expect(page.locator('.alert-success')).toContainText('Settings reset to defaults');
    });

    test('shows error when reset fails', async ({ context, page }) => {
      await context.unroute(themeRoutePattern);
      await routeApiContext(
        context,
        themeRoutePattern,
        (route) => {
          const method = route.request().method();
          if (method === 'GET') {
            route.fulfill({
              status: 200,
              contentType: 'application/json',
              body: JSON.stringify({ theme: mockTheme }),
            });
          } else if (method === 'DELETE') {
            route.fulfill({
              status: 500,
              contentType: 'application/json',
              body: JSON.stringify({ error: 'Failed to reset' }),
            });
          }
        }
      );

      await page.click('button:has-text("Reset to Defaults")');

      await expect(page.locator('.alert-error')).toBeVisible();
    });
  });

  test.describe('Loading State', () => {
    test('shows loading state while fetching theme', async ({ context, page }) => {
      let finishLoading: () => void = () => {};
      const themeCanLoad = new Promise<void>((resolve) => {
        finishLoading = resolve;
      });
      await context.unroute(themeRoutePattern);
      await routeApiContext(
        context,
        themeRoutePattern,
        async (route) => {
          await themeCanLoad;
          await route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ theme: mockTheme }),
          });
        }
      );

      await page.reload();

      await expect(page.locator('.loading-state')).toContainText('Loading theme settings');
      finishLoading();
      await expect(page.getByRole('heading', { name: 'Theme Configuration' })).toBeVisible();
    });
  });

  test.describe('Error State', () => {
    test('shows error when loading theme fails', async ({ context, page }) => {
      await context.unroute(themeRoutePattern);
      await routeApiContext(
        context,
        themeRoutePattern,
        (route) => {
          route.fulfill({
            status: 500,
            contentType: 'application/json',
            body: JSON.stringify({ error: 'Server error' }),
          });
        }
      );

      await page.reload();

      await expect(page.locator('.alert-error')).toBeVisible();
    });
  });

  test.describe('Live Preview', () => {
    test('changes are applied live to preview', async ({ page }) => {
      await page.waitForTimeout(500);

      await chooseSelect(page.getByRole('combobox', { name: 'Font Family' }), 'Roboto');
      await page.locator('#font-size').fill('18');
      await page.click('.radio-option:has-text("Large") input');

      await expect(page.locator('.alert-error')).not.toBeVisible();
    });
  });

  test.describe('Image Model', () => {
    test('inherits the organization ComfyUI checkpoint', async ({ page }) => {
      await page.goto(AI_PATH);
      const row = page.getByRole('group', { name: 'Image Model' });

      await expect(row.locator('.inherited-field-value')).toHaveText(IMAGE_MODEL);
      await expect(row.locator('.inherited-field-badge')).toHaveText(INHERITED);
      await expect(row.getByRole('button', { name: 'Override Image Model' })).toBeVisible();
      await expect(row.getByRole('button', { name: 'Reset Image Model' })).toHaveCount(0);
    });

    test('overriding saves only the image model for this workspace', async ({ page }) => {
      await page.goto(AI_PATH);
      const row = page.getByRole('group', { name: 'Image Model' });

      await row.getByRole('button', { name: 'Override Image Model' }).click();
      await expect(row.getByRole('combobox', { name: 'Image Model' })).toHaveText(IMAGE_MODEL);
      await row.getByRole('button', { name: 'Save Image Model' }).click();

      await expect(page.locator('.alert-success')).toHaveText('Image Model saved');
      expect(aiWrites).toEqual([{ method: 'PUT', body: { model_image: IMAGE_MODEL } }]);
      await expect(row.locator('.inherited-field-badge')).toHaveText(OVERRIDDEN);
      await expect(row.getByRole('button', { name: 'Edit Image Model' })).toBeVisible();
      await expect(row.getByRole('button', { name: 'Reset Image Model' })).toBeVisible();
    });

    test('cancelling an override sends nothing', async ({ page }) => {
      await page.goto(AI_PATH);
      const row = page.getByRole('group', { name: 'Image Model' });

      await row.getByRole('button', { name: 'Override Image Model' }).click();
      await row.getByRole('button', { name: 'Cancel' }).click();

      await expect(row.getByRole('combobox', { name: 'Image Model' })).toHaveCount(0);
      await expect(row.locator('.inherited-field-badge')).toHaveText(INHERITED);
      expect(aiWrites).toEqual([]);
    });
  });

  test.describe('Video Model', () => {
    test('shows the ComfyUI video UNET this workspace sets', async ({ page }) => {
      await page.goto(AI_PATH);
      const row = page.getByRole('group', { name: 'Video Model' });

      await expect(row.locator('.inherited-field-value')).toHaveText(VIDEO_MODEL);
      await expect(row.locator('.inherited-field-badge')).toHaveText(OVERRIDDEN);
      await expect(row.getByRole('button', { name: 'Edit Video Model' })).toBeVisible();
      await expect(row.getByRole('button', { name: 'Reset Video Model' })).toBeVisible();
      await expect(
        page.getByRole('button', { name: 'Reset all AI settings to the organization' })
      ).toBeVisible();
    });

    test('reset clears the video model so the workspace follows the organization', async ({
      page,
    }) => {
      await page.goto(AI_PATH);
      const row = page.getByRole('group', { name: 'Video Model' });

      await row.getByRole('button', { name: 'Reset Video Model' }).click();

      await expect(page.locator('.alert-success')).toHaveText(
        'Video Model follows the organization'
      );
      expect(aiWrites).toEqual([{ method: 'PUT', body: { model_video: '' } }]);
      await expect(row.locator('.inherited-field-badge')).toHaveText(INHERITED);
      await expect(
        page.getByRole('button', { name: 'Reset all AI settings to the organization' })
      ).toHaveCount(0);
    });
  });

  test.describe('Provider', () => {
    test('overriding with another provider sets it for this workspace', async ({ page }) => {
      await page.goto(AI_PATH);
      const row = page.getByRole('group', { name: 'Provider', exact: true });

      await expect(row.locator('.inherited-field-value')).toHaveText('OpenAI');
      await expect(row.locator('.inherited-field-badge')).toHaveText(INHERITED);
      await row.getByRole('button', { name: 'Override Provider' }).click();
      await chooseSelect(row.getByRole('combobox', { name: 'AI Provider' }), 'Anthropic');
      await row.getByRole('button', { name: 'Save Provider' }).click();

      await expect(page.locator('.alert-success')).toHaveText('Provider saved');
      expect(aiWrites).toEqual([
        {
          method: 'PUT',
          body: { provider: 'anthropic', model_fast: '', model_reasoning: '', model_embedding: '' },
        },
      ]);
      await expect(row.locator('.inherited-field-value')).toHaveText('Anthropic');
      await expect(row.locator('.inherited-field-badge')).toHaveText(OVERRIDDEN);
      await expect(row.getByRole('button', { name: 'Reset Provider' })).toBeVisible();
    });

    test('reset asks before removing the saved key, then inherits again', async ({ page }) => {
      workspaceRow = {
        ...workspaceRow,
        provider: 'anthropic',
        keys: { ...workspaceRow.keys, has_anthropic_api_key: true },
      };
      await page.goto(AI_PATH);
      const row = page.getByRole('group', { name: 'Provider', exact: true });
      await expect(row.locator('.inherited-field-badge')).toHaveText(OVERRIDDEN);

      await row.getByRole('button', { name: 'Reset Provider' }).click();
      const dialog = page.getByRole('dialog', { name: 'Reset Provider to the organization?' });
      await expect(dialog.getByRole('listitem')).toHaveText(['Anthropic API key']);
      expect(aiWrites).toEqual([]);
      await dialog.getByRole('button', { name: 'Reset' }).click();

      await expect(page.locator('.alert-success')).toHaveText('Provider follows the organization');
      expect(aiWrites).toEqual([
        {
          method: 'PUT',
          body: {
            model_fast: '',
            model_reasoning: '',
            model_embedding: '',
            anthropic_base_url: '',
            anthropic_api_key: '',
          },
        },
      ]);
      await expect(dialog).toHaveCount(0);
      await expect(row.locator('.inherited-field-value')).toHaveText('OpenAI');
      await expect(row.locator('.inherited-field-badge')).toHaveText(INHERITED);
    });
  });

  test.describe('Audio Model', () => {
    test('overriding saves the audio checkpoint for this workspace', async ({ page }) => {
      await page.goto(AI_PATH);
      const row = page.getByRole('group', { name: 'Audio Model' });

      await expect(row.locator('.inherited-field-value')).toHaveText(AUDIO_MODEL);
      await expect(row.locator('.inherited-field-badge')).toHaveText(INHERITED);
      await row.getByRole('button', { name: 'Override Audio Model' }).click();
      await expect(row.getByRole('combobox', { name: 'Audio Model' })).toHaveText(AUDIO_MODEL);
      await row.getByRole('button', { name: 'Save Audio Model' }).click();

      await expect(page.locator('.alert-success')).toHaveText('Audio Model saved');
      expect(aiWrites).toEqual([{ method: 'PUT', body: { model_audio: AUDIO_MODEL } }]);
      await expect(row.locator('.inherited-field-badge')).toHaveText(OVERRIDDEN);
    });
  });
});
