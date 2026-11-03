import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import { chooseSelect, openSelect } from '../../../test/select';
import type { ChatProviderOffer, ReasoningEffort, WorkspaceChatProviders } from '../types';

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

const firstWorkspace = { id: 'ws-1', name: 'Test Workspace', organization_id: 'org-1' };
const secondWorkspace = { id: 'ws-2', name: 'Second Workspace', organization_id: 'org-1' };

let currentWorkspace = firstWorkspace;
let modelsError: string | null = null;
let workspaceProviders: WorkspaceChatProviders | null = null;
let creation: Response;

Element.prototype.scrollIntoView = mock();

mock.module('../../models', () => ({
  useModels: () => ({
    models: [{ name: 'llama3.2', size: 1, modified_at: '', completion: true, tools: true }],
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
    projects: [],
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

const openDialog = async () => {
  fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);
  await screen.findByRole('heading', { name: 'New Chat' });
};

const closeDialog = () => {
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
};

const openNewChat = async () => {
  const view = render(page());
  await openDialog();
  return view;
};

const openNewChatWithProviders = async (fallback: ChatProviderOffer) => {
  workspaceProviders = { default: fallback, providers: offers };
  const view = await openNewChat();
  await screen.findByRole('combobox', { name: 'Provider' });
  return view;
};

const optionLabels = (label: string): string[] => {
  const trigger = openSelect(label);
  const labels = screen.getAllByRole('option').map((option) => option.textContent ?? '');
  fireEvent.keyDown(trigger, { key: 'Escape' });
  return labels;
};

const createdBody = async (): Promise<Record<string, unknown>> => {
  fireEvent.click(screen.getByRole('button', { name: 'Create Chat' }));
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

describe('ChatsPage new chat dialog', () => {
  beforeEach(() => {
    currentWorkspace = firstWorkspace;
    modelsError = null;
    workspaceProviders = null;
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
    await openNewChat();

    fireEvent.click(screen.getByRole('button', { name: 'Create Chat' }));

    expect(await screen.findByText(`Failed to create chat: ${REFUSAL}`)).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
  });

  it("shows why the workspace's models could not be listed in the model picker", async () => {
    modelsError = `Failed to fetch models: ${REFUSAL}`;

    await openNewChat();

    expect(screen.getByText(modelsError)).toBeInTheDocument();
  });

  it("lists the Claude Code default's own models and efforts instead of local ones", async () => {
    await openNewChatWithProviders(claudeCode);

    expect(optionLabels('Provider')).toEqual([
      'Default (Claude Code)',
      'Self-Hosted',
      'OpenAI',
      'Claude Code',
      'Codex',
    ]);
    expect(optionLabels('Select Model')).toEqual(['Automatic', 'Sonnet', 'Opus', 'Haiku']);
    expect(optionLabels('Reasoning')).toEqual([
      'Auto',
      'Low',
      'Medium',
      'High',
      'Extra high',
      'Max',
    ]);
    expect(screen.queryByRole('combobox', { name: 'Context' })).not.toBeInTheDocument();
    expect(screen.getByLabelText('Agent mode')).toBeInTheDocument();
  });

  it('sends a Default chat on an agent default with the agent model and effort', async () => {
    await openNewChatWithProviders(claudeCode);

    chooseSelect('Select Model', 'Opus');
    chooseSelect('Reasoning', 'Max');

    const body = await createdBody();
    expect(body.model_name).toBe('opus');
    expect(body.reasoning_effort).toBe('max');
    expect(body).not.toHaveProperty('provider');
    expect(body).not.toHaveProperty('context_tokens');
  });

  it('hides reasoning for a Claude model that takes no effort and sends none', async () => {
    await openNewChatWithProviders(claudeCode);

    chooseSelect('Select Model', 'Sonnet');
    chooseSelect('Reasoning', 'Max');
    chooseSelect('Select Model', 'Haiku');

    expect(screen.queryByRole('combobox', { name: 'Reasoning' })).not.toBeInTheDocument();
    const body = await createdBody();
    expect(body.model_name).toBe('haiku');
    expect(body.reasoning_effort).toBeUndefined();
  });

  it("refreshes the models and efforts with the chosen provider and restores Default's", async () => {
    await openNewChatWithProviders(selfHosted);

    expect(optionLabels('Select Model')).toEqual(['Automatic', 'llama3.2']);
    expect(screen.getByRole('combobox', { name: 'Provider' })).toHaveTextContent(
      'Default (Self-Hosted)'
    );

    chooseSelect('Provider', 'Codex');

    expect(optionLabels('Select Model')).toEqual(['Automatic', 'gpt-6-sol', 'gpt-5.5']);
    expect(optionLabels('Reasoning')).toEqual(['Auto', 'Low', 'Medium', 'High', 'Extra high']);

    chooseSelect('Select Model', 'gpt-6-sol');
    expect(optionLabels('Reasoning')).toContain('Ultra');
    chooseSelect('Reasoning', 'Ultra');

    chooseSelect('Select Model', 'gpt-5.5');
    expect(optionLabels('Reasoning')).not.toContain('Ultra');
    expect(screen.getByRole('combobox', { name: 'Reasoning' })).toHaveTextContent('Auto');

    chooseSelect('Provider', 'Default (Self-Hosted)');

    expect(optionLabels('Select Model')).toEqual(['Automatic', 'llama3.2']);
    expect(screen.queryByRole('combobox', { name: 'Reasoning' })).not.toBeInTheDocument();
  });

  it('asks for a typed model name on a provider with none saved and sends it', async () => {
    await openNewChatWithProviders(selfHosted);

    chooseSelect('Provider', 'OpenAI');

    expect(screen.queryByRole('combobox', { name: 'Select Model' })).not.toBeInTheDocument();
    const field = screen.getByRole('textbox', { name: 'Model name' });
    expect(field).toHaveValue('');
    expect(screen.getByRole('button', { name: 'Create Chat' })).toBeDisabled();
    fireEvent.change(field, { target: { value: '   ' } });
    expect(screen.getByRole('button', { name: 'Create Chat' })).toBeDisabled();
    fireEvent.change(field, { target: { value: ' my-deployment ' } });
    expect(screen.getByRole('button', { name: 'Create Chat' })).toBeEnabled();

    const body = await createdBody();
    expect(body).toMatchObject({
      provider: 'openai',
      provider_model: 'my-deployment',
      model_name: 'auto',
      title: 'Chat with my-deployment',
    });
  });

  it('sends the chosen provider with Automatic as no model', async () => {
    await openNewChatWithProviders(selfHosted);

    chooseSelect('Provider', 'Claude Code');
    chooseSelect('Reasoning', 'Extra high');

    const body = await createdBody();
    expect(body).toMatchObject({
      provider: 'claude_code',
      provider_model: null,
      model_name: 'auto',
      reasoning_effort: 'xhigh',
    });
    expect(body).not.toHaveProperty('context_tokens');
  });

  it("shows the server's refusal of the chosen provider in the dialog", async () => {
    creation = Response.json({ success: false, error: UNAVAILABLE }, { status: 409 });
    await openNewChatWithProviders(selfHosted);

    chooseSelect('Provider', 'Codex');
    fireEvent.click(screen.getByRole('button', { name: 'Create Chat' }));

    await waitFor(() =>
      expect(document.querySelector('.modal-error')).toHaveTextContent(UNAVAILABLE)
    );
    expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
  });

  it('says why the providers cannot be listed and keeps the local model picker', async () => {
    await openNewChat();

    expect(await screen.findByTestId('new-chat-providers-error')).toHaveTextContent(
      PROVIDERS_REFUSED
    );
    expect(screen.queryByRole('combobox', { name: 'Provider' })).not.toBeInTheDocument();
    expect(optionLabels('Select Model')).toEqual(['Automatic', 'llama3.2']);
  });

  it('lists the providers afresh each time the dialog opens', async () => {
    await openNewChatWithProviders(selfHosted);
    closeDialog();
    await waitFor(() =>
      expect(screen.queryByRole('heading', { name: 'New Chat' })).not.toBeInTheDocument()
    );
    workspaceProviders = { default: claudeCode, providers: offers };

    await openDialog();

    await waitFor(() =>
      expect(screen.getByRole('combobox', { name: 'Provider' })).toHaveTextContent(
        'Default (Claude Code)'
      )
    );
    expect(optionLabels('Select Model')).toEqual(['Automatic', 'Sonnet', 'Opus', 'Haiku']);
  });

  it('drops the providers error once they list on reopening the dialog', async () => {
    await openNewChat();
    await screen.findByTestId('new-chat-providers-error');
    closeDialog();
    await waitFor(() =>
      expect(screen.queryByRole('heading', { name: 'New Chat' })).not.toBeInTheDocument()
    );
    workspaceProviders = { default: claudeCode, providers: offers };

    await openDialog();

    expect(await screen.findByRole('combobox', { name: 'Provider' })).toHaveTextContent(
      'Default (Claude Code)'
    );
    expect(screen.queryByTestId('new-chat-providers-error')).not.toBeInTheDocument();
    expect(optionLabels('Select Model')).toEqual(['Automatic', 'Sonnet', 'Opus', 'Haiku']);
  });

  it('starts from Default and the local models every time the dialog opens', async () => {
    await openNewChatWithProviders(selfHosted);
    chooseSelect('Provider', 'Codex');
    chooseSelect('Select Model', 'gpt-6-sol');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() =>
      expect(screen.queryByRole('heading', { name: 'New Chat' })).not.toBeInTheDocument()
    );

    await openDialog();

    expect(screen.getByRole('combobox', { name: 'Provider' })).toHaveTextContent(
      'Default (Self-Hosted)'
    );
    expect(screen.getByRole('combobox', { name: 'Select Model' })).toHaveTextContent('Automatic');
    expect(optionLabels('Select Model')).toEqual(['Automatic', 'llama3.2']);
  });

  it('starts over when the workspace changes', async () => {
    const view = await openNewChatWithProviders(selfHosted);
    chooseSelect('Provider', 'Codex');
    chooseSelect('Select Model', 'gpt-6-sol');

    currentWorkspace = secondWorkspace;
    view.rerender(page());

    await waitFor(() =>
      expect(screen.getByRole('combobox', { name: 'Provider' })).toHaveTextContent(
        'Default (Self-Hosted)'
      )
    );
    expect(screen.getByRole('combobox', { name: 'Select Model' })).toHaveTextContent('Automatic');
  });

  it('offers agent mode for an agent provider and by model capability otherwise', async () => {
    await openNewChatWithProviders(selfHosted);

    chooseSelect('Provider', 'OpenAI');
    fireEvent.change(screen.getByRole('textbox', { name: 'Model name' }), {
      target: { value: 'gpt-4o' },
    });
    expect(screen.queryByLabelText('Agent mode')).not.toBeInTheDocument();

    chooseSelect('Provider', 'Codex');
    expect(screen.getByLabelText('Agent mode')).toBeInTheDocument();

    chooseSelect('Provider', 'Default (Self-Hosted)');
    expect(screen.getByLabelText('Agent mode')).toBeInTheDocument();
  });

  it("does not offer agent mode on a chosen endpoint provider from the instance's models", async () => {
    await openNewChatWithProviders(selfHosted);

    chooseSelect('Provider', 'Self-Hosted');
    expect(screen.getByRole('combobox', { name: 'Select Model' })).toHaveTextContent('Automatic');
    expect(screen.queryByLabelText('Agent mode')).not.toBeInTheDocument();
  });

  it('words the agent model hint so it reads right for any model', async () => {
    await openNewChatWithProviders(selfHosted);

    chooseSelect('Provider', 'Codex');
    chooseSelect('Select Model', 'gpt-6-sol');

    expect(
      screen.getByText('Choose Automatic to let Codex pick its own model.')
    ).toBeInTheDocument();
    expect(
      screen.queryByText('Automatic lets Codex choose its own model.')
    ).not.toBeInTheDocument();
  });
});
