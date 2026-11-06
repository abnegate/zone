import path from 'node:path';
import type { Locator, Page } from '@playwright/test';
import type {
  Chat,
  ChatProviderOffer,
  ChatProviders,
  ReasoningEffort,
  SetChatProviderRequest,
  WorkspaceChatProviders,
} from '../src/features/chats/types';
import type { AiProvider } from '../src/features/settings/workspace/types';
import { expect, test } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { installChatSocketMock } from './helpers/chatSocket';
import { blockServiceWorker, routeApi } from './test-utils';

const WORKSPACE = '00000000-0000-0000-0000-000000000001';
const TIMESTAMP = '2026-11-01T09:00:00Z';
const HTTP_EFFORTS: ReasoningEffort[] = ['off', 'low', 'medium', 'high'];
const CLAUDE_EFFORTS: ReasoningEffort[] = ['low', 'medium', 'high', 'xhigh', 'max'];
const UP_TO_XHIGH: ReasoningEffort[] = ['low', 'medium', 'high', 'xhigh'];
const UP_TO_MAX: ReasoningEffort[] = [...UP_TO_XHIGH, 'max'];
const UP_TO_ULTRA: ReasoningEffort[] = [...UP_TO_MAX, 'ultra'];
const ANTHROPIC_REASON = 'No key, endpoint or credentials are saved for Anthropic in AI Settings.';

const selfHosted: ChatProviderOffer = {
  provider: 'self_hosted',
  label: 'Self-Hosted',
  usable: true,
  reason: null,
  models: [
    { name: 'llama3.2:3b', label: 'llama3.2:3b', efforts: HTTP_EFFORTS },
    { name: 'qwen3:8b', label: 'qwen3:8b', efforts: HTTP_EFFORTS },
  ],
  efforts: HTTP_EFFORTS,
  requires_model: false,
};

const openai: ChatProviderOffer = {
  provider: 'openai',
  label: 'OpenAI',
  usable: true,
  reason: null,
  models: [],
  efforts: HTTP_EFFORTS,
  requires_model: true,
};

const anthropic: ChatProviderOffer = {
  provider: 'anthropic',
  label: 'Anthropic',
  usable: false,
  reason: ANTHROPIC_REASON,
  models: [],
  efforts: HTTP_EFFORTS,
  requires_model: false,
};

const claudeCode: ChatProviderOffer = {
  provider: 'claude_code',
  label: 'Claude Code',
  usable: true,
  reason: null,
  models: [
    { name: 'sonnet', label: 'Sonnet', efforts: CLAUDE_EFFORTS },
    { name: 'opus', label: 'Opus', efforts: CLAUDE_EFFORTS },
    { name: 'haiku', label: 'Haiku', efforts: [] },
    { name: 'fable', label: 'Fable', efforts: CLAUDE_EFFORTS },
  ],
  efforts: CLAUDE_EFFORTS,
  requires_model: false,
};

const codex: ChatProviderOffer = {
  provider: 'codex',
  label: 'Codex',
  usable: true,
  reason: null,
  models: [
    { name: 'gpt-6-astra', label: 'gpt-6-astra', efforts: UP_TO_ULTRA },
    { name: 'gpt-6-sol', label: 'gpt-6-sol', efforts: UP_TO_ULTRA },
    { name: 'gpt-6-luna', label: 'gpt-6-luna', efforts: UP_TO_MAX },
    { name: 'gpt-5.6-sol', label: 'gpt-5.6-sol', efforts: UP_TO_ULTRA },
    { name: 'gpt-5.6-terra', label: 'gpt-5.6-terra', efforts: UP_TO_ULTRA },
    { name: 'gpt-5.6-luna', label: 'gpt-5.6-luna', efforts: UP_TO_MAX },
    { name: 'gpt-5.5', label: 'gpt-5.5', efforts: UP_TO_XHIGH },
  ],
  efforts: UP_TO_XHIGH,
  requires_model: false,
};

const offers: ChatProviderOffer[] = [selfHosted, openai, anthropic, claudeCode, codex];
const choosable = offers.filter((offer) => offer.usable);
const workspaceProviders: WorkspaceChatProviders = { default: selfHosted, providers: choosable };

const installed = ['llama3.2:3b', 'qwen3:8b'].map((name) => ({
  name,
  size: 2_000_000_000,
  modified_at: TIMESTAMP,
  completion: true,
  tools: true,
  capabilities: ['text', 'tools', 'reasoning'],
}));

function chat(id: string, title: string, overrides: Partial<Chat> = {}): Chat {
  return {
    id,
    title,
    model_name: 'auto',
    created_at: TIMESTAMP,
    updated_at: TIMESTAMP,
    archived: false,
    agent_enabled: true,
    reasoning_effort: 'auto',
    provider: null,
    provider_model: null,
    ...overrides,
  };
}

type Captured = { provider: SetChatProviderRequest[]; updates: Array<Partial<Chat>> };

async function mockChats(page: Page): Promise<Captured> {
  const chats = new Map<string, Chat>(
    [
      chat('chat-default', 'Release notes draft'),
      chat('chat-codex', 'Refactor the indexer', {
        provider: 'codex',
        provider_model: 'gpt-6-sol',
        reasoning_effort: 'ultra',
      }),
      chat('chat-unusable', 'Contract summary', { provider: 'anthropic' }),
    ].map((entry) => [entry.id, entry])
  );
  const captured: Captured = { provider: [], updates: [] };
  const offerOf = (provider: AiProvider) =>
    offers.find((offer) => offer.provider === provider) ?? selfHosted;
  const providersOf = (entry: Chat): ChatProviders => ({
    default: selfHosted,
    selected: entry.provider
      ? { ...offerOf(entry.provider), model: entry.provider_model ?? null }
      : null,
    providers: choosable,
    cli: entry.provider === 'claude_code' || entry.provider === 'codex',
  });
  const withMessages = (entry: Chat) => ({
    ...entry,
    context: null,
    messages: [
      {
        id: `${entry.id}-welcome`,
        chat_id: entry.id,
        role: 'assistant',
        content: 'Ready when you are.',
        created_at: TIMESTAMP,
      },
    ],
  });

  await routeApi(page, /\/api\/models(\?|$)/, (route) =>
    route.fulfill({ json: { models: installed } })
  );
  await routeApi(page, /\/api\/workspaces\/[^/]+\/chat-providers(\?|$)/, (route) =>
    route.fulfill({ json: workspaceProviders })
  );
  await routeApi(page, /\/api\/chats($|\?|\/)/i, async (route) => {
    const request = route.request();
    const [, id, action] =
      new URL(request.url()).pathname.match(/^\/api\/chats(?:\/([^/]+))?(?:\/(.+))?$/) ?? [];
    const entry = id ? chats.get(id) : undefined;
    if (!id) {
      await route.fulfill({ json: { chats: [...chats.values()] } });
      return;
    }
    if (!entry) {
      await route.fulfill({ status: 404, json: { error: 'Chat not found' } });
      return;
    }
    if (action === 'providers') {
      await route.fulfill({ json: providersOf(entry) });
      return;
    }
    if (action === 'provider') {
      const body = request.postDataJSON() as SetChatProviderRequest;
      captured.provider.push(body);
      entry.provider = body.provider;
      entry.provider_model = body.model ?? null;
      await route.fulfill({ json: { chat: withMessages(entry) } });
      return;
    }
    if (action === 'sources') {
      await route.fulfill({ json: { sources: [] } });
      return;
    }
    if (action === 'messages') {
      await route.fulfill({ json: { messages: withMessages(entry).messages } });
      return;
    }
    if (action) {
      await route.fulfill({ status: 404, json: { error: 'Not mocked' } });
      return;
    }
    if (request.method() === 'PUT') {
      const body = request.postDataJSON() as Partial<Chat>;
      captured.updates.push(body);
      Object.assign(entry, body);
    }
    await route.fulfill({ json: { chat: withMessages(entry) } });
  });
  return captured;
}

async function optionLabels(page: Page): Promise<string[]> {
  const listbox = page.getByRole('listbox');
  await expect(listbox).toBeVisible();
  return (await listbox.getByRole('option').allTextContents()).map((label) => label.trim());
}

async function choose(page: Page, trigger: Locator, option: string): Promise<void> {
  await trigger.click();
  await page.getByRole('option', { name: option, exact: true }).click();
  await expect(page.getByRole('listbox')).toHaveCount(0);
}

async function closeMenu(page: Page): Promise<void> {
  await page.keyboard.press('Escape');
  await expect(page.getByRole('listbox')).toHaveCount(0);
}

async function expectNoOverflow(page: Page): Promise<void> {
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
    'the page never scrolls sideways'
  ).toBe(true);
}

async function expectReadableTitle(page: Page): Promise<void> {
  const heading = page.locator('.chat-header h3');
  if ((await heading.count()) === 0) return;
  const title = (await heading.textContent()) ?? '';
  expect(
    (await heading.boundingBox())?.width ?? 0,
    'the header controls leave the chat title readable'
  ).toBeGreaterThanOrEqual(Math.min(title.length * 6, 120));
}

for (const width of [1280, 1024, 900, 390]) {
  for (const theme of ['light', 'dark'] as const) {
    test(`chat provider pickers at ${width}px in ${theme}`, async ({ context, page }) => {
      test.slow();
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ reducedMotion: 'reduce', colorScheme: theme });
      await blockServiceWorker(context);
      await installChatSocketMock(page);
      await mockCommonEndpoints(page);
      const captured = await mockChats(page);
      await page.addInitScript((value) => localStorage.setItem('manager_theme', value), theme);
      await setupAuth(page);

      const shoot = async (name: string) => {
        await expectNoOverflow(page);
        await expectReadableTitle(page);
        await page.screenshot({
          path: path.join(
            test.info().project.outputDir,
            `chat-provider-${width}-${theme}-${name}.png`
          ),
          fullPage: true,
          animations: 'disabled',
        });
      };
      const openChat = async (id: string, title: string) => {
        await page.goto(`/chats?id=${id}`);
        await expect(page.getByRole('heading', { name: title })).toBeVisible();
        await page.getByTestId('chat-settings').click();
        await expect(page.getByTestId('chat-provider')).toBeVisible();
      };
      const closeSettings = async () => {
        await page.keyboard.press('Escape');
        await expect(page.getByTestId('chat-settings')).toHaveAttribute('aria-expanded', 'false');
      };
      const providerSelect = page.getByTestId('chat-provider');
      const modelSelect = page.getByTestId('chat-provider-model');
      const reasoningSelect = page.getByTestId('reasoning-effort');
      const permissions = page.getByTestId('agent-permissions');
      const sandbox = page.getByTestId('agent-sandbox-toggle');

      await openChat('chat-default', 'Release notes draft');
      await closeSettings();
      await permissions.click();
      await expect(page.getByTestId('auto-approve-toggle')).toBeVisible();
      await expect(sandbox).toHaveCount(0);
      await shoot('default-permissions');
      await page.keyboard.press('Escape');
      await page.getByTestId('chat-settings').click();
      await expect(providerSelect).toHaveText('ProviderDefault');
      await expect(providerSelect).toHaveAttribute('title', /\(Self-Hosted\)/);
      await expect(modelSelect).toContainText('Automatic');
      await expect(page.getByTestId('chat-provider-error')).toHaveCount(0);
      await shoot('default');

      await modelSelect.click();
      expect(await optionLabels(page)).toEqual(['Automatic', 'llama3.2:3b', 'qwen3:8b']);
      await shoot('default-models');
      await page.getByRole('option', { name: 'qwen3:8b', exact: true }).click();
      await expect.poll(() => captured.updates).toEqual([{ model_name: 'qwen3:8b' }]);
      await expect(modelSelect).toContainText('qwen3:8b');
      await expect(page.locator('.chat-model')).toHaveText('qwen3:8b');
      await expect(page.getByTestId('chat-operation-error')).toHaveCount(0);

      await providerSelect.click();
      expect(await optionLabels(page)).toEqual([
        'Default (Self-Hosted)',
        'Self-Hosted',
        'OpenAI',
        'Claude Code',
        'Codex',
      ]);
      await shoot('default-providers');
      await page.getByRole('option', { name: 'Claude Code', exact: true }).click();
      await expect
        .poll(() => captured.provider)
        .toEqual([{ provider: 'claude_code', model: null }]);
      await expect(providerSelect).toContainText('Claude Code');
      await expect(modelSelect).toContainText('Automatic');
      await expect(page.locator('.chat-settings-provider')).toHaveText('Claude Code');
      await expect(page.locator('.chat-model')).toHaveText('Automatic');
      await closeSettings();
      await permissions.click();
      await expect(sandbox).toBeVisible();
      await expect(sandbox).toHaveAttribute('aria-pressed', 'true');
      await shoot('claude-code-permissions');
      await page.keyboard.press('Escape');
      await page.getByTestId('chat-settings').click();
      await modelSelect.click();
      expect(await optionLabels(page)).toEqual(['Automatic', 'Sonnet', 'Opus', 'Haiku', 'Fable']);
      await shoot('claude-code-models');
      await closeMenu(page);
      await reasoningSelect.click();
      expect(await optionLabels(page)).toEqual([
        'Auto',
        'Low',
        'Medium',
        'High',
        'Extra high',
        'Max',
      ]);
      await shoot('claude-code-reasoning');
      await closeMenu(page);

      await openChat('chat-codex', 'Refactor the indexer');
      await expect(providerSelect).toContainText('Codex');
      await expect(modelSelect).toContainText('gpt-6-sol');
      await expect(reasoningSelect).toContainText('Ultra');
      await reasoningSelect.click();
      expect(await optionLabels(page)).toEqual([
        'Auto',
        'Low',
        'Medium',
        'High',
        'Extra high',
        'Max',
        'Ultra',
      ]);
      await shoot('codex-gpt-6-sol-reasoning');
      await closeMenu(page);
      await modelSelect.click();
      expect(await optionLabels(page)).toEqual([
        'Automatic',
        'gpt-6-astra',
        'gpt-6-sol',
        'gpt-6-luna',
        'gpt-5.6-sol',
        'gpt-5.6-terra',
        'gpt-5.6-luna',
        'gpt-5.5',
      ]);
      await shoot('codex-models');
      await page.getByRole('option', { name: 'gpt-5.5', exact: true }).click();
      await expect
        .poll(() => captured.provider.at(-1))
        .toEqual({ provider: 'codex', model: 'gpt-5.5' });
      await expect.poll(() => captured.updates.at(-1)).toEqual({ reasoning_effort: 'auto' });
      await expect(modelSelect).toContainText('gpt-5.5');
      await expect(reasoningSelect).toContainText('Auto');
      await reasoningSelect.click();
      expect(await optionLabels(page)).toEqual(['Auto', 'Low', 'Medium', 'High', 'Extra high']);
      await shoot('codex-gpt-5.5-reasoning');
      await closeMenu(page);

      await openChat('chat-unusable', 'Contract summary');
      await expect(page.getByRole('alert').filter({ hasText: ANTHROPIC_REASON })).toBeVisible();
      await expect(providerSelect).toContainText('Anthropic (unavailable)');
      await shoot('unusable');

      await choose(page, providerSelect, 'OpenAI');
      const modelName = page.getByRole('textbox', { name: 'Provider model name' });
      await expect(modelName).toHaveValue('');
      await expect(modelName).toHaveAttribute('placeholder', 'Type a model name');
      await expect(modelSelect).toHaveCount(0);
      expect(captured.provider.some((body) => body.provider === 'openai')).toBe(false);
      await shoot('openai-name-model');
      await modelName.fill('my-dep');
      await expect(page.getByText('Enter to save')).toBeVisible();
      await shoot('openai-unsaved-name');
      await reasoningSelect.focus();
      await page.getByTestId('chat-settings').click();
      await expect(page.getByTestId('chat-settings')).toHaveAttribute('aria-expanded', 'false');
      await page.getByTestId('chat-settings').click();
      await expect(modelName).toHaveValue('my-dep');
      await expect(page.getByText('Enter to save')).toBeVisible();
      await expect(page.locator('.chat-settings-provider')).toHaveText('Anthropic');
      expect(captured.provider.some((body) => body.provider === 'openai')).toBe(false);

      await page.route(
        /\/api\/chats\/chat-unusable\/provider$/,
        (route) => route.fulfill({ status: 409, json: { error: 'OpenAI refused that model.' } }),
        { times: 1 }
      );
      await modelName.fill('refused-name');
      await modelName.press('Enter');
      await expect(page.getByText('OpenAI refused that model.')).toBeVisible();
      await expect(modelName).toBeFocused();
      await expect(modelName).toHaveValue('refused-name');
      await expect(providerSelect).toContainText('OpenAI');
      expect(captured.provider.some((body) => body.provider === 'openai')).toBe(false);

      await page.keyboard.press('Escape');
      await expect(page.getByTestId('chat-settings')).toHaveAttribute('aria-expanded', 'false');
      await page.getByTestId('chat-settings').click();
      await expect(providerSelect).toContainText('Anthropic');
      await expect(modelName).toHaveCount(0);

      await choose(page, providerSelect, 'OpenAI');
      await modelName.fill('my-deployment');
      await modelName.press('Enter');
      await expect
        .poll(() => captured.provider.at(-1))
        .toEqual({ provider: 'openai', model: 'my-deployment' });
      await expect(page.locator('.chat-settings-provider')).toHaveText('OpenAI');
      await expect(page.locator('.chat-model')).toHaveText('my-deployment');
      await expect(modelName).toHaveValue('my-deployment');
      await expect(modelName).toBeFocused();
      await shoot('openai-named-model');

      await page.goto('/chats');
      if (width < 768) await expect(page.locator('.chat-item')).toHaveCount(3);
      await page.getByRole('button', { name: 'New chat' }).first().click();
      const dialog = page.getByRole('dialog');
      await expect(dialog.getByRole('heading', { name: 'New Chat' })).toBeVisible();
      const dialogProvider = dialog.getByRole('combobox', { name: 'Provider' });
      const dialogModel = dialog.getByRole('combobox', { name: 'Select Model' });
      const dialogReasoning = dialog.getByRole('combobox', { name: 'Reasoning' });

      await expect(dialogProvider).toContainText('Default (Self-Hosted)');
      await dialogModel.click();
      expect(await optionLabels(page)).toEqual(['Automatic', 'llama3.2:3b', 'qwen3:8b']);
      await shoot('new-default');
      await closeMenu(page);

      await choose(page, dialogProvider, 'Claude Code');
      await dialogModel.click();
      expect(await optionLabels(page)).toEqual(['Automatic', 'Sonnet', 'Opus', 'Haiku', 'Fable']);
      await shoot('new-claude-code');
      await closeMenu(page);
      await expect(dialogReasoning).toBeVisible();

      await choose(page, dialogProvider, 'Codex');
      await dialogModel.click();
      expect(await optionLabels(page)).toContain('gpt-6-sol');
      await shoot('new-codex');
      await page.getByRole('option', { name: 'gpt-6-sol', exact: true }).click();
      await expect(
        dialog.getByText('Choose Automatic to let Codex pick its own model.')
      ).toBeVisible();
      await dialogReasoning.click();
      expect(await optionLabels(page)).toContain('Ultra');
      await shoot('new-codex-reasoning');
      await closeMenu(page);

      await choose(page, dialogProvider, 'Self-Hosted');
      await choose(page, dialogModel, 'qwen3:8b');
      await dialog.getByRole('checkbox', { name: 'Agent mode', exact: true }).check();
      await expect(
        dialog.getByRole('checkbox', { name: 'Auto-approve writes and commands', exact: true })
      ).toBeVisible();
      await shoot('new-self-hosted-agent');

      await choose(page, dialogProvider, 'OpenAI');
      await expect(
        dialog.getByText('AI Settings save no model for OpenAI, so name one it serves.')
      ).toBeVisible();
      await expect(dialogModel).toHaveCount(0);
      const dialogModelName = dialog.getByRole('textbox', { name: 'Model name' });
      await expect(dialogModelName).toHaveValue('');
      const createChat = dialog.getByRole('button', { name: 'Create Chat' });
      await expect(createChat).toBeDisabled();
      await dialogModelName.fill('   ');
      await expect(createChat).toBeDisabled();
      await dialogModelName.fill('my-deployment');
      await expect(createChat).toBeEnabled();
      await shoot('new-openai');

      await dialog.getByRole('button', { name: 'Cancel' }).click();
      await expect(dialog).toHaveCount(0);
      await page.getByRole('button', { name: 'New chat' }).first().click();
      await expect(dialogProvider).toContainText('Default (Self-Hosted)');
      await expect(dialogModel).toContainText('Automatic');
    });
  }
}
