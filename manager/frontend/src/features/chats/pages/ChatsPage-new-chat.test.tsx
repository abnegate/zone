import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';

import type {
  ChatProviderOffer,
  ReasoningEffort,
  RecentChat,
  WorkspaceChatProviders,
} from '../types';

const REFUSAL =
  "This workspace's AI endpoint can't be used: its AI settings couldn't be read. Check AI Settings.";

const UNAVAILABLE = 'Codex is not signed in for this organization.';

const PROVIDERS_REFUSED = 'Failed to load AI providers: Not found';

const HTTP_EFFORTS: ReasoningEffort[] = ['off', 'low', 'medium', 'high'];
const CLAUDE_EFFORTS: ReasoningEffort[] = ['low', 'medium', 'high', 'xhigh', 'max'];
const CODEX_EFFORTS: ReasoningEffort[] = ['low', 'medium', 'high', 'xhigh'];

const selfHosted: ChatProviderOffer = {
  provider: 'self_hosted',
  label: 'Self-Hosted',
  usable: true,
  reason: null,
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
    { name: 'gpt-6-sol', label: 'gpt-6-sol', efforts: [...CODEX_EFFORTS, 'max', 'ultra'] },
    { name: 'gpt-5.5', label: 'gpt-5.5', efforts: CODEX_EFFORTS },
  ],
  efforts: CODEX_EFFORTS,
  requires_model: false,
};

const openAi: ChatProviderOffer = {
  provider: 'openai',
  label: 'OpenAI',
  usable: true,
  reason: null,
  models: [],
  efforts: HTTP_EFFORTS,
  requires_model: true,
};

const offers = [selfHosted, openAi, claudeCode, codex];

const toolModel = { name: 'llama3.2', size: 1, modified_at: '', completion: true, tools: true };
const toollessModel = { name: 'gemma3', size: 1, modified_at: '', completion: true, tools: false };

const instanceModels: ChatProviderOffer = {
  ...selfHosted,
  models: [
    { name: 'llama3.2', label: 'llama3.2', efforts: HTTP_EFFORTS },
    { name: 'gemma3', label: 'gemma3', efforts: HTTP_EFFORTS },
  ],
};

const recentCodex: RecentChat = {
  model_name: 'auto',
  provider: 'codex',
  provider_model: 'gpt-6-sol',
  reasoning_effort: 'high',
  agent_enabled: true,
  offline: true,
  context_tokens: null,
};

const firstWorkspace = { id: 'ws-1', name: 'Test Workspace', organization_id: 'org-1' };
const secondWorkspace = { id: 'ws-2', name: 'Second Workspace', organization_id: 'org-1' };

let currentWorkspace = firstWorkspace;
let modelsError: string | null = null;
let installed = [toolModel];
let workspaceProviders: WorkspaceChatProviders | null = null;
let creation: Response;
let projects: Array<{ id: string; name: string }> = [];

Element.prototype.scrollIntoView = mock();

mock.module('../../models', () => ({
  useModels: () => ({
    models: installed,
    loading: false,
    error: modelsError,
    refresh: mock(),
    deleteModel: mock(),
  }),
}));

mock.module('../../sources/hooks/useSources', () => ({
  useSources: () => ({
    sources: [],
    loading: false,
    error: null,
    createSource: mock(),
    updateSource: mock(),
    deleteSource: mock(),
    verifySource: mock(),
    refresh: mock(),
  }),
}));

mock.module('../../projects/hooks', () => ({
  useProjects: () => ({
    projects,
    loading: false,
    error: null,
    createProject: mock(),
    updateProject: mock(),
    deleteProject: mock(),
    refetch: mock(),
  }),
}));

mock.module('../../auth/context', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com' },
    roles: ['user'],
    permissions: ['chats:read', 'chats:create'],
    hasPermission: () => true,
    hasAnyPermission: () => true,
    hasRole: () => true,
    logout: mock(),
    login: mock(),
    register: mock(),
    setAccessToken: mock(),
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    organizations: [{ id: 'org-1', name: 'Test Org' }],
    currentOrganization: { id: 'org-1', name: 'Test Org' },
    currentWorkspace,
    workspaces: [firstWorkspace, secondWorkspace],
    loading: false,
    error: null,
    setCurrentOrganization: mock(),
    setCurrentWorkspace: mock(),
    refreshOrganizations: mock(),
    refreshWorkspaces: mock(),
  }),
  WorkspaceProvider: ({ children }: { children: React.ReactNode }) => children,
}));

let ChatsPage: typeof import('./ChatsPage').default;
const fetch = globalThis.fetch;

beforeAll(async () => {
  ChatsPage = (await import('./ChatsPage')).default;
});

afterAll(() => {
  mock.restore();
});

const page = () => (
  <BrowserRouter>
    <ChatsPage />
  </BrowserRouter>
);

/** Clicks `+` and returns the body of the create it sent. */
const created = async (): Promise<Record<string, unknown>> => {
  fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);
  const fetchMock = globalThis.fetch as unknown as ReturnType<typeof mock>;
  await waitFor(() =>
    expect(
      fetchMock.mock.calls.some(([, init]) => (init as RequestInit | undefined)?.method === 'POST')
    ).toBe(true)
  );
  const [, init] = fetchMock.mock.calls.find(
    ([, init]) => (init as RequestInit | undefined)?.method === 'POST'
  ) as [string, RequestInit];
  return JSON.parse(String(init.body));
};

/** The chat `+` would create once the workspace's providers have been read. */
const createdOn = async (providers: WorkspaceChatProviders): Promise<Record<string, unknown>> => {
  workspaceProviders = providers;
  render(page());
  return created();
};

const providerReads = (): number => {
  const fetchMock = globalThis.fetch as unknown as ReturnType<typeof mock>;
  return fetchMock.mock.calls.filter(([url]) =>
    /\/api\/workspaces\/[^/]+\/chat-providers$/.test(String(url))
  ).length;
};

describe('ChatsPage new chat', () => {
  beforeEach(() => {
    currentWorkspace = firstWorkspace;
    modelsError = null;
    installed = [toolModel];
    workspaceProviders = null;
    projects = [];
    creation = Response.json({ success: false, error: REFUSAL }, { status: 409 });
    window.history.pushState({}, '', '/');
    globalThis.fetch = mock(async (url: string, init?: RequestInit) => {
      if (init?.method === 'POST') return creation;
      if (/\/api\/workspaces\/[^/]+\/chat-providers$/.test(url)) {
        return workspaceProviders
          ? Response.json(workspaceProviders)
          : Response.json({ success: false, error: 'Not found' }, { status: 404 });
      }
      return Response.json({ chats: [] });
    }) as unknown as typeof globalThis.fetch;
  });

  afterEach(() => {
    globalThis.fetch = fetch;
  });

  it("shows the server's reason when it refuses to create the chat", async () => {
    await createdOn({ default: selfHosted, providers: offers });

    expect(await screen.findByText(`Failed to create chat: ${REFUSAL}`)).toBeInTheDocument();
  });

  it("repeats the settings of the workspace's chat created last", async () => {
    const body = await createdOn({ default: selfHosted, providers: offers, recent: recentCodex });

    expect(body).toMatchObject({
      provider: 'codex',
      provider_model: 'gpt-6-sol',
      model_name: 'auto',
      title: 'Chat with gpt-6-sol',
      automatic_title: true,
      reasoning_effort: 'high',
      agent_enabled: true,
      auto_approve: false,
      offline: true,
    });
  });

  it('reads the settings again on every click rather than only on mount', async () => {
    workspaceProviders = { default: selfHosted, providers: offers, recent: recentCodex };
    render(page());
    expect(await created()).toMatchObject({ provider: 'codex' });
    const onMount = providerReads();

    workspaceProviders = {
      default: selfHosted,
      providers: offers,
      recent: { ...recentCodex, provider: 'claude_code', provider_model: 'opus' },
    };
    fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);

    await waitFor(() => expect(providerReads()).toBeGreaterThan(onMount));
    const fetchMock = globalThis.fetch as unknown as ReturnType<typeof mock>;
    await waitFor(() => {
      const posts = fetchMock.mock.calls.filter(
        ([, init]) => (init as RequestInit | undefined)?.method === 'POST'
      );
      expect(posts.length).toBe(2);
      expect(JSON.parse(String((posts[1][1] as RequestInit).body))).toMatchObject({
        provider: 'claude_code',
        provider_model: 'opus',
      });
    });
  });

  it('sends a Default chat on an agent default with its model and effort', async () => {
    const body = await createdOn({
      default: claudeCode,
      providers: offers,
      recent: {
        ...recentCodex,
        provider: null,
        provider_model: null,
        model_name: 'opus',
        reasoning_effort: 'max',
        offline: false,
      },
    });

    expect(body.model_name).toBe('opus');
    expect(body.reasoning_effort).toBe('max');
    expect(body).not.toHaveProperty('provider');
    expect(body).not.toHaveProperty('context_tokens');
    expect(body).not.toHaveProperty('offline');
  });

  it('sends no effort for a model of the default that takes none', async () => {
    const body = await createdOn({
      default: claudeCode,
      providers: offers,
      recent: {
        ...recentCodex,
        provider: null,
        provider_model: null,
        model_name: 'haiku',
        reasoning_effort: 'max',
      },
    });

    expect(body.model_name).toBe('haiku');
    expect(body.reasoning_effort).toBeUndefined();
  });

  it('sends the model a provider with none saved was last given', async () => {
    const body = await createdOn({
      default: selfHosted,
      providers: offers,
      recent: { ...recentCodex, provider: 'openai', provider_model: 'my-deployment' },
    });

    expect(body).toMatchObject({
      provider: 'openai',
      provider_model: 'my-deployment',
      model_name: 'auto',
      title: 'Chat with my-deployment',
    });
  });

  it('sends a chosen provider whose model was Automatic as no model', async () => {
    const body = await createdOn({
      default: selfHosted,
      providers: offers,
      recent: {
        ...recentCodex,
        provider: 'claude_code',
        provider_model: null,
        reasoning_effort: 'xhigh',
      },
    });

    expect(body).toMatchObject({
      provider: 'claude_code',
      provider_model: null,
      model_name: 'auto',
      reasoning_effort: 'xhigh',
    });
    expect(body).not.toHaveProperty('context_tokens');
  });

  it('falls back to Default when the chat created last ran on a provider no longer offered', async () => {
    const body = await createdOn({
      default: selfHosted,
      providers: offers,
      recent: { ...recentCodex, provider: 'anthropic', provider_model: 'claude-opus-4' },
    });

    expect(body.model_name).toBe('auto');
    expect(body).not.toHaveProperty('provider');
    expect(body.offline, 'the settings it can still honour are kept').toBe(true);
  });

  it('falls back to Automatic when the model of the chat created last is no longer installed', async () => {
    const body = await createdOn({
      default: selfHosted,
      providers: offers,
      recent: { ...recentCodex, provider: null, provider_model: null, model_name: 'qwen3:8b' },
    });

    expect(body.model_name).toBe('auto');
  });

  it('reads a padded model name as the trimmed one it sends', async () => {
    const body = await createdOn({
      default: selfHosted,
      providers: offers,
      recent: { ...recentCodex, provider: 'openai', provider_model: '  my-deployment  ' },
    });

    expect(body).toMatchObject({ provider: 'openai', provider_model: 'my-deployment' });
  });

  it('starts from the default settings when the providers cannot be read', async () => {
    render(page());

    expect(await screen.findByTestId('new-chat-providers-error')).toHaveTextContent(
      PROVIDERS_REFUSED
    );
    expect(await created()).toMatchObject({ model_name: 'auto', title: 'New chat' });
  });

  it('creates into the workspace that is open', async () => {
    workspaceProviders = { default: selfHosted, providers: offers };
    const view = render(page());
    expect(await created()).toMatchObject({ workspace_id: 'ws-1' });

    currentWorkspace = secondWorkspace;
    view.rerender(page());
    fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);

    const fetchMock = globalThis.fetch as unknown as ReturnType<typeof mock>;
    await waitFor(() => {
      const posts = fetchMock.mock.calls.filter(
        ([, init]) => (init as RequestInit | undefined)?.method === 'POST'
      );
      expect(posts.length).toBe(2);
      expect(JSON.parse(String((posts[1][1] as RequestInit).body))).toMatchObject({
        workspace_id: 'ws-2',
      });
    });
  });

  it('carries the context window of a local chat and none off a provider', async () => {
    installed = [{ ...toolModel, details: { context_length: 131072 } }];
    const local = await createdOn({
      default: selfHosted,
      providers: [instanceModels, openAi, claudeCode, codex],
      recent: {
        ...recentCodex,
        provider: null,
        provider_model: null,
        model_name: 'llama3.2',
        context_tokens: 8192,
      },
    });

    expect(local).toMatchObject({ model_name: 'llama3.2', context_tokens: 8192 });
  });

  it('sends no context window for a chat on an agent provider', async () => {
    const body = await createdOn({
      default: selfHosted,
      providers: offers,
      recent: { ...recentCodex, context_tokens: 8192 },
    });

    expect(body).not.toHaveProperty('context_tokens');
  });

  it('carries agent mode only while the model it would run can call tools', async () => {
    installed = [toolModel, toollessModel];
    const calling = await createdOn({
      default: selfHosted,
      providers: [instanceModels, openAi, claudeCode, codex],
      recent: {
        ...recentCodex,
        provider: null,
        provider_model: null,
        model_name: 'llama3.2',
        agent_enabled: true,
      },
    });
    expect(calling).toMatchObject({ model_name: 'llama3.2', agent_enabled: true });
  });

  it('drops agent mode for a model the instance lists as unable to call tools', async () => {
    installed = [toolModel, toollessModel];
    const body = await createdOn({
      default: selfHosted,
      providers: [instanceModels, openAi, claudeCode, codex],
      recent: {
        ...recentCodex,
        provider: null,
        provider_model: null,
        model_name: 'gemma3',
        agent_enabled: true,
      },
    });

    expect(body).toMatchObject({ model_name: 'gemma3', agent_enabled: false });
  });
});
