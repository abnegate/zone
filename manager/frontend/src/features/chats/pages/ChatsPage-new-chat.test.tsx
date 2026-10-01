import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';

const REFUSAL =
  "This workspace's AI endpoint can't be used: its AI settings couldn't be read. Check AI Settings.";

let modelsError: string | null = null;

Element.prototype.scrollIntoView = mock();

mock.module('../../models', () => ({
  useModels: () => ({
    models: [],
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
    currentWorkspace: { id: 'ws-1', name: 'Test Workspace', organization_id: 'org-1' },
    workspaces: [{ id: 'ws-1', name: 'Test Workspace', organization_id: 'org-1' }],
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

const openNewChat = async () => {
  render(
    <BrowserRouter>
      <ChatsPage />
    </BrowserRouter>
  );
  fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);
  await screen.findByRole('heading', { name: 'New Chat' });
};

describe('ChatsPage new chat dialog', () => {
  beforeEach(() => {
    modelsError = null;
    window.history.pushState({}, '', '/');
    globalThis.fetch = mock(async (_url: string, init?: RequestInit) =>
      init?.method === 'POST'
        ? Response.json({ success: false, error: REFUSAL }, { status: 409 })
        : Response.json({ chats: [] })
    ) as unknown as typeof globalThis.fetch;
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
});
