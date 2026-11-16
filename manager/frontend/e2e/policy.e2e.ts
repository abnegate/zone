import type { Locator, Page } from '@playwright/test';
import type {
  Chat,
  ChatProviderOffer,
  ChatProviders,
  WorkspaceChatProviders,
} from '../src/features/chats/types';
import {
  ACTIVE_ORGANIZATION,
  ACTIVE_WORKSPACE_INHERITED,
  ACTIVE_WORKSPACE_OVERRIDDEN,
  COMMUNITY_ORGANIZATION,
  LAPSED_ORGANIZATION,
} from '../src/features/enterprise/policy/fixtures';
import {
  denialMessage,
  NOT_ALLOWED,
  refusalMessage,
} from '../src/features/enterprise/policy/messages';
import { PolicyCode } from '../src/features/enterprise/policy/schemas';
import type {
  OrganizationPolicyResponse,
  UpdatePolicyRequest,
  WorkspacePolicyResponse,
} from '../src/features/enterprise/policy/types';
import { expect, test } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { installChatSocketMock } from './helpers/chatSocket';
import {
  aiSettingsResponse,
  routeWorkspaceRole,
  setupAdminAuth,
  setupCommonRoutes,
} from './layout-fixtures';
import { blockServiceWorker, routeApi } from './test-utils';

const SAVE_LABEL = 'Save policy';
const ORGANIZATION_SUMMARY = 'Organization';
const EFFECTIVE_SUMMARY = 'In effect';
const INHERITED = 'Inherited from organization';
const OVERRIDDEN = 'Set for this workspace';
const UPGRADE_NOTICE = 'Agent policy is part of Zone Team and Enterprise.';
const LAPSED_NOTICE = 'Your Zone license has lapsed. Policies are still enforced';
const AGENT_POLICY_LINK = 'See the agent policy';

const ORGANIZATION_POLICY_PATH = '/settings/organization/agent-policy';
const WORKSPACE_POLICY_PATH = '/settings/workspace/agent-policy';
const ORGANIZATION_AI_PATH = '/settings/organization/ai';

const ORGANIZATION_POLICY_API = /\/api\/organizations\/[^/]+\/agent-policy$/;
const WORKSPACE_POLICY_API = /\/api\/organizations\/[^/]+\/workspaces\/[^/]+\/agent-policy$/;
const ORGANIZATIONS_API = /\/api\/organizations(\?|$)/;
const ORGANIZATION_AI_API = /\/api\/organizations\/[^/]+\/settings\/ai$/;
const AGENTS_API = /\/api\/organizations\/[^/]+\/agents$/;

const DESKTOP = { width: 1280, height: 800 };
const PHONE = { width: 375, height: 812 };
const WIDTHS = [DESKTOP, PHONE];

const TIMESTAMP = '2026-11-01T09:00:00Z';
const HTTP_EFFORTS: ChatProviderOffer['efforts'] = ['off', 'low', 'medium', 'high'];
const ALLOWED_MODEL = 'qwen3:8b';
const DENIED_MODEL = 'llama3.2:3b';
const MODEL_DENIED = denialMessage(PolicyCode.ModelDenied);
const ANTHROPIC_DENIED = denialMessage(PolicyCode.ProviderDenied, 'Anthropic');
const CODEX_DENIED = denialMessage(PolicyCode.AgentDenied, 'Codex');
const OPENAI_SETTINGS_DENIED = denialMessage(PolicyCode.ProviderDenied, 'OpenAI');
const CODEX_SETTINGS_DENIED = denialMessage(PolicyCode.AgentDenied, 'Codex (ChatGPT subscription)');

interface PolicyServer {
  patches: UpdatePolicyRequest[];
}

async function serveOrganizationPolicy(
  page: Page,
  initial: OrganizationPolicyResponse,
  refuse: (patch: UpdatePolicyRequest) => boolean = () => false
): Promise<PolicyServer> {
  const server: PolicyServer = { patches: [] };
  let current = initial;
  await routeApi(page, ORGANIZATION_POLICY_API, (route) => {
    if (route.request().method() === 'PATCH') {
      const patch = route.request().postDataJSON() as UpdatePolicyRequest;
      server.patches.push(patch);
      if (refuse(patch)) {
        return route.fulfill({
          status: 400,
          json: { error: 'Not a model pattern', code: PolicyCode.ModelInvalid },
        });
      }
      current = { ...current, policy: { ...current.policy, ...patch } };
    }
    return route.fulfill({ status: 200, json: current });
  });
  return server;
}

async function serveWorkspacePolicy(page: Page, policy: WorkspacePolicyResponse): Promise<void> {
  await routeApi(page, WORKSPACE_POLICY_API, (route) =>
    route.fulfill({ status: 200, json: policy })
  );
}

async function openSettings(page: Page): Promise<void> {
  await blockServiceWorker(page.context());
  await setupCommonRoutes(page, false);
  await routeWorkspaceRole(page, 'admin');
  await page.goto('/login', { waitUntil: 'domcontentloaded' });
  await setupAdminAuth(page);
}

async function expectNoSidewaysScroll(page: Page): Promise<void> {
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth
  );
  expect(overflow, 'the page never scrolls sideways').toBeLessThanOrEqual(0);
}

async function shoot(page: Page, view: string): Promise<void> {
  await expectNoSidewaysScroll(page);
  const width = page.viewportSize()?.width ?? DESKTOP.width;
  await page.screenshot({
    path: test.info().outputPath(`${view}-${width}.png`),
    fullPage: true,
    animations: 'disabled',
  });
}

async function shootWhole(page: Page, view: string): Promise<void> {
  await shoot(page, view);
  const viewport = page.viewportSize() ?? DESKTOP;
  const height = await page.evaluate(() =>
    Math.max(...Array.from(document.querySelectorAll('*'), (element) => element.scrollHeight))
  );
  await page.setViewportSize({ width: viewport.width, height: Math.max(viewport.height, height) });
  await expectNoSidewaysScroll(page);
  await page.screenshot({
    path: test.info().outputPath(`${view}-${viewport.width}-whole.png`),
    animations: 'disabled',
  });
}

function field(page: Page, name: string): Locator {
  return page.getByRole('group', { name, exact: true });
}

test.describe('Organization agent policy', () => {
  test('offers the upgrade on Community and edits nothing', async ({ page }) => {
    await openSettings(page);
    await serveOrganizationPolicy(page, COMMUNITY_ORGANIZATION);
    await page.goto(ORGANIZATION_POLICY_PATH, { waitUntil: 'domcontentloaded' });

    await expect(page.getByRole('note').filter({ hasText: UPGRADE_NOTICE })).toBeVisible();
    await expect(page.getByRole('button', { name: SAVE_LABEL })).toHaveCount(0);
  });

  test('saves only the changed field and shows a refusal beside its field', async ({ page }) => {
    await openSettings(page);
    const server = await serveOrganizationPolicy(page, ACTIVE_ORGANIZATION, (patch) =>
      Boolean(patch.models?.includes('gpt-5**'))
    );
    await page.goto(ORGANIZATION_POLICY_PATH, { waitUntil: 'domcontentloaded' });

    const servers = field(page, 'MCP servers');
    await servers.getByRole('checkbox', { name: 'Restrict MCP servers' }).click();
    await servers.getByRole('textbox', { name: 'Add to MCP servers' }).fill('github');
    await servers.getByRole('button', { name: 'Add to MCP servers' }).click();
    await page.getByRole('button', { name: SAVE_LABEL }).click();

    await expect.poll(() => server.patches).toEqual([{ mcp_servers: ['github'] }]);

    const models = field(page, 'Models');
    await models.getByRole('textbox', { name: 'Add to Models' }).fill('gpt-5**');
    await models.getByRole('button', { name: 'Add to Models' }).click();
    await page.getByRole('button', { name: SAVE_LABEL }).click();

    await expect(models.getByRole('alert')).toHaveText(refusalMessage(PolicyCode.ModelInvalid));
    expect(server.patches.at(-1)).toEqual({ models: ['claude-*', 'qwen3', 'gpt-5**'] });
  });

  test('keeps a lapsed policy read-only and says it is still enforced', async ({ page }) => {
    await openSettings(page);
    await serveOrganizationPolicy(page, LAPSED_ORGANIZATION);
    await page.goto(ORGANIZATION_POLICY_PATH, { waitUntil: 'domcontentloaded' });

    await expect(page.getByRole('status').filter({ hasText: LAPSED_NOTICE })).toBeVisible();
    await expect(page.getByRole('button', { name: SAVE_LABEL })).toHaveCount(0);
    await expect(
      field(page, 'Providers').getByRole('checkbox', { name: 'AWS Bedrock' })
    ).toBeDisabled();
  });

  for (const viewport of WIDTHS) {
    test(`lays the editor out at ${viewport.width}px`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await openSettings(page);
      await serveOrganizationPolicy(page, ACTIVE_ORGANIZATION);
      await page.goto(ORGANIZATION_POLICY_PATH, { waitUntil: 'domcontentloaded' });

      await expect(field(page, 'Providers')).toBeVisible();
      await expect(page.getByRole('button', { name: SAVE_LABEL })).toBeVisible();
      await shootWhole(page, 'organization');
    });
  }
});

test.describe('Workspace agent policy', () => {
  test('shows what it inherits from the organization', async ({ page }) => {
    await openSettings(page);
    await serveWorkspacePolicy(page, ACTIVE_WORKSPACE_INHERITED);
    await page.goto(WORKSPACE_POLICY_PATH, { waitUntil: 'domcontentloaded' });

    const providers = field(page, 'Providers');
    await expect(providers.getByText(INHERITED)).toBeVisible();
    await expect(providers).toContainText(
      `${ORGANIZATION_SUMMARY}Local models, Anthropic API, AWS Bedrock`
    );
    await expect(providers.getByRole('button', { name: 'Override Providers' })).toBeVisible();
  });

  test('shows an override, what is in effect, and disables what the organization denies', async ({
    page,
  }) => {
    await openSettings(page);
    await serveWorkspacePolicy(page, ACTIVE_WORKSPACE_OVERRIDDEN);
    await page.goto(WORKSPACE_POLICY_PATH, { waitUntil: 'domcontentloaded' });

    const providers = field(page, 'Providers');
    await expect(providers.getByText(OVERRIDDEN)).toBeVisible();
    await expect(providers).toContainText(`${EFFECTIVE_SUMMARY}Local models`);
    await expect(providers.getByRole('checkbox', { name: 'Local models' })).toBeChecked();
    await expect(providers.getByRole('checkbox', { name: 'Anthropic API' })).toBeEnabled();
    const openai = providers.getByRole('checkbox', { name: 'OpenAI API' });
    await expect(openai).toBeDisabled();
    await expect(providers.locator('.policy-option--denied')).toHaveCount(4);
    await expect(providers.getByText(NOT_ALLOWED).first()).toBeVisible();
  });

  for (const viewport of WIDTHS) {
    test(`lays the editor out at ${viewport.width}px`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await openSettings(page);
      await serveWorkspacePolicy(page, ACTIVE_WORKSPACE_OVERRIDDEN);
      await page.goto(WORKSPACE_POLICY_PATH, { waitUntil: 'domcontentloaded' });

      await expect(field(page, 'Providers')).toBeVisible();
      await shootWhole(page, 'workspace');
    });
  }
});

test.describe('AI settings under an agent policy', () => {
  async function openAiDefaults(page: Page, provider: string): Promise<void> {
    await openSettings(page);
    await serveOrganizationPolicy(page, ACTIVE_ORGANIZATION);
    await routeApi(page, ORGANIZATIONS_API, (route) =>
      route.fulfill({
        json: {
          organizations: [
            {
              id: 'org-1',
              name: 'Acme Corp',
              slug: 'acme-corp',
              description: null,
              is_active: true,
              role: 'owner',
              created_at: '2024-01-01T00:00:00Z',
              updated_at: '2024-01-01T00:00:00Z',
            },
          ],
        },
      })
    );
    await routeApi(page, ORGANIZATION_AI_API, (route) => {
      const path = new URL(route.request().url()).pathname;
      const saved = aiSettingsResponse(path);
      return route.fulfill({ json: { ...saved, provider } });
    });
    await routeApi(page, AGENTS_API, (route) =>
      route.fulfill({
        json: {
          agents: [agentStatus('claude'), agentStatus('codex', PolicyCode.AgentDenied)],
        },
      })
    );
    await page.goto(ORGANIZATION_AI_PATH, { waitUntil: 'domcontentloaded' });
  }

  test('disables the providers the policy denies, with the reason', async ({ page }) => {
    await openAiDefaults(page, 'self_hosted');

    const trigger = page.getByRole('combobox', { name: 'AI Provider' });
    await expect(trigger).toBeVisible();
    await trigger.click();
    const openai = page.getByRole('option', { name: 'OpenAI', exact: true });
    await expect(openai).toHaveAttribute('data-disabled', '');
    await expect(openai).toContainText(OPENAI_SETTINGS_DENIED);
    await expect(page.getByRole('option', { name: 'Codex (ChatGPT subscription)' })).toContainText(
      CODEX_SETTINGS_DENIED
    );
    await expect(page.getByRole('option', { name: 'Anthropic', exact: true })).not.toHaveAttribute(
      'data-disabled'
    );
  });

  test('disables signing in to an agent the policy denies', async ({ page }) => {
    await openAiDefaults(page, 'codex');

    const panel = page.getByRole('region', { name: 'Codex sign-in' });
    await expect(panel.getByTestId('agent-sign-in-denial')).toHaveText(CODEX_DENIED);
    await expect(panel.getByRole('button', { name: 'Sign in with ChatGPT' })).toBeDisabled();
    await expect(page.getByTestId('ai-provider-denial')).toHaveText(CODEX_SETTINGS_DENIED);
  });
});

function agentStatus(agent: 'claude' | 'codex', denial?: string) {
  return {
    agent,
    provider: agent === 'claude' ? 'claude_code' : 'codex',
    state: 'signed_out',
    source: null,
    label: null,
    expires_at: null,
    models: [],
    pending: null,
    error: null,
    logins: [],
    ...(denial ? { denial } : {}),
  };
}

const selfHosted: ChatProviderOffer = {
  provider: 'self_hosted',
  label: 'Self-Hosted',
  usable: true,
  reason: null,
  models: [{ name: ALLOWED_MODEL, label: ALLOWED_MODEL, efforts: HTTP_EFFORTS }],
  efforts: HTTP_EFFORTS,
  requires_model: false,
};

const anthropic: ChatProviderOffer = {
  provider: 'anthropic',
  label: 'Anthropic',
  usable: false,
  reason: ANTHROPIC_DENIED,
  denial: PolicyCode.ProviderDenied,
  models: [],
  efforts: HTTP_EFFORTS,
  requires_model: false,
};

const codex: ChatProviderOffer = {
  provider: 'codex',
  label: 'Codex',
  usable: false,
  reason: CODEX_DENIED,
  denial: PolicyCode.AgentDenied,
  models: [],
  efforts: HTTP_EFFORTS,
  requires_model: false,
};

const offers = [selfHosted, anthropic, codex];

const installed = [
  { name: ALLOWED_MODEL, completion: true },
  { name: DENIED_MODEL, completion: true, denial: PolicyCode.ModelDenied },
].map((model) => ({ ...model, size: 2_000_000_000, modified_at: TIMESTAMP, tools: true }));

const chat: Chat = {
  id: 'chat-policy',
  title: 'Quarterly planning',
  model_name: 'auto',
  created_at: TIMESTAMP,
  updated_at: TIMESTAMP,
  archived: false,
  agent_enabled: false,
  reasoning_effort: 'auto',
  provider: null,
  provider_model: null,
};

async function openChats(page: Page): Promise<Awaited<ReturnType<typeof installChatSocketMock>>> {
  await blockServiceWorker(page.context());
  const socket = await installChatSocketMock(page);
  await mockCommonEndpoints(page);
  const providers: ChatProviders = { default: selfHosted, selected: null, providers: offers };
  const workspace: WorkspaceChatProviders = { default: selfHosted, providers: offers };
  await routeApi(page, /\/api\/models(\?|$)/, (route) =>
    route.fulfill({ json: { models: installed } })
  );
  await routeApi(page, /\/api\/workspaces\/[^/]+\/chat-providers(\?|$)/, (route) =>
    route.fulfill({ json: workspace })
  );
  await routeApi(page, /\/api\/chats($|\?|\/)/i, async (route) => {
    const [, id, action] =
      new URL(route.request().url()).pathname.match(/^\/api\/chats(?:\/([^/]+))?(?:\/(.+))?$/) ??
      [];
    if (!id) return route.fulfill({ json: { chats: [chat] } });
    if (action === 'providers') return route.fulfill({ json: providers });
    if (action === 'sources') return route.fulfill({ json: { sources: [] } });
    if (action === 'messages') return route.fulfill({ json: { messages: [] } });
    if (action === 'tasks') return route.fulfill({ json: { tasks: [] } });
    if (action === 'subagents') return route.fulfill({ json: { subagents: [] } });
    if (action) return route.fulfill({ status: 404, json: { error: 'Not mocked' } });
    return route.fulfill({ json: { chat: { ...chat, context: null, messages: [] } } });
  });
  await setupAuth(page);
  return socket;
}

test.describe('Chats under an agent policy', () => {
  for (const viewport of WIDTHS) {
    test(`disables a denied provider with its reason at ${viewport.width}px`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await openChats(page);
      await page.goto(`/chats?id=${chat.id}`);
      await expect(page.getByRole('heading', { name: chat.title })).toBeVisible();
      await page.getByTestId('chat-settings').click();

      const provider = page.getByTestId('chat-provider');
      await provider.click();
      const denied = page.getByRole('option', { name: 'Anthropic', exact: true });
      await expect(denied).toHaveAttribute('data-disabled', '');
      await expect(denied).toContainText(ANTHROPIC_DENIED);
      await expect(page.getByRole('option', { name: 'Codex', exact: true })).toContainText(
        CODEX_DENIED
      );
      await expect(
        page.getByRole('option', { name: 'Self-Hosted', exact: true })
      ).not.toHaveAttribute('data-disabled');
      await expect(denied).toBeInViewport({ ratio: 1 });
      await shoot(page, 'chat-picker');
    });
  }

  test('disables a denied model in the new chat dialog, with the reason', async ({ page }) => {
    await openChats(page);
    await page.goto('/chats');
    await page.getByRole('button', { name: 'New chat' }).first().click();
    const dialog = page.getByRole('dialog');
    await dialog.getByRole('combobox', { name: 'Select Model' }).click();

    const denied = page.getByRole('option', { name: DENIED_MODEL, exact: true });
    await expect(denied).toHaveAttribute('data-disabled', '');
    await expect(denied).toContainText(MODEL_DENIED);
    await expect(
      page.getByRole('option', { name: ALLOWED_MODEL, exact: true })
    ).not.toHaveAttribute('data-disabled');
  });

  for (const viewport of WIDTHS) {
    test(`explains a denied turn and links to the policy at ${viewport.width}px`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      const socket = await openChats(page);
      socket.setOnSend(async () => {
        await socket.emit({
          type: 'denied',
          message: 'Your organization denied this turn.',
          code: PolicyCode.ModelDenied,
        });
      });
      await page.goto(`/chats?id=${chat.id}`);
      await expect(page.locator('.message-form textarea')).toBeVisible();
      await page.fill('.message-form textarea', 'Summarise the plan');
      await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();

      const denial = page.getByTestId('chat-denial');
      await expect(denial).toHaveClass(/chats-error/);
      await expect(denial).toContainText(MODEL_DENIED);
      await expect(denial).not.toContainText('Your organization denied this turn.');
      const link = denial.getByRole('link', { name: AGENT_POLICY_LINK });
      await expect(link).toHaveAttribute('href', WORKSPACE_POLICY_PATH);
      await expect(page.locator('.message-form textarea')).toBeEnabled();
      await shoot(page, 'chat-denial');
    });
  }
});
