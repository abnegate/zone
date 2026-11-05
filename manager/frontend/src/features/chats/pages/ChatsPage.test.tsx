import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  mock,
  spyOn,
} from 'bun:test';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import { chooseMenuItem } from '../../../test/menu';
import { chooseSelect } from '../../../test/select';
import type {
  Chat,
  ChatProviderOffer,
  ChatProviderSelection,
  ChatProviders,
  ChatSearchResult,
  ChatWithMessages,
  Message,
  ReasoningEffort,
  UpdateChatRequest,
} from '../types';
import { CHAT_GROUP_STORAGE_KEY, CHAT_SORT_STORAGE_KEY } from '../utils/arrange';

// Create mock functions
const mockGetChats = mock();
const mockGetChat = mock();
const mockCreateChat = mock();
const mockSendMessage = mock();
const mockArchiveChat = mock();
const mockUnarchiveChat = mock();
const mockDeleteChat = mock();
const mockSearchChatMessages = mock();
const mockUpdateChat = mock();
const mockGetChatSources = mock();
const mockSetChatSources = mock();
const mockGetChatProviders = mock();
const mockSetChatProvider = mock();
const mockGetWorkspaceChatProviders = mock();
const mockWsSend = mock();
const mockWsClose = mock();

class FakeSocket {
  readyState = 1;
  send = mockWsSend;
  close = mockWsClose;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  addEventListener(): void {}
  emit(payload: unknown): void {
    this.onmessage?.({ data: JSON.stringify(payload) });
  }
}

let socket: FakeSocket;

// Mock scrollIntoView
Element.prototype.scrollIntoView = mock();

// Mock the chatsApi
mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChats: mockGetChats,
    getChat: mockGetChat,
    createChat: mockCreateChat,
    sendMessage: mockSendMessage,
    archiveChat: mockArchiveChat,
    unarchiveChat: mockUnarchiveChat,
    deleteChat: mockDeleteChat,
    searchChatMessages: mockSearchChatMessages,
    getChatSources: mockGetChatSources,
    setChatSources: mockSetChatSources,
    getChatProviders: mockGetChatProviders,
    setChatProvider: mockSetChatProvider,
    getWorkspaceChatProviders: mockGetWorkspaceChatProviders,
    setGetAccessToken: mock(),
    getMessages: mock(),
    updateChat: mockUpdateChat,
    deleteMessage: mock(),
    chatAccessToken: () => 'test-token',
    ensureAccessToken: async () => 'test-token',
    createChatWebSocket: () => {
      socket = new FakeSocket();
      return socket;
    },
  },
}));

const workspaceSources = [
  {
    id: 'source-repo',
    name: 'abnegate/zone-tests',
    source_type: 'github',
    category: 'file',
    config: {},
    description: null,
    url: 'https://github.com/abnegate/zone-tests',
    is_active: true,
    last_verified_at: null,
    last_error: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
  },
  {
    id: 'source-notes',
    name: 'Team notes',
    source_type: 'text',
    category: 'text',
    config: {},
    description: null,
    url: '',
    is_active: true,
    last_verified_at: null,
    last_error: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
  },
];

mock.module('../../sources/hooks/useSources', () => ({
  useSources: () => ({
    sources: workspaceSources,
    loading: false,
    error: null,
    createSource: mock(),
    updateSource: mock(),
    deleteSource: mock(),
    verifySource: mock(),
    refresh: mock(),
  }),
}));

type ListedModel = { name: string; size: number; modified_at: string } & Record<string, unknown>;

const instanceModels: ListedModel[] = [
  { name: 'llama2', size: 1, modified_at: '', tools: false },
  { name: 'mistral', size: 1, modified_at: '', completion: true },
  { name: 'llama3.1', size: 1, modified_at: '', completion: true, tools: true },
  { name: 'vectors', size: 1, modified_at: '', completion: false },
];
let listedModels = instanceModels;
const modelWorkspaces: (string | undefined)[] = [];

const mockProjects = [
  {
    id: 'proj-alpha',
    name: 'Alpha',
    description: null,
    status: 'active',
    github_repo_url: null,
    source_id: null,
    auto: false,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
  },
  {
    id: 'proj-zeta',
    name: 'Zeta',
    description: null,
    status: 'active',
    github_repo_url: null,
    source_id: null,
    auto: false,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
  },
];

mock.module('../../projects/hooks', () => ({
  useProjects: () => ({
    projects: mockProjects,
    loading: false,
    error: null,
    createProject: mock(),
    updateProject: mock(),
    deleteProject: mock(),
    refetch: mock(),
  }),
}));

// Mock useModels - include all exports from models module for proper mocking
mock.module('../../models', () => ({
  useModels: (workspaceId?: string) => {
    modelWorkspaces.push(workspaceId);
    return {
      models: listedModels,
      loading: false,
      error: null,
      refresh: mock(),
      deleteModel: mock(),
    };
  },
  useBrowse: () => ({
    browse: mock(),
    models: [],
    loading: false,
    error: null,
    hasMore: false,
    loadMore: mock(),
  }),
  usePull: () => ({
    pull: mock(),
    progress: null,
    pulling: false,
    error: null,
  }),
  VirtualBrowseList: () => null,
  ModelsPage: () => null,
  formatNumber: (n: number) => String(n),
  // Schemas
  InstalledModelSchema: {},
  BrowseModelSchema: {},
  ModelSourceSchema: {},
  ModelsResponseSchema: {},
  BrowseResponseSchema: {},
  PullProgressSchema: {},
}));

// Mock useAuth to always return authenticated
mock.module('../../auth/context', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com' },
    roles: ['user'],
    permissions: ['chats:read', 'chats:create', 'chats:delete'],
    hasPermission: (p: string) => ['chats:read', 'chats:create', 'chats:delete'].includes(p),
    hasAnyPermission: () => true,
    hasRole: () => true,
    logout: mock(),
    login: mock(),
    register: mock(),
    setAccessToken: mock(),
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

// Mock useWorkspace
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

// Create mockClient object for backward compatibility in tests
const mockClient = {
  getChats: mockGetChats,
  getChat: mockGetChat,
  createChat: mockCreateChat,
  sendMessage: mockSendMessage,
  archiveChat: mockArchiveChat,
  unarchiveChat: mockUnarchiveChat,
  deleteChat: mockDeleteChat,
  searchChatMessages: mockSearchChatMessages,
  updateChat: mockUpdateChat,
};

let ChatsPage: typeof import('./ChatsPage').default;

// Helper to create date strings relative to now for testing formatDate
const getDateString = (daysAgo: number, hours = 0): string => {
  const date = new Date();
  date.setDate(date.getDate() - daysAgo);
  date.setHours(date.getHours() - hours);
  return date.toISOString();
};

const mockChats: Chat[] = [
  {
    id: 'chat-1',
    title: 'Chat 1',
    model_name: 'llama2',
    updated_at: getDateString(0),
    archived: false,
    agent_enabled: false,
    created_at: '2024-01-01T00:00:00Z',
  },
  {
    id: 'chat-2',
    title: 'Chat 2',
    model_name: 'mistral',
    updated_at: getDateString(1),
    archived: false,
    agent_enabled: false,
    created_at: '2024-01-02T00:00:00Z',
  },
  {
    id: 'chat-3',
    title: 'Chat 3',
    model_name: 'llama2',
    updated_at: getDateString(3),
    archived: false,
    agent_enabled: false,
    created_at: '2024-01-03T00:00:00Z',
  },
  {
    id: 'chat-4',
    title: 'Chat 4',
    model_name: 'mistral',
    updated_at: getDateString(10),
    archived: false,
    agent_enabled: false,
    created_at: '2024-01-04T00:00:00Z',
  },
];

const mockChatWithMessages: ChatWithMessages = {
  id: 'chat-1',
  title: 'Chat 1',
  model_name: 'llama2',
  updated_at: '2024-01-01T00:00:00Z',
  archived: false,
  agent_enabled: false,
  created_at: '2024-01-01T00:00:00Z',
  messages: [
    {
      id: 'msg-1',
      chat_id: 'chat-1',
      role: 'user',
      content: 'Hello',
      created_at: '2024-01-01T00:00:00Z',
    },
    {
      id: 'msg-2',
      chat_id: 'chat-1',
      role: 'assistant',
      content: 'Hi there!',
      created_at: '2024-01-01T00:01:00Z',
    },
  ],
};

const mockChatEmpty: ChatWithMessages = {
  id: 'chat-2',
  title: 'Empty Chat',
  model_name: 'mistral',
  updated_at: '2024-01-02T00:00:00Z',
  archived: false,
  agent_enabled: false,
  created_at: '2024-01-02T00:00:00Z',
  messages: [],
};

const mockChatWithSystemMessage: ChatWithMessages = {
  id: 'chat-3',
  title: 'System Chat',
  model_name: 'llama2',
  updated_at: '2024-01-03T00:00:00Z',
  archived: false,
  agent_enabled: false,
  created_at: '2024-01-03T00:00:00Z',
  messages: [
    {
      id: 'msg-3',
      chat_id: 'chat-3',
      role: 'system',
      content: 'System message',
      created_at: '2024-01-03T00:00:00Z',
    },
  ],
};

const HTTP_EFFORTS: ReasoningEffort[] = ['off', 'low', 'medium', 'high'];
const CLAUDE_EFFORTS: ReasoningEffort[] = ['low', 'medium', 'high', 'xhigh', 'max'];

const providerOffer = (overrides: Partial<ChatProviderOffer> = {}): ChatProviderOffer => ({
  provider: 'self_hosted',
  label: 'Self-Hosted',
  usable: true,
  reason: null,
  models: [],
  efforts: HTTP_EFFORTS,
  requires_model: false,
  ...overrides,
});

const selfHostedOffer = providerOffer();

const openAiOffer = providerOffer({
  provider: 'openai',
  label: 'OpenAI',
  models: [{ name: 'gpt-4o', label: 'gpt-4o', efforts: HTTP_EFFORTS }],
});

const claudeOffer = providerOffer({
  provider: 'claude_code',
  label: 'Claude Code',
  models: [
    { name: 'sonnet', label: 'Sonnet', efforts: CLAUDE_EFFORTS },
    { name: 'haiku', label: 'Haiku', efforts: [] },
  ],
  efforts: CLAUDE_EFFORTS,
});

const codexOffer = providerOffer({
  provider: 'codex',
  label: 'Codex',
  models: [
    { name: 'gpt-6-sol', label: 'gpt-6-sol', efforts: [...CLAUDE_EFFORTS, 'ultra'] },
    { name: 'gpt-5.5', label: 'gpt-5.5', efforts: ['low', 'medium', 'high', 'xhigh'] },
  ],
  efforts: ['low', 'medium', 'high', 'xhigh'],
});

const chatProviders = (overrides: Partial<ChatProviders> = {}): ChatProviders => ({
  default: selfHostedOffer,
  selected: null,
  providers: [selfHostedOffer, openAiOffer, claudeOffer, codexOffer],
  ...overrides,
});

const selection = (
  offer: ChatProviderOffer,
  model: string | null,
  overrides: Partial<ChatProviderSelection> = {}
): ChatProviderSelection => ({ ...offer, model, ...overrides });

const newChatButtons = () => screen.getAllByRole('button', { name: 'New chat' });

function chooseSelectOption(testId: string, optionName: string) {
  const trigger = screen.getByTestId(testId);
  expect(trigger.tagName).toBe('BUTTON');
  fireEvent.keyDown(trigger, { key: 'ArrowDown' });
  fireEvent.click(screen.getByRole('option', { name: optionName }));
}

const selectedChatParam = (): string | null =>
  new URLSearchParams(window.location.search).get('id');

const renderChatsPage = () => {
  return render(
    <BrowserRouter>
      <ChatsPage />
    </BrowserRouter>
  );
};

beforeAll(async () => {
  ChatsPage = (await import('./ChatsPage')).default;
});

afterAll(() => {
  mock.restore();
});

describe('ChatsPage', () => {
  beforeEach(() => {
    localStorage.removeItem(CHAT_GROUP_STORAGE_KEY);
    localStorage.removeItem(CHAT_SORT_STORAGE_KEY);
    mockGetChatSources.mockReset();
    mockSetChatSources.mockReset();
    mockGetChatSources.mockResolvedValue([]);
    mockGetChatProviders.mockReset();
    mockSetChatProvider.mockReset();
    mockGetChatProviders.mockResolvedValue(chatProviders());
    mockGetWorkspaceChatProviders.mockReset();
    mockGetWorkspaceChatProviders.mockResolvedValue({
      default: selfHostedOffer,
      providers: [selfHostedOffer],
    });
    window.history.pushState({}, '', '/');
    mockGetChats.mockReset();
    mockGetChat.mockReset();
    mockCreateChat.mockReset();
    mockSendMessage.mockReset();
    mockArchiveChat.mockReset();
    mockUnarchiveChat.mockReset();
    mockDeleteChat.mockReset();
    mockSearchChatMessages.mockReset();
    mockUpdateChat.mockReset();
    mockWsSend.mockReset();
    mockWsClose.mockReset();
    mockClient.getChats.mockResolvedValue(mockChats);
    mockClient.getChat.mockResolvedValue(mockChatWithMessages);
    listedModels = instanceModels;
    modelWorkspaces.length = 0;
  });

  describe('chat names', () => {
    it('renames the active chat without replacing its streaming messages', async () => {
      mockUpdateChat.mockResolvedValue({ ...mockChats[0], title: 'New name' });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByText('Hi there!');
      act(() => {
        socket.emit({ type: 'message_start', message_id: 'stream', role: 'assistant' });
        socket.emit({ type: 'chunk', content: 'Still writing', index: 0 });
      });
      await chooseMenuItem('Actions for Chat 1', 'Rename');
      expect(screen.getByLabelText('Chat name')).toHaveValue('Chat 1');
      fireEvent.change(screen.getByLabelText('Chat name'), { target: { value: '  New name  ' } });
      fireEvent.click(screen.getByRole('button', { name: 'Save name' }));
      await screen.findByRole('heading', { name: 'New name' });
      expect(mockUpdateChat).toHaveBeenCalledWith('chat-1', { title: 'New name' });
      expect(screen.getAllByText('New name')).toHaveLength(2);
      await waitFor(() => expect(screen.getByText('Still writing')).toBeInTheDocument());
    });

    it('keeps failed input, rejects blank names, and cancels without an update', async () => {
      mockUpdateChat.mockRejectedValue(new Error('Rename unavailable'));
      renderChatsPage();
      await screen.findByRole('button', { name: 'Actions for Chat 1' });
      await chooseMenuItem('Actions for Chat 1', 'Rename');
      fireEvent.change(screen.getByLabelText('Chat name'), { target: { value: '   ' } });
      expect(screen.getByRole('button', { name: 'Save name' })).toBeDisabled();
      fireEvent.change(screen.getByLabelText('Chat name'), { target: { value: 'Try again' } });
      fireEvent.click(screen.getByRole('button', { name: 'Save name' }));
      await screen.findByText('Rename unavailable');
      expect(screen.getByLabelText('Chat name')).toHaveValue('Try again');
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
      expect(mockUpdateChat).toHaveBeenCalledTimes(1);
    });

    it('renames nonactive chats without changing selection and persists unchanged names', async () => {
      mockUpdateChat.mockResolvedValue(mockChats[1]);
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByText('Hi there!');
      await chooseMenuItem('Actions for Chat 2', 'Rename');
      fireEvent.click(screen.getByRole('button', { name: 'Save name' }));
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(mockUpdateChat).toHaveBeenCalledWith('chat-2', { title: 'Chat 2' });
      expect(screen.getByRole('heading', { name: mockChatWithMessages.title })).toBeInTheDocument();
    });

    it('opens rename from the row menu with the name field focused, leaving the chat unselected', async () => {
      renderChatsPage();
      await screen.findByRole('button', { name: 'Actions for Chat 1' });
      await chooseMenuItem('Actions for Chat 1', 'Rename');
      const input = await screen.findByLabelText('Chat name');
      await waitFor(() => expect(document.activeElement).toBe(input));
      expect(input).toHaveValue('Chat 1');
      expect(mockGetChat).not.toHaveBeenCalled();
    });

    it('archives from the row menu without opening the chat', async () => {
      mockClient.archiveChat.mockResolvedValueOnce({ ...mockChats[1]!, archived: true });
      renderChatsPage();
      await screen.findByRole('button', { name: 'Actions for Chat 2' });
      await chooseMenuItem('Actions for Chat 2', 'Archive');
      await waitFor(() => expect(mockClient.archiveChat).toHaveBeenCalledWith('chat-2'));
      expect(mockGetChat).not.toHaveBeenCalled();
      expect(document.querySelector('.chat-item.active')).toBeNull();
    });

    it('updates both titles from a socket event while preserving the response', async () => {
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByText('Hi there!');
      act(() => {
        socket.emit({ type: 'message_start', message_id: 'stream', role: 'assistant' });
        socket.emit({ type: 'chunk', content: 'First chunk', index: 0 });
        socket.emit({ type: 'title_updated', chat_id: 'chat-1', title: 'Summary title' });
        socket.emit({ type: 'chunk', content: ' continues', index: 1 });
      });
      expect(screen.getAllByText('Summary title')).toHaveLength(2);
      await waitFor(() => {
        expect(screen.getByText('First chunk continues')).toBeInTheDocument();
      });
      act(() => socket.emit({ type: 'title_updated', chat_id: 'chat-2', title: 'Wrong chat' }));
      expect(screen.queryByText('Wrong chat')).not.toBeInTheDocument();
    });
  });

  it('prevents duplicate submissions and does not apply a late rename to another selection', async () => {
    let resolve: (chat: Chat) => void = () => {};
    mockUpdateChat.mockImplementation(
      () =>
        new Promise<Chat>((done) => {
          resolve = done;
        })
    );
    renderChatsPage();
    fireEvent.click(await screen.findByText('Chat 1'));
    await screen.findByText('Hi there!');
    await chooseMenuItem('Actions for Chat 1', 'Rename');
    const input = screen.getByLabelText('Chat name');
    fireEvent.change(input, { target: { value: 'Renamed first' } });
    const form = input.closest('form')!;
    fireEvent.submit(form);
    fireEvent.submit(form);
    expect(mockUpdateChat).toHaveBeenCalledTimes(1);
    mockGetChat.mockResolvedValue(mockChatEmpty);
    fireEvent.click(screen.getByText('Chat 2'));
    await screen.findByRole('heading', { name: 'Empty Chat', hidden: true });
    await act(async () => {
      resolve({ ...mockChats[0]!, title: 'Renamed first' });
    });
    expect(screen.getByRole('heading', { name: 'Empty Chat' })).toBeInTheDocument();
    expect(screen.getByText('Renamed first')).toBeInTheDocument();
  });

  it('ignores delayed automatic names after manually renaming active or nonactive chats', async () => {
    mockUpdateChat.mockImplementation(async (id: string, request: { title: string }) => ({
      ...mockChats.find((chat) => chat.id === id),
      title: request.title,
    }));
    renderChatsPage();
    fireEvent.click(await screen.findByText('Chat 1'));
    await screen.findByText('Hi there!');
    await chooseMenuItem('Actions for Chat 1', 'Rename');
    fireEvent.change(screen.getByLabelText('Chat name'), { target: { value: 'My first name' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save name' }));
    await screen.findByRole('heading', { name: 'My first name' });
    act(() => socket.emit({ type: 'title_updated', chat_id: 'chat-1', title: 'Late summary' }));
    expect(screen.getAllByText('My first name')).toHaveLength(2);
    expect(screen.queryByText('Late summary')).not.toBeInTheDocument();

    await chooseMenuItem('Actions for Chat 2', 'Rename');
    fireEvent.change(screen.getByLabelText('Chat name'), { target: { value: 'My second name' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save name' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    mockGetChat.mockResolvedValue({ ...mockChatEmpty, title: 'My second name' });
    fireEvent.click(screen.getByText('My second name'));
    await screen.findByRole('heading', { name: 'My second name' });
    act(() =>
      socket.emit({ type: 'title_updated', chat_id: 'chat-2', title: 'Another late summary' })
    );
    expect(screen.getAllByText('My second name')).toHaveLength(2);
    expect(screen.queryByText('Another late summary')).not.toBeInTheDocument();
  });

  it('reconciles the sidebar when returning to a chat named while it was unselected', async () => {
    renderChatsPage();
    fireEvent.click(await screen.findByText('Chat 1'));
    await screen.findByText('Hi there!');
    mockGetChat.mockResolvedValue(mockChatEmpty);
    fireEvent.click(screen.getByText('Chat 2'));
    await screen.findByRole('heading', { name: 'Empty Chat' });
    mockGetChat.mockResolvedValue({ ...mockChatWithMessages, title: 'Summary while away' });
    fireEvent.click(screen.getByText(mockChatWithMessages.title));
    await screen.findByRole('heading', { name: 'Summary while away' });
    expect(screen.getAllByText('Summary while away')).toHaveLength(2);
  });

  describe('rendering', () => {
    it('renders sidebar with chats heading', async () => {
      renderChatsPage();
      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'Chats' })).toBeInTheDocument();
      });
    });

    it('renders new chat button', async () => {
      renderChatsPage();
      await waitFor(() => {
        expect(newChatButtons()[0]).toBeInTheDocument();
      });
    });

    it('keeps the new chat control an icon-only button drawn with the shared plus icon', async () => {
      renderChatsPage();
      await waitFor(() => {
        expect(newChatButtons()[0]).toHaveClass('btn-icon');
      });
      expect(newChatButtons()[0].querySelector('svg.plus-icon')).not.toBeNull();
    });

    it('renders filter buttons', async () => {
      renderChatsPage();
      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Active' })).toBeInTheDocument();
        expect(screen.getByRole('tab', { name: 'Archived' })).toBeInTheDocument();
      });
    });
  });

  describe('loading state', () => {
    it('shows loading spinner while fetching', async () => {
      mockClient.getChats.mockImplementation(() => new Promise(() => {}));
      renderChatsPage();
      expect(screen.getByText('Loading...')).toBeInTheDocument();
    });
  });

  describe('error state', () => {
    it('shows error when loading fails', async () => {
      mockClient.getChats.mockRejectedValueOnce(new Error('Network error'));
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Network error')).toBeInTheDocument();
      });
    });
  });

  describe('chat list', () => {
    it('fetches chats on mount', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(mockClient.getChats).toHaveBeenCalled();
      });
    });

    it('displays chat list when loaded', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
        expect(screen.getByText('Chat 2')).toBeInTheDocument();
      });
    });

    it('shows empty state when no chats', async () => {
      mockClient.getChats.mockResolvedValueOnce([]);
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('No chats yet')).toBeInTheDocument();
      });
    });
  });

  describe('selecting chat', () => {
    it('loads chat when clicked', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(mockClient.getChat).toHaveBeenCalledWith('chat-1');
      });
    });

    it('displays chat messages when selected', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByText('Hello')).toBeInTheDocument();
        expect(screen.getByText('Hi there!')).toBeInTheDocument();
      });
      expect(screen.queryByText('You')).not.toBeInTheDocument();
      expect(screen.queryByText('Assistant')).not.toBeInTheDocument();
    });

    it('shows a reasoning disclosure when the assistant stored thinking', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        messages: [
          mockChatWithMessages.messages[0],
          {
            ...mockChatWithMessages.messages[1],
            metadata: { reasoning: 'The capital is Paris.' },
          },
        ],
      });
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByText('Reasoning')).toBeInTheDocument();
        expect(screen.getByText('The capital is Paris.')).toBeInTheDocument();
        expect(screen.getByText('Hi there!')).toBeInTheDocument();
      });
    });

    it('hides the waiting placeholder body of a turn that asked a question', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        messages: [
          mockChatWithMessages.messages[0],
          {
            ...mockChatWithMessages.messages[1],
            content: '[Waiting for your answer]',
            metadata: {
              tool_calls: [
                {
                  id: 'call_q',
                  name: 'ask_user',
                  arguments: '{}',
                  success: true,
                  detail: 'Waiting for your answer…',
                  duration_ms: 0,
                  questions: [
                    {
                      header: 'Which city?',
                      question: 'Which city do you mean?',
                      required: false,
                      multi_select: false,
                      choices: [{ label: 'Auckland', description: 'North Island' }],
                    },
                  ],
                },
              ],
            },
          },
        ],
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));

      await screen.findByTestId('question-card');
      expect(screen.queryByText('[Waiting for your answer]')).not.toBeInTheDocument();
    });

    const askedQuestion = {
      ...mockChatWithMessages.messages[1],
      content: '[Waiting for your answer]',
      metadata: {
        tool_calls: [
          {
            id: 'call_q',
            name: 'ask_user',
            arguments: '{}',
            success: true,
            detail: 'Waiting for your answer…',
            duration_ms: 0,
            questions: [
              {
                header: 'Which city?',
                question: 'Which city do you mean?',
                required: false,
                multi_select: false,
                choices: [{ label: 'Auckland', description: 'North Island' }],
              },
            ],
          },
        ],
      },
    };

    it('leaves a question open when only a reminder turn follows it', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        messages: [
          mockChatWithMessages.messages[0],
          askedQuestion,
          {
            id: 'msg-reminder',
            chat_id: 'chat-1',
            role: 'user',
            content: 'Check the deploy queue',
            created_at: '2024-01-01T00:02:00Z',
            metadata: { source: 'reminder' },
          },
        ],
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));

      const card = await screen.findByTestId('question-card');
      expect(card).not.toHaveClass('question-card--answered');
      const choice = within(card).getByRole('radio', { name: /Auckland/ });
      expect(choice).toBeEnabled();
      fireEvent.click(choice);
      const submit = within(card).getByRole('button', { name: 'Send answer' });
      await waitFor(() => expect(submit).toBeEnabled());
      fireEvent.click(submit);

      await waitFor(() => {
        const sent = mockWsSend.mock.calls.map(([frame]) => JSON.parse(String(frame)));
        expect(sent.some((frame) => frame.type === 'send' && /Auckland/.test(frame.content))).toBe(
          true
        );
      });
    });

    it('settles a question once the reader types a reply after a reminder turn', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        messages: [
          mockChatWithMessages.messages[0],
          askedQuestion,
          {
            id: 'msg-reminder',
            chat_id: 'chat-1',
            role: 'user',
            content: 'Check the deploy queue',
            created_at: '2024-01-01T00:02:00Z',
            metadata: { source: 'reminder' },
          },
          {
            id: 'msg-typed',
            chat_id: 'chat-1',
            role: 'user',
            content: 'Auckland',
            created_at: '2024-01-01T00:03:00Z',
          },
        ],
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));

      const card = await screen.findByTestId('question-card');
      expect(card).toHaveClass('question-card--answered');
      expect(within(card).getByRole('radio', { name: /Auckland/ })).toBeDisabled();
    });

    it('shows thinking interleaved with the tool trace', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        messages: [
          mockChatWithMessages.messages[0],
          {
            ...mockChatWithMessages.messages[1],
            content: 'We deploy on Fridays.',
            metadata: {
              tool_calls: [
                {
                  id: 'call_1',
                  name: 'search_knowledge',
                  arguments: '{"query":"deploys"}',
                  success: true,
                  detail: '3 passages',
                  duration_ms: 128,
                  reasoning: 'Search the workspace first.',
                },
                {
                  id: 'call_2',
                  name: 'read_document',
                  arguments: '{"id":"doc-1"}',
                  success: true,
                  detail: '1 document',
                  duration_ms: 40,
                  reasoning: 'That hit looks right; read it.',
                },
              ],
              reasoning: 'Fridays are the deploy window.',
            },
          },
        ],
      });
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByText('We deploy on Fridays.')).toBeInTheDocument();
      });

      const firstThought = screen.getByText('Search the workspace first.');
      const firstTool = screen.getByText('Searched the knowledge base');
      const secondThought = screen.getByText('That hit looks right; read it.');
      const secondTool = screen.getByText('Read a workspace document');
      const leftover = screen.getByText('Fridays are the deploy window.');
      const answer = screen.getByText('We deploy on Fridays.');

      expect(
        firstThought.compareDocumentPosition(firstTool) & Node.DOCUMENT_POSITION_FOLLOWING
      ).toBeTruthy();
      expect(
        firstTool.compareDocumentPosition(secondThought) & Node.DOCUMENT_POSITION_FOLLOWING
      ).toBeTruthy();
      expect(
        secondThought.compareDocumentPosition(secondTool) & Node.DOCUMENT_POSITION_FOLLOWING
      ).toBeTruthy();
      expect(
        secondTool.compareDocumentPosition(leftover) & Node.DOCUMENT_POSITION_FOLLOWING
      ).toBeTruthy();
      expect(
        leftover.compareDocumentPosition(answer) & Node.DOCUMENT_POSITION_FOLLOWING
      ).toBeTruthy();

      const blocks = screen.getAllByTestId('reasoning');
      expect(blocks).toHaveLength(3);
      expect(document.querySelectorAll('.message-activity-toggle')).toHaveLength(1);
      for (const block of blocks) {
        expect(block).toHaveAttribute('hidden');
      }

      fireEvent.click(screen.getByRole('button', { name: 'Reasoning' }));

      for (const block of blocks) {
        expect(block).not.toHaveAttribute('hidden');
      }
    });

    it('shows error when chat loading fails', async () => {
      mockClient.getChat.mockRejectedValueOnce(new Error('Chat not found'));
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByText('Chat not found')).toBeInTheDocument();
      });
      expect(screen.getByText('Chat 1')).toBeInTheDocument();
      expect(screen.getByText('Chat 2')).toBeInTheDocument();
    });

    it('keeps the chat list visible while a conversation loads', async () => {
      mockClient.getChat.mockImplementation(() => new Promise(() => {}));
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      expect(screen.getByText('Chat 1')).toBeInTheDocument();
      expect(screen.getByText('Chat 2')).toBeInTheDocument();
      expect(screen.getByText('Chat 3')).toBeInTheDocument();
    });
  });

  describe('archive filter', () => {
    it('switches to archived filter', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Archived' })).toBeInTheDocument();
      });

      const archivedTab = screen.getByRole('tab', { name: 'Archived' });
      fireEvent.mouseDown(archivedTab);
      fireEvent.mouseUp(archivedTab);
      fireEvent.click(archivedTab);

      await waitFor(() => {
        expect(mockClient.getChats).toHaveBeenCalledWith('ws-1', true);
      });
    });

    it('switches back to active filter', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Archived' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('tab', { name: 'Archived' }));
      fireEvent.click(screen.getByRole('tab', { name: 'Active' }));

      await waitFor(() => {
        expect(mockClient.getChats).toHaveBeenLastCalledWith('ws-1', false);
      });
    });
  });

  describe('placeholder', () => {
    it('shows placeholder when no chat selected', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Select a chat to start')).toBeInTheDocument();
      });
    });

    it('shows start new chat button', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(newChatButtons()[1]).toBeInTheDocument();
      });
    });

    it('opens new chat modal from start new chat button', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(newChatButtons()[1]).toBeInTheDocument();
      });

      fireEvent.click(newChatButtons()[1]);

      expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
    });
  });

  describe('new chat modal', () => {
    it('opens new chat modal', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(newChatButtons()[0]).toBeInTheDocument();
      });

      fireEvent.click(newChatButtons()[0]);

      expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      expect(screen.getByLabelText('Select Model')).toBeInTheDocument();
      expect(screen.getByLabelText('Project')).toBeInTheDocument();
      fireEvent.keyDown(screen.getByLabelText('Select Model'), { key: 'ArrowDown' });
      expect(screen.getByRole('option', { name: 'llama2' })).toBeInTheDocument();
      expect(screen.getByRole('option', { name: 'mistral' })).toBeInTheDocument();
      expect(screen.queryByRole('option', { name: 'vectors' })).not.toBeInTheDocument();
    });

    it('offers only the models the current workspace can run', async () => {
      listedModels = [
        { name: 'gpt-4o-mini', size: 0, modified_at: '2026-01-01T00:00:00Z' },
        { name: 'o3', size: 0, modified_at: '2026-01-01T00:00:00Z' },
      ];
      renderChatsPage();

      await waitFor(() => {
        expect(newChatButtons()[0]).toBeInTheDocument();
      });
      fireEvent.click(newChatButtons()[0]);
      fireEvent.keyDown(screen.getByLabelText('Select Model'), { key: 'ArrowDown' });

      expect(modelWorkspaces).toContain('ws-1');
      expect(modelWorkspaces).not.toContain(undefined);
      expect(screen.getAllByRole('option').map((option) => option.textContent)).toEqual([
        'Automatic',
        'gpt-4o-mini',
        'o3',
      ]);
    });

    it('offers Agent mode for a saved model whose tool calling is unknown', async () => {
      listedModels = [
        { name: 'gpt-4o-mini', size: 0, modified_at: '2026-01-01T00:00:00Z' },
        { name: 'o3', size: 0, modified_at: '2026-01-01T00:00:00Z' },
      ];
      renderChatsPage();

      fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);
      expect(screen.getByLabelText('Agent mode')).toBeInTheDocument();

      fireEvent.keyDown(screen.getByLabelText('Select Model'), { key: 'ArrowDown' });
      fireEvent.click(screen.getByRole('option', { name: 'gpt-4o-mini' }));

      await waitFor(() => {
        expect(screen.getByLabelText('Agent mode')).toBeInTheDocument();
      });
    });

    it('closes new chat modal on cancel', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(newChatButtons()[0]).toBeInTheDocument();
      });

      fireEvent.click(newChatButtons()[0]);
      expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();

      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      await waitFor(() => {
        expect(screen.queryByRole('heading', { name: 'New Chat' })).not.toBeInTheDocument();
      });
    });

    it('closes modal on backdrop click', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(newChatButtons()[0]).toBeInTheDocument();
      });

      fireEvent.click(newChatButtons()[0]);

      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      });

      // Click the overlay (backdrop) - try multiple events to trigger Radix UI's handler
      const overlay = document.querySelector('.ui-dialog-overlay');
      fireEvent.pointerDown(overlay!, { isPrimary: true, pointerType: 'mouse' });
      fireEvent.pointerUp(overlay!, { isPrimary: true, pointerType: 'mouse' });
      fireEvent.mouseDown(overlay!);
      fireEvent.mouseUp(overlay!);
      fireEvent.click(overlay!);

      await waitFor(() => {
        expect(screen.queryByRole('heading', { name: 'New Chat' })).not.toBeInTheDocument();
      });
    });

    it('closes modal on escape key', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(newChatButtons()[0]).toBeInTheDocument();
      });

      fireEvent.click(newChatButtons()[0]);

      // Press Escape to close
      fireEvent.keyDown(document, { key: 'Escape' });

      await waitFor(() => {
        expect(screen.queryByRole('heading', { name: 'New Chat' })).not.toBeInTheDocument();
      });
    });

    it('creates chat when form is submitted', async () => {
      const newChat: Chat = {
        id: 'new-chat',
        title: 'Chat with llama2',
        model_name: 'llama2',
        updated_at: '2024-01-05T00:00:00Z',
        archived: false,
        agent_enabled: false,
        created_at: '2024-01-05T00:00:00Z',
      };
      mockClient.createChat.mockResolvedValueOnce(newChat);
      mockClient.getChat.mockResolvedValueOnce({ ...newChat, messages: [] });

      renderChatsPage();

      await waitFor(() => {
        expect(newChatButtons()[0]).toBeInTheDocument();
      });

      fireEvent.click(newChatButtons()[0]);

      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      });

      // Open the select dropdown by clicking the trigger
      const selectTrigger = screen.getByLabelText('Select Model');
      fireEvent.mouseDown(selectTrigger);
      fireEvent.mouseUp(selectTrigger);
      fireEvent.click(selectTrigger);

      // Wait for options to appear and click the llama2 option
      await waitFor(() => {
        expect(screen.getByRole('option', { name: 'llama2' })).toBeInTheDocument();
      });

      const option = screen.getByRole('option', { name: 'llama2' });
      fireEvent.mouseDown(option);
      fireEvent.mouseUp(option);
      fireEvent.click(option);

      // Now submit the form
      const createButton = screen.getByRole('button', { name: 'Create Chat' });
      fireEvent.click(createButton);

      await waitFor(() => {
        expect(mockClient.createChat).toHaveBeenCalledWith({
          workspace_id: 'ws-1',
          title: 'Chat with llama2',
          automatic_title: true,
          model_name: 'llama2',
          agent_enabled: false,
          auto_approve: false,
          context_tokens: 32768,
        });
      });
    });

    it('creates a chat with automatic routing by default', async () => {
      mockCreateChat.mockResolvedValueOnce({
        id: 'chat-auto',
        workspace_id: 'ws-1',
        title: 'New chat',
        model_name: 'auto',
        archived: false,
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-01T00:00:00Z',
      });
      renderChatsPage();

      await waitFor(() => {
        expect(newChatButtons()[0]).toBeInTheDocument();
      });

      fireEvent.click(newChatButtons()[0]);
      fireEvent.click(screen.getByRole('button', { name: 'Create Chat' }));

      await waitFor(() => {
        expect(mockClient.createChat).toHaveBeenCalledWith(
          expect.objectContaining({
            model_name: 'auto',
            title: 'New chat',
            automatic_title: true,
          })
        );
        expect(mockClient.createChat.mock.calls[0][0].project_id).toBeUndefined();
      });
    });

    it('creates an offline chat when Offline is checked', async () => {
      mockCreateChat.mockResolvedValueOnce({
        id: 'chat-offline',
        title: 'New chat',
        model_name: 'auto',
        archived: false,
        agent_enabled: false,
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-01T00:00:00Z',
        offline: true,
      });
      renderChatsPage();
      fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);
      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      });
      expect(screen.getByLabelText('Offline')).toBeInTheDocument();
      fireEvent.click(screen.getByLabelText('Offline'));
      fireEvent.click(screen.getByRole('button', { name: 'Create Chat' }));
      await waitFor(() => {
        expect(mockClient.createChat).toHaveBeenCalledWith(
          expect.objectContaining({ offline: true })
        );
      });
    });

    it('omits offline when creating a chat without checking Offline', async () => {
      mockCreateChat.mockResolvedValueOnce({
        id: 'chat-auto',
        workspace_id: 'ws-1',
        title: 'New chat',
        model_name: 'auto',
        archived: false,
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-01T00:00:00Z',
      });
      renderChatsPage();
      fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);
      fireEvent.click(screen.getByRole('button', { name: 'Create Chat' }));
      await waitFor(() => {
        expect(mockClient.createChat.mock.calls[0][0].offline).toBeUndefined();
        expect(mockClient.createChat.mock.calls[0][0].context_tokens).toBeUndefined();
      });
    });

    it('hides context size until a local model is selected', async () => {
      renderChatsPage();
      fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);
      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      });
      expect(screen.queryByLabelText('Context')).not.toBeInTheDocument();

      const selectTrigger = screen.getByLabelText('Select Model');
      fireEvent.mouseDown(selectTrigger);
      fireEvent.mouseUp(selectTrigger);
      fireEvent.click(selectTrigger);
      await waitFor(() => {
        expect(screen.getByRole('option', { name: 'llama2' })).toBeInTheDocument();
      });
      fireEvent.click(screen.getByRole('option', { name: 'llama2' }));
      expect(screen.getByLabelText('Context')).toBeInTheDocument();
    });

    it('hides context size for a cloud model', async () => {
      listedModels = [{ name: 'gpt-4o-mini', size: 0, modified_at: '2026-01-01T00:00:00Z' }];
      renderChatsPage();
      fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);
      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      });
      const selectTrigger = screen.getByLabelText('Select Model');
      fireEvent.mouseDown(selectTrigger);
      fireEvent.mouseUp(selectTrigger);
      fireEvent.click(selectTrigger);
      await waitFor(() => {
        expect(screen.getByRole('option', { name: 'gpt-4o-mini' })).toBeInTheDocument();
      });
      fireEvent.click(screen.getByRole('option', { name: 'gpt-4o-mini' }));
      expect(screen.queryByLabelText('Context')).not.toBeInTheDocument();
    });

    it('sends a smaller chosen window for a local chat', async () => {
      mockCreateChat.mockResolvedValueOnce({
        id: 'chat-8k',
        title: 'Chat with llama2',
        model_name: 'llama2',
        archived: false,
        agent_enabled: false,
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-01T00:00:00Z',
        context_tokens: 8192,
      });
      renderChatsPage();
      fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);
      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      });
      const modelTrigger = screen.getByLabelText('Select Model');
      fireEvent.mouseDown(modelTrigger);
      fireEvent.mouseUp(modelTrigger);
      fireEvent.click(modelTrigger);
      await waitFor(() => {
        expect(screen.getByRole('option', { name: 'llama2' })).toBeInTheDocument();
      });
      fireEvent.click(screen.getByRole('option', { name: 'llama2' }));
      const contextTrigger = screen.getByLabelText('Context');
      fireEvent.mouseDown(contextTrigger);
      fireEvent.mouseUp(contextTrigger);
      fireEvent.click(contextTrigger);
      fireEvent.click(await screen.findByRole('option', { name: '8K tokens' }));
      fireEvent.click(screen.getByRole('button', { name: 'Create Chat' }));
      await waitFor(() => {
        expect(mockClient.createChat).toHaveBeenCalledWith(
          expect.objectContaining({ model_name: 'llama2', context_tokens: 8192 })
        );
      });
    });

    it('offers a project when creating a chat and sends the chosen id', async () => {
      mockCreateChat.mockResolvedValueOnce({
        id: 'chat-alpha',
        title: 'New chat',
        model_name: 'auto',
        archived: false,
        agent_enabled: false,
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-01T00:00:00Z',
        project_id: 'proj-alpha',
      });
      renderChatsPage();
      fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);
      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      });
      const projectTrigger = screen.getByLabelText('Project');
      fireEvent.mouseDown(projectTrigger);
      fireEvent.mouseUp(projectTrigger);
      fireEvent.click(projectTrigger);
      fireEvent.click(await screen.findByRole('option', { name: 'Alpha' }));
      fireEvent.click(screen.getByRole('button', { name: 'Create Chat' }));
      await waitFor(() => {
        expect(mockClient.createChat).toHaveBeenCalledWith(
          expect.objectContaining({ project_id: 'proj-alpha' })
        );
      });
    });

    it('names Offline on a chat created that way', async () => {
      mockGetChat.mockResolvedValue({
        ...mockChatWithMessages,
        offline: true,
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      expect(await screen.findByTestId('chat-offline')).toHaveTextContent('Offline');
    });

    it('names the project on an assistant chat that belongs to one', async () => {
      mockGetChat.mockResolvedValue({
        ...mockChatWithMessages,
        project_id: 'proj-alpha',
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      expect(await screen.findByTestId('chat-project')).toHaveTextContent('Alpha');
      expect(screen.getByRole('link', { name: 'Alpha' })).toHaveAttribute(
        'href',
        '/projects?id=proj-alpha'
      );
    });

    it('hides Agent mode for a model known not to call tools', async () => {
      renderChatsPage();
      fireEvent.click((await screen.findAllByRole('button', { name: 'New chat' }))[0]);
      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      });
      expect(screen.getByLabelText('Agent mode')).toBeInTheDocument();

      const selectTrigger = screen.getByLabelText('Select Model');
      fireEvent.mouseDown(selectTrigger);
      fireEvent.mouseUp(selectTrigger);
      fireEvent.click(selectTrigger);
      await waitFor(() => {
        expect(screen.getByRole('option', { name: 'llama2' })).toBeInTheDocument();
      });
      fireEvent.click(screen.getByRole('option', { name: 'llama2' }));
      expect(screen.queryByLabelText('Agent mode')).not.toBeInTheDocument();

      fireEvent.mouseDown(selectTrigger);
      fireEvent.mouseUp(selectTrigger);
      fireEvent.click(selectTrigger);
      await waitFor(() => {
        expect(screen.getByRole('option', { name: 'llama3.1' })).toBeInTheDocument();
      });
      fireEvent.click(screen.getByRole('option', { name: 'llama3.1' }));
      await waitFor(() => {
        expect(screen.getByLabelText('Agent mode')).toBeInTheDocument();
      });
    });

    it('keeps a failed create in the dialog, out of the open chat', async () => {
      mockClient.createChat.mockRejectedValueOnce(new Error('Failed to create'));
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-provider');

      fireEvent.click(newChatButtons()[0]);
      await screen.findByRole('heading', { name: 'New Chat' });
      const model = screen.getByLabelText('Select Model');
      fireEvent.keyDown(model, { key: 'ArrowDown' });
      fireEvent.click(await screen.findByRole('option', { name: 'llama2' }));
      fireEvent.click(screen.getByRole('button', { name: 'Create Chat' }));

      await waitFor(() => expect(screen.getByText('Failed to create')).toBeInTheDocument());
      expect(screen.queryByTestId('chat-operation-error')).not.toBeInTheDocument();
    });

    it('shows error when create chat fails', async () => {
      mockClient.createChat.mockRejectedValueOnce(new Error('Failed to create'));

      renderChatsPage();

      await waitFor(() => {
        expect(newChatButtons()[0]).toBeInTheDocument();
      });

      fireEvent.click(newChatButtons()[0]);

      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      });

      // Open the select dropdown by clicking the trigger
      const selectTrigger = screen.getByLabelText('Select Model');
      fireEvent.mouseDown(selectTrigger);
      fireEvent.mouseUp(selectTrigger);
      fireEvent.click(selectTrigger);

      // Wait for options to appear and click the llama2 option
      await waitFor(() => {
        expect(screen.getByRole('option', { name: 'llama2' })).toBeInTheDocument();
      });

      const option = screen.getByRole('option', { name: 'llama2' });
      fireEvent.mouseDown(option);
      fireEvent.mouseUp(option);
      fireEvent.click(option);

      // Now submit the form
      const createButton = screen.getByRole('button', { name: 'Create Chat' });
      fireEvent.click(createButton);

      await waitFor(() => {
        const errorMessages = screen.getAllByText('Failed to create');
        expect(errorMessages.length).toBeGreaterThan(0);
      });
    });
  });

  describe('generation feedback', () => {
    let restoreClock: (() => void) | undefined;

    afterEach(() => {
      restoreClock?.();
      restoreClock = undefined;
    });

    function controlClock(): (elapsed: number) => Promise<void> {
      const ticks = new Map<number, () => void>();
      let nextIntervalId = 1;
      const now = spyOn(performance, 'now').mockReturnValue(0);
      const interval = spyOn(window, 'setInterval').mockImplementation((callback: TimerHandler) => {
        const id = nextIntervalId++;
        ticks.set(id, () => {
          if (typeof callback === 'function') callback();
        });
        return id;
      });
      const clear = spyOn(window, 'clearInterval').mockImplementation((id) => {
        ticks.delete(Number(id));
      });
      restoreClock = () => {
        now.mockRestore();
        interval.mockRestore();
        clear.mockRestore();
      };
      return async (elapsed: number): Promise<void> => {
        // Drain the pending send before touching the clock: `started` and the
        // interval are captured in an effect, and advancing first makes that
        // effect read the moved clock, so elapsed never leaves 0. A sync
        // act() is not enough — the update is still queued in a microtask.
        // waitFor cannot help here either, since it polls on the setInterval
        // this test has stubbed out.
        await act(async () => {});
        // Advancing a clock nothing is subscribed to reports the timer's
        // starting text back as if it never moved, which reads as a broken
        // component rather than a test that measured nothing.
        expect(ticks.size).toBeGreaterThan(0);
        now.mockReturnValue(elapsed);
        act(() => {
          for (const tick of ticks.values()) tick();
        });
      };
    }

    async function sendPrompt(): Promise<HTMLTextAreaElement> {
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      const input = await screen.findByPlaceholderText<HTMLTextAreaElement>(
        'Type a message, or drop a file...'
      );
      fireEvent.change(input, { target: { value: 'Generate an image of a rooster' } });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));
      await waitFor(() => expect(input.value).toBe(''));
      // Generation reads the clock and registers its interval in an effect. Let
      // that run before the caller advances time, or it starts from the advanced
      // reading and reports no elapsed time at all.
      await act(async () => {});
      return input;
    }

    it('keeps elapsed time through status changes and resets it for the next request', async () => {
      const advance = controlClock();
      const input = await sendPrompt();
      expect(screen.getByRole('timer')).toHaveTextContent('0:00');
      expect(screen.getByRole('status').contains(screen.getByRole('timer'))).toBe(false);
      await advance(1000);
      expect(screen.getByRole('timer')).toHaveTextContent('0:01');
      act(() => socket.emit({ type: 'status', message: 'Generating image…' }));
      expect(screen.getByRole('timer')).toHaveTextContent('0:01');
      act(() => socket.emit({ type: 'message_start', message_id: 'reply', role: 'assistant' }));
      expect(screen.getByRole('timer')).toHaveTextContent('0:01');
      act(() => socket.emit({ type: 'message_end', message_id: 'reply', content: 'Done' }));
      expect(screen.queryByRole('timer')).not.toBeInTheDocument();
      fireEvent.change(input, { target: { value: 'Generate another image' } });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));
      expect(await screen.findByRole('timer')).toHaveTextContent('0:00');
      act(() => socket.emit({ type: 'error', message: 'Generation failed' }));
      expect(screen.queryByRole('timer')).not.toBeInTheDocument();
    });

    it('removes elapsed feedback when switching chats and starts fresh on return', async () => {
      const advance = controlClock();
      await sendPrompt();
      expect(await screen.findByRole('timer')).toHaveTextContent('0:00');
      await advance(1000);
      expect(screen.getByRole('timer')).toHaveTextContent('0:01');
      fireEvent.click(screen.getByText('Chat 2'));
      expect(screen.queryByRole('timer')).not.toBeInTheDocument();
      fireEvent.click(screen.getByText('Chat 1'));
      const input = await screen.findByPlaceholderText('Type a message, or drop a file...');
      fireEvent.change(input, { target: { value: 'New image' } });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));
      expect(await screen.findByRole('timer')).toHaveTextContent('0:00');
    });

    it('keeps pending feedback and blocks another send until the response ends', async () => {
      const input = await sendPrompt();
      expect(screen.getByRole('status')).toHaveTextContent('Generating response…');
      expect(screen.getByRole('button', { name: 'Stop' })).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Send' })).not.toBeInTheDocument();
      fireEvent.change(input, { target: { value: 'another prompt' } });
      fireEvent.keyDown(input, { key: 'Enter' });
      expect(mockWsSend).toHaveBeenCalledTimes(1);
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();

      act(() => socket.emit({ type: 'status', message: 'Generating image…' }));
      expect(screen.getByRole('status')).toHaveTextContent('Generating image…');
      act(() => socket.emit({ type: 'message_start', message_id: 'reply', role: 'assistant' }));
      expect(screen.getByRole('status')).toHaveTextContent('Generating response…');
      act(() => socket.emit({ type: 'message_end', message_id: 'reply', content: 'Done' }));
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled();
      expect(screen.getByText('Done')).toBeInTheDocument();
    });

    it('shows image generation errors in the active conversation and permits retry', async () => {
      const input = await sendPrompt();
      act(() => socket.emit({ type: 'error', message: 'Image service is unavailable' }));
      expect(screen.getByRole('alert')).toHaveTextContent('Image service is unavailable');
      expect(screen.getByRole('alert').closest('.chats-main')).not.toBeNull();
      expect(screen.getByText('Hi there!')).toBeInTheDocument();
      expect(screen.getByText('Generate an image of a rooster')).toBeInTheDocument();
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Stop' })).not.toBeInTheDocument();
      fireEvent.change(input, { target: { value: 'Try again' } });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));
      await waitFor(() => expect(mockWsSend).toHaveBeenCalledTimes(2));
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
      expect(screen.getByRole('status')).toHaveTextContent('Generating response…');
    });

    it('shows a terminal stream failure and keeps the interrupted reply visible after retry', async () => {
      const input = await sendPrompt();
      act(() => {
        socket.emit({ type: 'message_start', message_id: 'partial', role: 'assistant' });
        socket.emit({ type: 'chunk', content: 'Partial reply', index: 0 });
        socket.emit({
          type: 'message_end',
          message_id: 'partial',
          content: 'Partial reply\n\n[Response interrupted]',
          error: 'The model connection was interrupted',
        });
      });
      expect(screen.getByRole('alert')).toHaveTextContent('The model connection was interrupted');
      expect(screen.getByText('[Response interrupted]')).toBeInTheDocument();
      expect(screen.queryByRole('status')).not.toBeInTheDocument();

      fireEvent.change(input, { target: { value: 'Retry prompt' } });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));
      await waitFor(() => expect(mockWsSend).toHaveBeenCalledTimes(2));
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
      expect(screen.getByText('[Response interrupted]')).toBeInTheDocument();
    });

    it('keeps the response pending until Stop is acknowledged', async () => {
      await sendPrompt();
      fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
      expect(mockWsSend).toHaveBeenLastCalledWith(JSON.stringify({ type: 'cancel' }));
      expect(screen.getByRole('status')).toBeInTheDocument();
      act(() => socket.emit({ type: 'cancelled', message_id: null }));
      expect(screen.queryByRole('timer')).not.toBeInTheDocument();
      expect(screen.queryByText('Generate an image of a rooster')).not.toBeInTheDocument();
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Stop' })).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Send' })).toBeInTheDocument();
      expect(screen.getByText('Hi there!')).toBeInTheDocument();
    });
  });

  describe('sending messages', () => {
    it('sends message when form is submitted', async () => {
      const newMessage: Message = {
        id: 'msg-3',
        chat_id: 'chat-1',
        role: 'assistant',
        content: 'Response',
        created_at: '2024-01-01T00:02:00Z',
      };
      mockClient.sendMessage.mockResolvedValueOnce(newMessage);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(
          screen.getByPlaceholderText('Type a message, or drop a file...')
        ).toBeInTheDocument();
      });

      fireEvent.change(screen.getByPlaceholderText('Type a message, or drop a file...'), {
        target: { value: 'Test message' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));

      await waitFor(() => {
        expect(mockWsSend).toHaveBeenCalledWith(
          JSON.stringify({
            type: 'send',
            content: 'Test message',
            metadata: undefined,
          })
        );
      });
    });

    it('sends message on Enter key (without shift)', async () => {
      const newMessage: Message = {
        id: 'msg-3',
        chat_id: 'chat-1',
        role: 'assistant',
        content: 'Response',
        created_at: '2024-01-01T00:02:00Z',
      };
      mockClient.sendMessage.mockResolvedValueOnce(newMessage);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(
          screen.getByPlaceholderText('Type a message, or drop a file...')
        ).toBeInTheDocument();
      });

      const textarea = screen.getByPlaceholderText('Type a message, or drop a file...');
      fireEvent.change(textarea, { target: { value: 'Enter test' } });
      fireEvent.keyDown(textarea, { key: 'Enter', shiftKey: false });

      await waitFor(() => {
        expect(mockWsSend).toHaveBeenCalledWith(
          JSON.stringify({
            type: 'send',
            content: 'Enter test',
            metadata: undefined,
          })
        );
      });
    });

    it('does not send message on Shift+Enter', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(
          screen.getByPlaceholderText('Type a message, or drop a file...')
        ).toBeInTheDocument();
      });

      const textarea = screen.getByPlaceholderText('Type a message, or drop a file...');
      fireEvent.change(textarea, { target: { value: 'Shift enter test' } });
      fireEvent.keyDown(textarea, { key: 'Enter', shiftKey: true });

      expect(mockWsSend).not.toHaveBeenCalled();
    });

    it('disables send button when message is empty', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
      });
    });

    it('shows error when sending message fails', async () => {
      mockWsSend.mockImplementationOnce(() => {
        throw new Error('Send failed');
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(
          screen.getByPlaceholderText('Type a message, or drop a file...')
        ).toBeInTheDocument();
      });

      fireEvent.change(screen.getByPlaceholderText('Type a message, or drop a file...'), {
        target: { value: 'Test' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));

      await waitFor(() => {
        expect(screen.getByText('Send failed')).toBeInTheDocument();
      });
    });
  });

  describe('archive/unarchive chat', () => {
    it('archives chat when archive button clicked', async () => {
      const archivedChat: Chat = {
        id: 'chat-1',
        title: 'Chat 1',
        model_name: 'llama2',
        updated_at: '2024-01-01T00:00:00Z',
        archived: true,
        agent_enabled: false,
        created_at: '2024-01-01T00:00:00Z',
      };
      mockClient.archiveChat.mockResolvedValueOnce(archivedChat);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      await chooseMenuItem('Actions for Chat 1', 'Archive');

      await waitFor(() => {
        expect(mockClient.archiveChat).toHaveBeenCalledWith('chat-1');
      });
    });

    it('shows error when archive fails', async () => {
      mockClient.archiveChat.mockRejectedValueOnce(new Error('Archive failed'));

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      await chooseMenuItem('Actions for Chat 1', 'Archive');

      await waitFor(() => {
        expect(screen.getByText('Archive failed')).toBeInTheDocument();
      });
    });

    it('unarchives chat when in archived view', async () => {
      const archivedChats: Chat[] = [
        {
          id: 'chat-archived',
          title: 'Archived Chat',
          model_name: 'llama2',
          updated_at: '2024-01-01T00:00:00Z',
          archived: true,
          agent_enabled: false,
          created_at: '2024-01-01T00:00:00Z',
        },
      ];
      mockClient.getChats.mockResolvedValueOnce(mockChats).mockResolvedValueOnce(archivedChats);
      const unarchivedChat: Chat = {
        id: 'chat-archived',
        title: 'Archived Chat',
        model_name: 'llama2',
        updated_at: '2024-01-01T00:00:00Z',
        archived: false,
        agent_enabled: false,
        created_at: '2024-01-01T00:00:00Z',
      };
      mockClient.unarchiveChat.mockResolvedValueOnce(unarchivedChat);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Archived' })).toBeInTheDocument();
      });

      const archivedTab = screen.getByRole('tab', { name: 'Archived' });
      fireEvent.mouseDown(archivedTab);
      fireEvent.mouseUp(archivedTab);
      fireEvent.click(archivedTab);

      await waitFor(() => {
        expect(screen.getByText('Archived Chat')).toBeInTheDocument();
      });

      await chooseMenuItem('Actions for Archived Chat', 'Unarchive');

      await waitFor(() => {
        expect(mockClient.unarchiveChat).toHaveBeenCalledWith('chat-archived');
      });
    });

    it('shows error when unarchive fails', async () => {
      const archivedChats: Chat[] = [
        {
          id: 'chat-archived',
          title: 'Archived Chat',
          model_name: 'llama2',
          updated_at: '2024-01-01T00:00:00Z',
          archived: true,
          agent_enabled: false,
          created_at: '2024-01-01T00:00:00Z',
        },
      ];
      mockClient.getChats.mockResolvedValueOnce(mockChats).mockResolvedValueOnce(archivedChats);
      mockClient.unarchiveChat.mockRejectedValueOnce(new Error('Unarchive failed'));

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Archived' })).toBeInTheDocument();
      });

      const archivedTab = screen.getByRole('tab', { name: 'Archived' });
      fireEvent.mouseDown(archivedTab);
      fireEvent.mouseUp(archivedTab);
      fireEvent.click(archivedTab);

      await waitFor(() => {
        expect(screen.getByText('Archived Chat')).toBeInTheDocument();
      });

      await chooseMenuItem('Actions for Archived Chat', 'Unarchive');

      await waitFor(() => {
        expect(screen.getByText('Unarchive failed')).toBeInTheDocument();
      });
    });

    it('shows no archived chats message', async () => {
      mockClient.getChats.mockResolvedValueOnce(mockChats).mockResolvedValueOnce([]);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Archived' })).toBeInTheDocument();
      });

      const archivedTab = screen.getByRole('tab', { name: 'Archived' });
      fireEvent.mouseDown(archivedTab);
      fireEvent.mouseUp(archivedTab);
      fireEvent.click(archivedTab);

      await waitFor(() => {
        expect(screen.getByText('No archived chats')).toBeInTheDocument();
      });
    });

    it('clears active chat when archiving it', async () => {
      const archivedChat: Chat = {
        id: 'chat-1',
        title: 'Chat 1',
        model_name: 'llama2',
        updated_at: '2024-01-01T00:00:00Z',
        archived: true,
        agent_enabled: false,
        created_at: '2024-01-01T00:00:00Z',
      };
      mockClient.archiveChat.mockResolvedValueOnce(archivedChat);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByText('Hello')).toBeInTheDocument();
      });
      await waitFor(() => expect(selectedChatParam()).toBe('chat-1'));

      await chooseMenuItem('Actions for Chat 1', 'Archive');

      await waitFor(() => {
        expect(mockClient.archiveChat).toHaveBeenCalledWith('chat-1');
      });
      await waitFor(() => expect(selectedChatParam()).toBeNull());
    });
  });

  describe('delete chat', () => {
    it('opens delete confirmation modal', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      await chooseMenuItem('Actions for Chat 1', 'Delete');

      expect(screen.getByRole('heading', { name: 'Delete Chat' })).toBeInTheDocument();
      expect(
        screen.getByText('Are you sure you want to delete this chat? This action cannot be undone.')
      ).toBeInTheDocument();
      const confirm = screen.getByRole('dialog', { name: 'Delete Chat' });
      expect(confirm.querySelector('.ui-btn-destructive')).toHaveTextContent('Delete');
    });

    it('cancels delete on cancel button', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      await chooseMenuItem('Actions for Chat 1', 'Delete');

      // Click the Cancel button in the modal
      const modalButtons = screen.getAllByRole('button', { name: 'Cancel' });
      fireEvent.click(modalButtons[modalButtons.length - 1]);

      await waitFor(() => {
        expect(screen.queryByRole('heading', { name: 'Delete Chat' })).not.toBeInTheDocument();
      });
      expect(mockClient.deleteChat).not.toHaveBeenCalled();
    });

    it('cancels delete on backdrop click', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      await chooseMenuItem('Actions for Chat 1', 'Delete');

      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'Delete Chat' })).toBeInTheDocument();
      });

      // Click the backdrop (overlay) - try multiple events to trigger Radix UI's handler
      const overlay = document.querySelector('.ui-dialog-overlay');
      fireEvent.pointerDown(overlay!, { isPrimary: true, pointerType: 'mouse' });
      fireEvent.pointerUp(overlay!, { isPrimary: true, pointerType: 'mouse' });
      fireEvent.mouseDown(overlay!);
      fireEvent.mouseUp(overlay!);
      fireEvent.click(overlay!);

      await waitFor(() => {
        expect(screen.queryByRole('heading', { name: 'Delete Chat' })).not.toBeInTheDocument();
      });
    });

    it('cancels delete on escape key', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      await chooseMenuItem('Actions for Chat 1', 'Delete');

      // Press Escape
      fireEvent.keyDown(document, { key: 'Escape' });

      await waitFor(() => {
        expect(screen.queryByRole('heading', { name: 'Delete Chat' })).not.toBeInTheDocument();
      });
    });

    it('deletes chat on confirm', async () => {
      mockClient.deleteChat.mockResolvedValueOnce(undefined);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      await chooseMenuItem('Actions for Chat 1', 'Delete');

      // Click the Delete button in the modal using accessible query
      const deleteModalButtons = screen.getAllByRole('button', { name: 'Delete' });
      // The last Delete button should be the one in the modal
      fireEvent.click(deleteModalButtons[deleteModalButtons.length - 1]);

      await waitFor(() => {
        expect(mockClient.deleteChat).toHaveBeenCalledWith('chat-1');
      });
    });

    it('shows error when delete fails', async () => {
      mockClient.deleteChat.mockRejectedValueOnce(new Error('Delete failed'));

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      await chooseMenuItem('Actions for Chat 1', 'Delete');

      // Click the Delete button in the modal using accessible query
      const deleteModalButtons = screen.getAllByRole('button', { name: 'Delete' });
      fireEvent.click(deleteModalButtons[deleteModalButtons.length - 1]);

      await waitFor(() => {
        expect(screen.getByText('Delete failed')).toBeInTheDocument();
      });
    });

    it('clears active chat when deleting it', async () => {
      mockClient.deleteChat.mockResolvedValueOnce(undefined);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByText('Hello')).toBeInTheDocument();
      });
      await waitFor(() => expect(selectedChatParam()).toBe('chat-1'));

      await chooseMenuItem('Actions for Chat 1', 'Delete');

      const deleteModalButtons = screen.getAllByRole('button', { name: 'Delete' });
      fireEvent.click(deleteModalButtons[deleteModalButtons.length - 1]);

      await waitFor(() => {
        expect(mockClient.deleteChat).toHaveBeenCalledWith('chat-1');
      });
      await waitFor(() => expect(selectedChatParam()).toBeNull());
    });
  });

  describe('chat display', () => {
    it('shows empty messages state', async () => {
      mockClient.getChat.mockResolvedValueOnce(mockChatEmpty);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 2')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 2'));

      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'No messages yet' })).toBeInTheDocument();
        expect(screen.getByText('Send a message to start the conversation')).toBeInTheDocument();
      });
    });

    it('displays system messages correctly', async () => {
      mockClient.getChat.mockResolvedValueOnce(mockChatWithSystemMessage);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 3')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 3'));

      await waitFor(() => {
        expect(screen.getByText('System')).toBeInTheDocument();
        expect(screen.getByText('System message')).toBeInTheDocument();
      });
    });

    it('labels the model badge with the full model id and splits the list meta', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        model_name: 'hf.co/Ttimofeyka/MistralRP-Noromaid-NSFW-Mistral-7B-GGUF:latest',
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      const item = screen.getByText('Chat 1').closest('.chat-item');
      expect(item?.querySelector('.chat-meta-model')).toHaveTextContent('llama2');
      expect(item?.querySelector('.chat-meta-time')).toHaveTextContent('·');

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByText('Hello')).toBeInTheDocument();
      });
      expect(document.querySelector('.chat-header-info .chat-model')).toBeNull();
      const badge = document.querySelector('.chat-settings-trigger .chat-model');
      expect(badge).toHaveTextContent(/^MistralRP-Noromaid-NSFW-Mistral-7B-GGUF:latest$/);
      expect(badge).toHaveAttribute(
        'title',
        'hf.co/Ttimofeyka/MistralRP-Noromaid-NSFW-Mistral-7B-GGUF:latest'
      );
    });

    it('keeps the messages anchored to the bottom when the scroller resizes', async () => {
      const Original = globalThis.ResizeObserver;
      const observers: StubObserver[] = [];
      class StubObserver {
        readonly callback: ResizeObserverCallback;
        readonly targets = new Set<Element>();
        constructor(callback: ResizeObserverCallback) {
          this.callback = callback;
          observers.push(this);
        }
        observe(target: Element): void {
          this.targets.add(target);
        }
        unobserve(target: Element): void {
          this.targets.delete(target);
        }
        disconnect(): void {
          this.targets.clear();
        }
      }
      const watching = (scroller: Element | null) =>
        observers.filter((observer) => scroller !== null && observer.targets.has(scroller));
      globalThis.ResizeObserver = StubObserver as unknown as typeof ResizeObserver;
      try {
        renderChatsPage();

        await waitFor(() => {
          expect(screen.getByText('Chat 1')).toBeInTheDocument();
        });

        fireEvent.click(screen.getByText('Chat 1'));

        await waitFor(() => {
          expect(screen.getByText('Hello')).toBeInTheDocument();
          expect(watching(document.querySelector('.messages-container'))).not.toHaveLength(0);
        });
        const container = document.querySelector('.messages-container') as HTMLDivElement;
        Object.defineProperty(container, 'scrollHeight', { configurable: true, value: 900 });
        const scrollTo = spyOn(container, 'scrollTo').mockImplementation(() => {});

        act(() => {
          for (const observer of watching(container)) {
            observer.callback([], observer as unknown as ResizeObserver);
          }
        });

        expect(scrollTo).toHaveBeenCalledWith({ top: 900, behavior: 'instant' });
      } finally {
        globalThis.ResizeObserver = Original;
      }
    });

    it('badges an assistant reply that read stored memory, never a user message', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        messages: mockChatWithMessages.messages.map((message) => ({
          ...message,
          metadata: { memory_used: true },
        })),
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByText('Hi there!')).toBeInTheDocument();
      });
      const badges = screen.getAllByTestId('memory-badge');
      expect(badges).toHaveLength(1);
      expect(badges[0]).toHaveTextContent('Memory read');
      expect(badges[0].closest('.message')).toHaveClass('message-assistant');
      expect(screen.getByText('Hello').closest('.message')).toHaveClass('message-user');
    });

    it('marks the turns automations sent, and no turn a person typed', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        messages: [
          {
            id: 'msg-reminder',
            chat_id: 'chat-1',
            role: 'user',
            content: 'Check the deploy queue',
            created_at: '2024-01-01T00:00:00Z',
            metadata: { source: 'reminder', reminder_id: 'reminder-1' },
          },
          ...mockChatWithMessages.messages,
          {
            id: 'msg-notice',
            chat_id: 'chat-1',
            role: 'assistant',
            content: 'Project build finished',
            created_at: '2024-01-01T00:01:30Z',
            metadata: { source: 'auto_project' },
          },
          {
            id: 'msg-unknown',
            chat_id: 'chat-1',
            role: 'user',
            content: 'Posted elsewhere',
            created_at: '2024-01-01T00:02:00Z',
            metadata: { source: 'somewhere_new' },
          },
        ],
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByText('Check the deploy queue')).toBeInTheDocument();
      });
      const badgeIn = (text: string) =>
        screen
          .getByText(text)
          .closest('.message')
          ?.querySelector('[data-testid="automation-badge"]');
      expect(screen.getAllByTestId('automation-badge')).toHaveLength(2);
      const reminder = badgeIn('Check the deploy queue');
      expect(reminder).toHaveTextContent('Reminder');
      expect(reminder?.closest('.message-header')).not.toBeNull();
      expect(reminder?.closest('.message')).toHaveClass('message-user');
      const notice = badgeIn('Project build finished');
      expect(notice).toHaveTextContent('Auto project');
      expect(notice?.closest('.message')).toHaveClass('message-assistant');
      expect(badgeIn('Hello')).toBeNull();
      expect(badgeIn('Hi there!')).toBeNull();
      expect(badgeIn('Posted elsewhere')).toBeNull();
    });

    it('selects chat via keyboard Enter', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      const chatItem = screen.getByText('Chat 1').closest('[role="button"]');
      fireEvent.keyDown(chatItem!, { key: 'Enter' });

      await waitFor(() => {
        expect(mockClient.getChat).toHaveBeenCalledWith('chat-1');
      });
    });

    it('renders an attached image in the message', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        messages: [
          {
            id: 'msg-img',
            chat_id: 'chat-1',
            role: 'user',
            content: "That's what I meant",
            created_at: '2024-01-01T00:00:00Z',
            metadata: {
              attachments: [
                {
                  name: 'shot.png',
                  mime: 'image/png',
                  url: 'data:image/png;base64,aaaa',
                },
              ],
            },
          },
        ],
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        const image = screen.getByRole('img', { name: 'shot.png' });
        expect(image).toHaveAttribute('src', 'data:image/png;base64,aaaa');
      });
    });

    it('renders generated assistant images as full-size links', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        messages: [
          {
            id: 'msg-generated',
            chat_id: 'chat-1',
            role: 'assistant',
            content: 'Here is the image.',
            created_at: '2024-01-01T00:00:00Z',
            metadata: {
              attachments: [
                {
                  name: 'generated-image-1.webp',
                  mime: 'image/webp',
                  url: 'data:image/webp;base64,generated',
                },
              ],
            },
          },
        ],
      });

      renderChatsPage();
      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        const image = screen.getByRole('img', { name: 'generated-image-1.webp' });
        expect(image).toHaveAttribute('src', 'data:image/webp;base64,generated');
        expect(image).toHaveAttribute('loading', 'lazy');
        expect(
          screen.getByRole('link', { name: 'Open generated-image-1.webp full size' })
        ).toHaveAttribute('target', '_blank');
      });
    });

    it('renders generated assistant videos with playback controls', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        messages: [
          {
            id: 'msg-generated-video',
            chat_id: 'chat-1',
            role: 'assistant',
            content: 'Generated video.',
            created_at: '2024-01-01T00:00:00Z',
            metadata: {
              attachments: [
                {
                  name: 'generated-video-1.webm',
                  mime: 'video/webm',
                  url: 'data:video/webm;base64,generated',
                },
              ],
            },
          },
        ],
      });

      renderChatsPage();
      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        const video = screen.getByLabelText('generated-video-1.webm');
        expect(video.tagName).toBe('VIDEO');
        expect(video).toHaveAttribute('src', 'data:video/webm;base64,generated');
        expect(video).toHaveAttribute('controls');
        expect(screen.getByText('Generated video.')).toBeInTheDocument();
      });
    });

    it('renders generated assistant audio with playback controls', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        messages: [
          {
            id: 'msg-generated-audio',
            chat_id: 'chat-1',
            role: 'assistant',
            content: 'Generated audio.',
            created_at: '2024-01-01T00:00:00Z',
            metadata: {
              attachments: [
                {
                  name: 'generated-audio-1.flac',
                  mime: 'audio/flac',
                  url: 'data:audio/flac;base64,generated',
                },
              ],
            },
          },
        ],
      });

      renderChatsPage();
      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        const audio = screen.getByTestId('message-audio');
        expect(audio.tagName).toBe('AUDIO');
        expect(audio).toHaveAttribute('aria-label', 'generated-audio-1.flac');
        expect(audio).toHaveAttribute('src', 'data:audio/flac;base64,generated');
        expect(audio).toHaveAttribute('controls');
        expect(screen.getByText('Generated audio.')).toBeInTheDocument();
      });
    });

    it('reuses a thread image as the next starting image', async () => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        messages: [
          {
            id: 'msg-generated',
            chat_id: 'chat-1',
            role: 'assistant',
            content: 'Here is the image.',
            created_at: '2024-01-01T00:00:00Z',
            metadata: {
              attachments: [
                {
                  name: 'generated-image-1.webp',
                  mime: 'image/webp',
                  url: 'data:image/webp;base64,generated',
                },
              ],
            },
          },
        ],
      });

      renderChatsPage();
      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Use as starting image' })).toBeInTheDocument();
      });
      fireEvent.click(screen.getByRole('button', { name: 'Use as starting image' }));
      expect(screen.getByText('Starting image')).toBeInTheDocument();
      expect(
        screen.getByText(
          'Ask to generate, edit, remove an object, change the setting, or animate and this image will be the starting point.'
        )
      ).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Added as starting image' })).toBeDisabled();

      fireEvent.change(screen.getByPlaceholderText('Type a message, or drop a file...'), {
        target: { value: 'Make this a watercolor' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));
      await waitFor(() => {
        expect(mockWsSend).toHaveBeenCalledWith(
          JSON.stringify({
            type: 'send',
            content: 'Make this a watercolor',
            metadata: {
              attachments: [
                {
                  name: 'generated-image-1.webp',
                  mime: 'image/webp',
                  url: 'data:image/webp;base64,generated',
                },
              ],
            },
          })
        );
      });
    });

    it('displays chat model name', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(document.querySelector('.chat-model')).toHaveTextContent('llama2');
      });
    });
  });

  it('keeps agent mode without a separate host access control', async () => {
    mockClient.getChat.mockResolvedValueOnce({ ...mockChatWithMessages, agent_enabled: true });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() =>
      expect(screen.getByTestId('agent-toggle')).toHaveAttribute('aria-pressed', 'true')
    );
    expect(screen.queryByTestId('sandbox-toggle')).not.toBeInTheDocument();
    expect(screen.queryByText('Host access')).not.toBeInTheDocument();
    expect(screen.getByTestId('agent-toggle')).toHaveAttribute(
      'title',
      'Agent mode on: replies can search this workspace, run server commands, and read or write server files'
    );
  });

  it('lets the reader pick reasoning effort when the model can think', async () => {
    mockClient.getChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      reasoning: true,
      reasoning_effort: 'auto',
    });
    mockClient.updateChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      reasoning: true,
      reasoning_effort: 'high',
    });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByTestId('reasoning-effort')).toBeInTheDocument());
    chooseSelectOption('reasoning-effort', 'High');
    await waitFor(() => {
      expect(mockClient.updateChat).toHaveBeenCalledWith('chat-1', { reasoning_effort: 'high' });
    });
  });

  it('hides reasoning effort when the model cannot think', async () => {
    mockClient.getChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      reasoning: false,
    });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByText('Hi there!')).toBeInTheDocument());
    expect(screen.queryByTestId('reasoning-effort')).not.toBeInTheDocument();
  });

  it('lets the reader pick context size when the chat uses a local model', async () => {
    mockClient.getChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      context_tokens: 32768,
    });
    mockClient.updateChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      context_tokens: 8192,
    });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByTestId('context-tokens')).toBeInTheDocument());
    chooseSelectOption('context-tokens', '8K tokens');
    await waitFor(() => {
      expect(mockClient.updateChat).toHaveBeenCalledWith('chat-1', { context_tokens: 8192 });
    });
  });

  it('shows used and total on the meter from the chosen window', async () => {
    mockClient.getChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      context_tokens: 32768,
      context: null,
    });
    mockClient.updateChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      context_tokens: 131072,
    });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByTestId('context-tokens')).toBeInTheDocument());
    expect(document.querySelector('.context-usage-toggle')?.textContent).toContain('0 / 32K');
    chooseSelectOption('context-tokens', '128K tokens');
    await waitFor(() => {
      expect(document.querySelector('.context-usage-toggle')?.textContent).toContain('0 / 128K');
    });
  });

  it('hides context size for an Automatic chat', async () => {
    mockClient.getChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      model_name: 'auto',
    });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByText('Hi there!')).toBeInTheDocument());
    expect(screen.queryByTestId('context-tokens')).not.toBeInTheDocument();
  });

  it('shows auto-approve only while agent mode is on', async () => {
    mockClient.getChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      agent_enabled: true,
      auto_approve: false,
    });
    mockClient.updateChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      agent_enabled: true,
      auto_approve: true,
    });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByTestId('auto-approve-toggle')).toBeInTheDocument());
    expect(screen.getByTestId('auto-approve-toggle')).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(screen.getByTestId('auto-approve-toggle'));
    await waitFor(() => {
      expect(mockClient.updateChat).toHaveBeenCalledWith('chat-1', { auto_approve: true });
    });
  });

  it('shows the agent confined to Zone tools until the reader says otherwise', async () => {
    mockGetChatProviders.mockResolvedValue(chatProviders({ cli: true }));
    mockClient.getChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      agent_enabled: true,
      agent_sandboxed: true,
    });
    mockClient.updateChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      agent_enabled: true,
      agent_sandboxed: false,
    });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByTestId('agent-sandbox-toggle')).toBeInTheDocument());
    expect(screen.getByTestId('agent-sandbox-toggle')).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(screen.getByTestId('agent-sandbox-toggle'));
    await waitFor(() => {
      expect(mockClient.updateChat).toHaveBeenCalledWith('chat-1', { agent_sandboxed: false });
    });
    await waitFor(() =>
      expect(screen.getByTestId('agent-sandbox-toggle')).toHaveAttribute('aria-pressed', 'false')
    );
  });

  it('puts an agent that kept its own tools back inside Zone', async () => {
    mockGetChatProviders.mockResolvedValue(chatProviders({ cli: true }));
    mockClient.getChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      agent_enabled: true,
      agent_sandboxed: false,
    });
    mockClient.updateChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      agent_enabled: true,
      agent_sandboxed: true,
    });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByTestId('agent-sandbox-toggle')).toBeInTheDocument());
    expect(screen.getByTestId('agent-sandbox-toggle')).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByTestId('agent-sandbox-toggle').getAttribute('title')).toContain(
      'run commands on this host'
    );
    fireEvent.click(screen.getByTestId('agent-sandbox-toggle'));
    await waitFor(() => {
      expect(mockClient.updateChat).toHaveBeenCalledWith('chat-1', { agent_sandboxed: true });
    });
  });

  it('leaves auto-approve alone when the reader changes tool access', async () => {
    mockGetChatProviders.mockResolvedValue(chatProviders({ cli: true }));
    mockClient.getChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      agent_enabled: true,
      agent_sandboxed: true,
      auto_approve: true,
    });
    mockClient.updateChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      agent_enabled: true,
      agent_sandboxed: false,
      auto_approve: true,
    });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByTestId('agent-sandbox-toggle')).toBeInTheDocument());
    fireEvent.click(screen.getByTestId('agent-sandbox-toggle'));
    await waitFor(() => {
      expect(mockClient.updateChat).toHaveBeenCalledWith('chat-1', { agent_sandboxed: false });
    });
    expect(screen.getByTestId('auto-approve-toggle')).toHaveAttribute('aria-pressed', 'true');
  });

  describe('Zone tools only toggle', () => {
    const openAgentChat = async (providers: ChatProviders) => {
      mockGetChatProviders.mockResolvedValue(providers);
      mockClient.getChat.mockResolvedValue({ ...mockChatWithMessages, agent_enabled: true });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-provider');
    };

    it('shows on a chat that chose an agent provider', async () => {
      await openAgentChat(chatProviders({ selected: selection(claudeOffer, null), cli: true }));
      fireEvent.click(screen.getByTestId('agent-permissions'));
      expect(screen.getByRole('button', { name: 'Zone tools only' })).toBe(
        screen.getByTestId('agent-sandbox-toggle')
      );
    });

    it('stays hidden on a chat that chose a self-hosted provider', async () => {
      await openAgentChat(
        chatProviders({ selected: selection(selfHostedOffer, null), cli: false })
      );
      expect(screen.queryByTestId('agent-sandbox-toggle')).not.toBeInTheDocument();
      expect(screen.getByTestId('auto-approve-toggle')).toBeInTheDocument();
    });

    it('shows on a Default chat that resolves to an agent', async () => {
      await openAgentChat(chatProviders({ default: codexOffer, cli: true }));
      expect(screen.getByTestId('agent-sandbox-toggle')).toBeInTheDocument();
    });

    it('shows on a Default chat the instance runs on an agent behind a self-hosted default', async () => {
      await openAgentChat(chatProviders({ default: selfHostedOffer, cli: true }));
      expect(screen.getByTestId('agent-sandbox-toggle')).toBeInTheDocument();
    });

    it('stays hidden on a Default chat that resolves to self-hosted', async () => {
      await openAgentChat(chatProviders({ default: selfHostedOffer, cli: false }));
      expect(screen.queryByTestId('agent-sandbox-toggle')).not.toBeInTheDocument();
    });

    it('stays hidden on a Default chat that resolves to another HTTP provider', async () => {
      await openAgentChat(chatProviders({ default: openAiOffer, cli: false }));
      expect(screen.queryByTestId('agent-sandbox-toggle')).not.toBeInTheDocument();
    });

    it('keeps the way back and the warning for an unsandboxed chat when providers fail to load', async () => {
      mockGetChatProviders.mockRejectedValue(new Error('Failed to load AI providers: 502'));
      mockClient.getChat.mockResolvedValue({
        ...mockChatWithMessages,
        agent_enabled: true,
        agent_sandboxed: false,
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-providers-error');
      expect(screen.getByTestId('agent-sandbox-toggle')).toHaveAttribute('aria-pressed', 'false');
      expect(screen.getByTestId('agent-permissions')).toHaveTextContent('Host tools');
    });

    it('stays hidden for a sandboxed chat when providers fail to load', async () => {
      mockGetChatProviders.mockRejectedValue(new Error('Failed to load AI providers: 502'));
      mockClient.getChat.mockResolvedValue({ ...mockChatWithMessages, agent_enabled: true });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-providers-error');
      expect(screen.queryByTestId('agent-sandbox-toggle')).not.toBeInTheDocument();
    });

    it('stays hidden for a sandboxed chat when the server does not say', async () => {
      await openAgentChat(chatProviders());
      expect(screen.queryByTestId('agent-sandbox-toggle')).not.toBeInTheDocument();
    });

    it('keeps the way back for an unsandboxed chat when the server does not say', async () => {
      mockGetChatProviders.mockResolvedValue(chatProviders());
      mockClient.getChat.mockResolvedValue({
        ...mockChatWithMessages,
        agent_enabled: true,
        agent_sandboxed: false,
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-provider');
      expect(screen.getByTestId('agent-sandbox-toggle')).toHaveAttribute('aria-pressed', 'false');
    });

    it('hides for an unsandboxed chat once the server says it runs over HTTP', async () => {
      mockGetChatProviders.mockResolvedValue(chatProviders({ cli: false }));
      mockClient.getChat.mockResolvedValue({
        ...mockChatWithMessages,
        agent_enabled: true,
        agent_sandboxed: false,
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-provider');
      await waitFor(() =>
        expect(screen.queryByTestId('agent-sandbox-toggle')).not.toBeInTheDocument()
      );
    });

    const openUnsandboxedChat = async () => {
      mockGetChatProviders.mockResolvedValue(chatProviders({ cli: false }));
      mockClient.getChat.mockResolvedValue({
        ...mockChatWithMessages,
        agent_enabled: true,
        agent_sandboxed: false,
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-provider');
      await waitFor(() =>
        expect(screen.queryByTestId('agent-sandbox-toggle')).not.toBeInTheDocument()
      );
    };

    const sendTurn = () => {
      fireEvent.change(screen.getByPlaceholderText('Type a message, or drop a file...'), {
        target: { value: 'Next turn' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    };

    it('warns when a provider change lands but reading the providers back fails', async () => {
      await openUnsandboxedChat();
      mockSetChatProvider.mockResolvedValue({
        ...mockChatWithMessages,
        agent_enabled: true,
        agent_sandboxed: false,
        provider: 'claude_code',
      });
      mockGetChatProviders.mockRejectedValue(new Error('Failed to load AI providers: 502'));

      chooseSelectOption('chat-provider', 'Claude Code');

      await screen.findByTestId('chat-providers-error');
      expect(screen.getByTestId('agent-sandbox-toggle')).toHaveAttribute('aria-pressed', 'false');
      expect(screen.getByTestId('agent-permissions')).toHaveTextContent('Host tools');
    });

    it('picks up a default that changed between turns as the next turn starts', async () => {
      await openUnsandboxedChat();
      mockGetChatProviders.mockResolvedValue(chatProviders({ cli: true }));

      sendTurn();

      await screen.findByTestId('agent-sandbox-toggle');
      expect(screen.getByTestId('agent-permissions')).toHaveTextContent('Host tools');
    });

    it('reads the providers again when a turn ends', async () => {
      await openUnsandboxedChat();
      sendTurn();
      await waitFor(() => expect(mockWsSend).toHaveBeenCalled());
      await waitFor(() => expect(mockGetChatProviders.mock.calls.length).toBe(2));
      mockGetChatProviders.mockResolvedValue(chatProviders({ cli: true }));

      act(() => socket.emit({ type: 'message_start', message_id: 'reply', role: 'assistant' }));
      act(() => socket.emit({ type: 'message_end', message_id: 'reply', content: 'Done' }));

      await screen.findByTestId('agent-sandbox-toggle');
    });

    it('sends the turn without waiting for a slow read of the providers', async () => {
      await openUnsandboxedChat();
      mockGetChatProviders.mockReturnValue(new Promise(() => {}));

      sendTurn();

      await waitFor(() => expect(mockWsSend).toHaveBeenCalled());
      expect(await screen.findByRole('status')).toHaveTextContent('Generating response…');
      await waitFor(() => expect(mockGetChatProviders).toHaveBeenCalledTimes(2));
    });

    it('hides the toggle again once a later read says the chat runs over HTTP', async () => {
      await openUnsandboxedChat();
      mockGetChatProviders.mockRejectedValue(new Error('Failed to load AI providers: 502'));
      sendTurn();
      await screen.findByTestId('agent-sandbox-toggle');

      mockGetChatProviders.mockResolvedValue(chatProviders({ cli: false }));
      act(() => socket.emit({ type: 'message_start', message_id: 'reply', role: 'assistant' }));
      act(() => socket.emit({ type: 'message_end', message_id: 'reply', content: 'Done' }));

      await waitFor(() =>
        expect(screen.queryByTestId('agent-sandbox-toggle')).not.toBeInTheDocument()
      );
      expect(screen.queryByTestId('chat-providers-error')).not.toBeInTheDocument();
    });

    it('keeps the toggle on a sandboxed CLI chat through a failed read', async () => {
      mockGetChatProviders.mockResolvedValue(chatProviders({ cli: true }));
      mockClient.getChat.mockResolvedValue({
        ...mockChatWithMessages,
        agent_enabled: true,
        agent_sandboxed: true,
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('agent-sandbox-toggle');
      mockGetChatProviders.mockRejectedValue(new Error('Failed to load AI providers: 502'));

      sendTurn();

      await screen.findByTestId('chat-providers-error');
      expect(screen.getByTestId('agent-sandbox-toggle')).toHaveAttribute('aria-pressed', 'true');
    });

    it('warns while a provider change is still being read back', async () => {
      await openUnsandboxedChat();
      mockSetChatProvider.mockResolvedValue({
        ...mockChatWithMessages,
        agent_enabled: true,
        agent_sandboxed: false,
        provider: 'claude_code',
      });
      mockGetChatProviders.mockReturnValue(new Promise(() => {}));

      chooseSelectOption('chat-provider', 'Claude Code');

      await screen.findByTestId('agent-sandbox-toggle');
      expect(screen.getByTestId('agent-permissions')).toHaveTextContent('Host tools');
    });

    it('keeps the turn running when reading the providers fails', async () => {
      await openUnsandboxedChat();
      mockGetChatProviders.mockRejectedValue(new Error('Failed to load AI providers: 502'));

      sendTurn();

      await screen.findByTestId('chat-providers-error');
      expect(mockWsSend).toHaveBeenCalled();
      expect(screen.getByRole('status')).toHaveTextContent('Generating response…');
      expect(screen.getByTestId('agent-sandbox-toggle')).toHaveAttribute('aria-pressed', 'false');
    });

    it('follows the provider as it changes in the picker', async () => {
      await openAgentChat(chatProviders({ cli: false }));
      expect(screen.queryByTestId('agent-sandbox-toggle')).not.toBeInTheDocument();

      mockSetChatProvider.mockResolvedValue({
        ...mockChatWithMessages,
        agent_enabled: true,
        provider: 'claude_code',
      });
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ selected: selection(claudeOffer, null), cli: true })
      );
      fireEvent.click(screen.getByTestId('chat-settings'));
      chooseSelectOption('chat-provider', 'Claude Code');
      await screen.findByTestId('agent-sandbox-toggle');

      mockSetChatProvider.mockResolvedValue({ ...mockChatWithMessages, agent_enabled: true });
      mockGetChatProviders.mockResolvedValue(chatProviders({ cli: false }));
      chooseSelectOption('chat-provider', 'Default (Self-Hosted)');
      await waitFor(() =>
        expect(screen.queryByTestId('agent-sandbox-toggle')).not.toBeInTheDocument()
      );
    });
  });

  describe('chat header layout', () => {
    const openHeader = async (chat: Partial<ChatWithMessages> = {}) => {
      mockClient.getChat.mockResolvedValue({ ...mockChatWithMessages, ...chat });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-provider');
    };

    it('shows the model once, shortened, with the full name in its tooltip', async () => {
      const name = 'orcarouter/Qwen3.8-27B-Uncensored:Q4_K_M';
      await openHeader({ model_name: name });
      const trigger = screen.getByTestId('chat-settings');
      const model = trigger.querySelector('.chat-model');
      expect(model).toHaveTextContent(/^Qwen3.8-27B-Uncensored:Q4_K_M$/);
      expect(model).toHaveAttribute('title', name);
      expect(trigger).toHaveAttribute('aria-label', `Model settings: Default · ${name}`);
      expect(document.querySelectorAll('.chat-model')).toHaveLength(1);
    });

    it('sums up the provider, the model and the effort on the one control', async () => {
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ selected: selection(codexOffer, 'gpt-5.5'), cli: true })
      );
      await openHeader({ provider: 'codex', provider_model: 'gpt-5.5', reasoning_effort: 'high' });
      await waitFor(() =>
        expect(screen.getByTestId('chat-settings')).toHaveAttribute(
          'aria-label',
          'Model settings: Codex · gpt-5.5 · High'
        )
      );
    });

    it('holds the pickers in a panel the summary opens and Escape closes', async () => {
      await openHeader();
      const trigger = screen.getByTestId('chat-settings');
      expect(trigger).toHaveAttribute('aria-expanded', 'false');
      expect(screen.queryByRole('dialog', { name: 'Model settings' })).not.toBeInTheDocument();

      fireEvent.click(trigger);
      expect(trigger).toHaveAttribute('aria-expanded', 'true');
      const panel = screen.getByRole('dialog', { name: 'Model settings' });
      expect(panel.contains(screen.getByTestId('chat-provider'))).toBe(true);

      fireEvent.keyDown(document.body, { key: 'Escape' });
      expect(trigger).toHaveAttribute('aria-expanded', 'false');
      expect(document.activeElement).toBe(trigger);
    });

    it('keeps a provider error in view while the panel is closed', async () => {
      mockGetChatProviders.mockResolvedValue(
        chatProviders({
          selected: selection(codexOffer, null, { usable: false, reason: 'Codex is signed out.' }),
        })
      );
      await openHeader({ provider: 'codex' });
      expect(screen.getByRole('alert')).toHaveTextContent('Codex is signed out.');
      expect(screen.getByTestId('chat-settings')).toHaveAttribute('aria-expanded', 'false');
    });

    it('still names the model when nothing can be chosen', async () => {
      mockGetChatProviders.mockRejectedValue(new Error('Failed to load AI providers: 502'));
      mockClient.getChat.mockResolvedValue(mockChatWithMessages);
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-providers-error');
      const trigger = screen.getByTestId('chat-settings');
      expect(trigger.querySelector('.chat-model')).toHaveTextContent('llama2');
    });

    it('keeps the agent toggle visible and tucks its permissions behind a labelled trigger', async () => {
      mockGetChatProviders.mockResolvedValue(chatProviders({ cli: true }));
      await openHeader({ agent_enabled: true, auto_approve: true, agent_sandboxed: false });
      const trigger = screen.getByTestId('agent-permissions');
      expect(trigger).toHaveAttribute(
        'aria-label',
        'Agent permissions: auto-approve on, Zone tools only off'
      );
      expect(
        [...trigger.querySelectorAll('.agent-permission-chip')].map((chip) => chip.textContent)
      ).toEqual(['Auto-approve', 'Host tools']);
      expect(trigger.querySelector('.agent-permission-chip--host')).toHaveTextContent('Host tools');
      expect(screen.getByRole('button', { name: /^Agent$/ })).toBe(
        screen.getByTestId('agent-toggle')
      );

      fireEvent.click(trigger);
      expect(screen.getByRole('button', { name: 'Auto-approve', pressed: true })).toBe(
        screen.getByTestId('auto-approve-toggle')
      );
      expect(screen.getByRole('button', { name: 'Zone tools only', pressed: false })).toBe(
        screen.getByTestId('agent-sandbox-toggle')
      );
      expect(screen.getByTestId('auto-approve-toggle')).toHaveTextContent('On');
      expect(screen.getByTestId('agent-sandbox-toggle')).toHaveTextContent('Off');
      expect(screen.getByTestId('agent-sandbox-toggle')).toHaveClass('agent-permission--warning');
      expect(screen.getByTestId('auto-approve-toggle')).not.toHaveClass(
        'agent-permission--warning'
      );
    });

    it('labels the permissions trigger even when no permission is loosened', async () => {
      mockGetChatProviders.mockResolvedValue(chatProviders({ cli: true }));
      await openHeader({ agent_enabled: true, auto_approve: false, agent_sandboxed: true });
      const trigger = screen.getByTestId('agent-permissions');
      expect(trigger).toHaveAttribute(
        'aria-label',
        'Agent permissions: auto-approve off, Zone tools only on'
      );
      expect(trigger).toHaveTextContent('Permissions');
      expect(trigger).not.toHaveTextContent('Host tools');
      expect(trigger.getAttribute('aria-label')?.toLowerCase()).toContain(
        trigger.querySelector('.agent-permission-chip-label')?.textContent?.toLowerCase()
      );

      fireEvent.click(trigger);
      expect(screen.getByTestId('agent-sandbox-toggle')).not.toHaveClass(
        'agent-permission--warning'
      );
    });

    it('offers no permissions while agent mode is off', async () => {
      await openHeader({ tools: true, agent_enabled: false });
      expect(screen.getByTestId('agent-toggle')).toHaveAttribute('aria-pressed', 'false');
      expect(screen.queryByTestId('agent-permissions')).not.toBeInTheDocument();
    });
  });

  it('hides the tool-access toggle when the chat is not agentic', async () => {
    mockClient.getChat.mockResolvedValueOnce({ ...mockChatWithMessages, tools: true });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByTestId('agent-toggle')).toBeInTheDocument());
    expect(screen.queryByTestId('agent-sandbox-toggle')).not.toBeInTheDocument();
  });

  it('saves a pasted character card onto the open chat', async () => {
    mockClient.getChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      needs_character: true,
    });
    mockClient.updateChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      needs_character: true,
      character: { name: 'Ada', system_prompt: 'Stay in character as Ada.' },
    });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByTestId('character-toggle')).toBeInTheDocument());
    fireEvent.click(screen.getByTestId('character-toggle'));
    fireEvent.change(screen.getByLabelText('Card JSON or system prompt'), {
      target: { value: 'Stay in character as Ada.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save character' }));
    await waitFor(() => {
      expect(mockClient.updateChat).toHaveBeenCalledWith(
        'chat-1',
        expect.objectContaining({
          character: expect.objectContaining({
            name: 'Character',
            system_prompt: 'Stay in character as Ada.',
          }),
        })
      );
    });
  });

  it('hides auto-approve when the chat is not agentic', async () => {
    mockClient.getChat.mockResolvedValueOnce({ ...mockChatWithMessages, tools: true });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByTestId('agent-toggle')).toBeInTheDocument());
    expect(screen.queryByTestId('auto-approve-toggle')).not.toBeInTheDocument();
  });

  it('hides Agent and Character until the model needs them', async () => {
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByText('Hi there!')).toBeInTheDocument());
    expect(screen.queryByTestId('agent-toggle')).not.toBeInTheDocument();
    expect(screen.queryByTestId('character-toggle')).not.toBeInTheDocument();
  });

  it('shows Agent for a tool-capable model and Character when a card is needed', async () => {
    mockClient.getChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      tools: true,
      needs_character: true,
    });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByTestId('agent-toggle')).toBeInTheDocument());
    expect(screen.getByTestId('character-toggle')).toBeInTheDocument();
  });

  it('keeps Agent and Character visible so an active setting can be turned off', async () => {
    mockClient.getChat.mockResolvedValueOnce({
      ...mockChatWithMessages,
      agent_enabled: true,
      character: { name: 'Ada' },
    });
    renderChatsPage();
    await waitFor(() => expect(screen.getByText('Chat 1')).toBeInTheDocument());
    fireEvent.click(screen.getByText('Chat 1'));
    await waitFor(() => expect(screen.getByTestId('agent-toggle')).toBeInTheDocument());
    expect(screen.getByTestId('character-toggle')).toHaveTextContent('Ada');
  });

  describe('chat provider', () => {
    const openChat = async (chat: Partial<ChatWithMessages> = {}) => {
      mockClient.getChat.mockResolvedValue({ ...mockChatWithMessages, ...chat });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByText('Hi there!');
      fireEvent.click(screen.getByTestId('chat-settings'));
    };

    const optionNames = (testId: string): string[] => {
      const trigger = screen.getByTestId(testId);
      fireEvent.keyDown(trigger, { key: 'ArrowDown' });
      const names = screen.getAllByRole('option').map((option) => option.textContent ?? '');
      fireEvent.keyDown(trigger, { key: 'Escape' });
      return names;
    };

    const badge = () => document.querySelector('.chat-settings-trigger');

    it('renders the picker on the workspace default', async () => {
      await openChat();
      const picker = await screen.findByTestId('chat-provider');
      expect(picker).toHaveTextContent('Default');
      expect(picker).not.toHaveTextContent('Self-Hosted');
      expect(picker.getAttribute('title')).toContain('(Self-Hosted)');
      expect(optionNames('chat-provider')[0]).toBe('Default (Self-Hosted)');
      expect(screen.queryByTestId('chat-provider-model')).not.toBeInTheDocument();
      expect(badge()).toHaveTextContent('llama2');
    });

    it('chooses Claude Code and refreshes both the providers and the chat', async () => {
      await openChat();
      await screen.findByTestId('chat-provider');
      const providerLoads = mockGetChatProviders.mock.calls.length;
      const chatLoads = mockClient.getChat.mock.calls.length;
      mockSetChatProvider.mockResolvedValue({ ...mockChatWithMessages, provider: 'claude_code' });
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ selected: selection(claudeOffer, null) })
      );
      mockClient.getChat.mockResolvedValue({ ...mockChatWithMessages, provider: 'claude_code' });

      chooseSelectOption('chat-provider', 'Claude Code');

      await waitFor(() =>
        expect(mockSetChatProvider).toHaveBeenCalledWith('chat-1', {
          provider: 'claude_code',
          model: null,
        })
      );
      await waitFor(() => {
        expect(mockGetChatProviders.mock.calls.length).toBeGreaterThan(providerLoads);
        expect(mockClient.getChat.mock.calls.length).toBeGreaterThan(chatLoads);
      });
      await waitFor(() => expect(badge()).toHaveTextContent('Claude Code·Automatic'));
      expect(mockClient.updateChat).not.toHaveBeenCalled();
    });

    const openChatById = async () => {
      mockClient.getChat.mockImplementation(async (id: string) => ({
        ...mockChatWithMessages,
        id,
      }));
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-provider');
    };

    const switchToChat2 = async () => {
      fireEvent.click(screen.getByText('Chat 2'));
      await waitFor(() => expect(mockClient.getChat.mock.calls.at(-1)?.[0]).toBe('chat-2'));
      await screen.findByTestId('chat-provider');
    };

    it('does not carry a refusal over to the chat opened next', async () => {
      await openChatById();
      mockSetChatProvider.mockRejectedValue(new Error('Codex is not signed in.'));
      chooseSelectOption('chat-provider', 'Codex');
      await screen.findByText('Codex is not signed in.');

      await switchToChat2();

      expect(screen.queryByText('Codex is not signed in.')).not.toBeInTheDocument();
    });

    it('does not show a late refusal on the chat opened since', async () => {
      await openChatById();
      let refuse = (_error: Error) => {};
      mockSetChatProvider.mockReturnValue(
        new Promise((_resolve, reject) => {
          refuse = reject;
        })
      );
      chooseSelectOption('chat-provider', 'Codex');
      await switchToChat2();

      await act(async () => {
        refuse(new Error('Codex is not signed in.'));
      });

      expect(screen.queryByText('Codex is not signed in.')).not.toBeInTheDocument();
    });

    it('leaves the chat opened since alone when an earlier change lands', async () => {
      await openChatById();
      let save = (_chat: ChatWithMessages) => {};
      mockSetChatProvider.mockReturnValue(
        new Promise<ChatWithMessages>((resolve) => {
          save = resolve;
        })
      );
      chooseSelectOption('chat-provider', 'Codex');
      await switchToChat2();
      const reads = mockClient.getChat.mock.calls.length;

      await act(async () => {
        save({ ...mockChatWithMessages, provider: 'codex' });
      });

      expect(mockClient.getChat.mock.calls.length).toBe(reads);
      expect(mockClient.updateChat).not.toHaveBeenCalled();
      expect(screen.getByTestId('chat-provider')).toBeInTheDocument();
    });

    it('resets the effort of the chat left behind without disturbing the one opened', async () => {
      mockClient.getChat.mockImplementation(async (id: string) => ({
        ...mockChatWithMessages,
        id,
        reasoning_effort: 'ultra',
      }));
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-provider');
      let settle = (_chat: ChatWithMessages) => {};
      mockSetChatProvider.mockResolvedValue({ ...mockChatWithMessages, provider: 'openai' });
      mockClient.updateChat.mockReturnValue(
        new Promise<ChatWithMessages>((resolve) => {
          settle = resolve;
        })
      );

      chooseSelectOption('chat-provider', 'Self-Hosted');
      await waitFor(() =>
        expect(mockClient.updateChat).toHaveBeenCalledWith('chat-1', { reasoning_effort: 'auto' })
      );
      await switchToChat2();
      const reads = mockClient.getChat.mock.calls.length;

      await act(async () => {
        settle({ ...mockChatWithMessages, reasoning_effort: 'auto' });
      });

      expect(mockClient.getChat.mock.calls.length).toBe(reads);
      expect(screen.getByTestId('chat-provider')).toBeInTheDocument();
    });

    it('does not show a late effort reset failure on the chat opened since', async () => {
      mockClient.getChat.mockImplementation(async (id: string) => ({
        ...mockChatWithMessages,
        id,
        reasoning_effort: 'ultra',
      }));
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-provider');
      let refuse = (_error: Error) => {};
      mockSetChatProvider.mockResolvedValue({ ...mockChatWithMessages, provider: 'openai' });
      mockClient.updateChat.mockReturnValue(
        new Promise((_resolve, reject) => {
          refuse = reject;
        })
      );

      chooseSelectOption('chat-provider', 'Self-Hosted');
      await waitFor(() => expect(mockClient.updateChat).toHaveBeenCalled());
      await switchToChat2();

      await act(async () => {
        refuse(new Error('nope'));
      });

      expect(screen.queryByTestId('chat-operation-error')).not.toBeInTheDocument();
    });

    it('does not show a late toggle failure on the chat opened since', async () => {
      mockClient.getChat.mockImplementation(async (id: string) => ({
        ...mockChatWithMessages,
        id,
        tools: true,
      }));
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      const toggle = await screen.findByTestId('agent-toggle');
      let refuse = (_error: Error) => {};
      mockClient.updateChat.mockReturnValue(
        new Promise((_resolve, reject) => {
          refuse = reject;
        })
      );
      fireEvent.click(toggle);
      await switchToChat2();

      await act(async () => {
        refuse(new Error('Failed on the old chat'));
      });

      expect(screen.queryByText('Failed on the old chat')).not.toBeInTheDocument();
    });

    it('still shows a toggle failure on the chat it happened to', async () => {
      mockClient.getChat.mockResolvedValue({ ...mockChatWithMessages, tools: true });
      mockClient.updateChat.mockRejectedValue(new Error('Failed on this chat'));
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      fireEvent.click(await screen.findByTestId('agent-toggle'));

      const error = await screen.findByText('Failed on this chat');
      expect(error.closest('.chat-header')).not.toBeNull();
    });

    it('shows a failure on another chat in the sidebar, not the open chat', async () => {
      await openChatById();
      mockClient.archiveChat.mockRejectedValue(new Error('Archive failed'));

      await chooseMenuItem('Actions for Chat 2', 'Archive');

      const error = await screen.findByText('Archive failed');
      expect(error.closest('.chat-header')).toBeNull();
      expect(error.closest('.chats-error')).not.toBeNull();
    });

    it('keeps the picker of a chat opened during another chat change usable', async () => {
      await openChatById();
      mockSetChatProvider.mockReturnValue(new Promise(() => {}));
      chooseSelectOption('chat-provider', 'Codex');
      await waitFor(() => expect(screen.getByTestId('chat-provider')).toBeDisabled());

      await switchToChat2();

      expect(screen.getByTestId('chat-provider')).toBeEnabled();
    });

    const heldSave = () => {
      let save = (_chat: ChatWithMessages) => {};
      mockSetChatProvider.mockReturnValueOnce(
        new Promise<ChatWithMessages>((resolve) => {
          save = resolve;
        })
      );
      return (chat: ChatWithMessages) => save(chat);
    };

    it('keeps a chat busy when an earlier change on another chat lands', async () => {
      await openChatById();
      const landOnChat1 = heldSave();
      chooseSelectOption('chat-provider', 'Codex');
      await switchToChat2();
      heldSave();
      chooseSelectOption('chat-provider', 'Codex');
      await waitFor(() => expect(screen.getByTestId('chat-provider')).toBeDisabled());

      await act(async () => {
        landOnChat1({ ...mockChatWithMessages, provider: 'codex' });
      });

      expect(screen.getByTestId('chat-provider')).toBeDisabled();
    });

    it('still shows a chat as busy after another chat started a change', async () => {
      await openChatById();
      heldSave();
      chooseSelectOption('chat-provider', 'Codex');
      await switchToChat2();
      heldSave();
      chooseSelectOption('chat-provider', 'Codex');
      await waitFor(() => expect(screen.getByTestId('chat-provider')).toBeDisabled());

      fireEvent.click(screen.getAllByText('Chat 1')[0]);
      await waitFor(() => expect(mockClient.getChat.mock.calls.at(-1)?.[0]).toBe('chat-1'));
      await screen.findByTestId('chat-provider');

      expect(screen.getByTestId('chat-provider')).toBeDisabled();
    });

    it('does not clear the error of a chat opened while a dropped card was read', async () => {
      mockClient.getChat.mockImplementation(async (id: string) => ({
        ...mockChatWithMessages,
        id,
        tools: true,
        needs_character: true,
      }));
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-provider');
      let read = (_text: string) => {};
      const card = new File(['{}'], 'card.json', { type: 'application/json' });
      card.text = () =>
        new Promise<string>((resolve) => {
          read = resolve;
        });
      fireEvent.drop(document.querySelector('.message-form') as Element, {
        dataTransfer: { files: [card], types: ['Files'] },
      });
      await switchToChat2();
      mockClient.updateChat.mockResolvedValue(mockChatWithMessages);
      mockClient.updateChat.mockRejectedValueOnce(new Error('Failed on chat 2'));
      fireEvent.click(await screen.findByTestId('agent-toggle'));
      await screen.findByText('Failed on chat 2');

      await act(async () => {
        read(JSON.stringify({ name: 'Ada', description: 'A card' }));
      });

      expect(screen.getByText('Failed on chat 2')).toBeInTheDocument();
    });

    it('warns during a turn while reading the providers is still pending', async () => {
      mockGetChatProviders.mockResolvedValue(chatProviders({ cli: false }));
      mockClient.getChat.mockResolvedValue({
        ...mockChatWithMessages,
        agent_enabled: true,
        agent_sandboxed: false,
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      await screen.findByTestId('chat-provider');
      await waitFor(() =>
        expect(screen.queryByTestId('agent-sandbox-toggle')).not.toBeInTheDocument()
      );
      mockGetChatProviders.mockReturnValue(new Promise(() => {}));

      fireEvent.change(screen.getByPlaceholderText('Type a message, or drop a file...'), {
        target: { value: 'Next turn' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));

      await screen.findByTestId('agent-sandbox-toggle');
      expect(screen.getByTestId('agent-permissions')).toHaveTextContent('Host tools');
    });

    it('reads a chat opened during a turn only once', async () => {
      await openChatById();
      fireEvent.change(screen.getByPlaceholderText('Type a message, or drop a file...'), {
        target: { value: 'Next turn' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));
      await waitFor(() => expect(mockWsSend).toHaveBeenCalled());

      await switchToChat2();
      await new Promise((resolve) => setTimeout(resolve, 20));

      const reads = mockGetChatProviders.mock.calls.filter(([id]) => id === 'chat-2');
      expect(reads).toHaveLength(1);
    });

    it('shows a saved choice in the picker when reading it back fails', async () => {
      await openChatById();
      mockSetChatProvider.mockResolvedValue({
        ...mockChatWithMessages,
        provider: 'codex',
        provider_model: 'gpt-5.5',
      });
      mockGetChatProviders.mockRejectedValue(new Error('Failed to load AI providers: 502'));

      chooseSelectOption('chat-provider', 'Codex');

      await screen.findByTestId('chat-providers-error');
      await waitFor(() => expect(screen.getByTestId('chat-provider')).toHaveTextContent('Codex'));
      expect(document.querySelector('.chat-settings-provider')).toHaveTextContent('Codex');
      expect(document.querySelector('.chat-model')).toHaveTextContent('gpt-5.5');
    });

    it('shows a refused choice and keeps the chat as it was', async () => {
      await openChat();
      await screen.findByTestId('chat-provider');
      mockSetChatProvider.mockRejectedValue(new Error('Codex is not signed in.'));

      chooseSelectOption('chat-provider', 'Codex');

      const refusal = await screen.findByText('Codex is not signed in.');
      expect(refusal.closest('.chat-header')).not.toBeNull();
      expect(screen.getByTestId('chat-provider')).toHaveTextContent('Default');
      expect(screen.getByTestId('chat-provider')).not.toHaveTextContent('Codex');
    });

    it('hides the context size for a chosen provider', async () => {
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ selected: selection(openAiOffer, 'gpt-4o') })
      );
      await openChat({ context_tokens: 32768, provider: 'openai', provider_model: 'gpt-4o' });
      await waitFor(() => expect(screen.getByTestId('chat-provider')).toHaveTextContent('OpenAI'));
      expect(screen.queryByTestId('context-tokens')).not.toBeInTheDocument();
      expect(badge()).toHaveTextContent('OpenAI·gpt-4o');
      expect(optionNames('reasoning-effort')).toEqual(['Auto', 'Off', 'Low', 'Medium', 'High']);
    });

    it('hides the context size for an agent default', async () => {
      mockGetChatProviders.mockResolvedValue(chatProviders({ default: claudeOffer }));
      await openChat({ context_tokens: 32768 });
      await screen.findByTestId('chat-provider-model');
      expect(screen.getByTestId('chat-provider')).toHaveTextContent('Default');
      expect(screen.queryByTestId('context-tokens')).not.toBeInTheDocument();
    });

    it('shows the agent toggle from an agent offer', async () => {
      mockGetChatProviders.mockResolvedValue(chatProviders({ default: claudeOffer }));
      await openChat({ model_name: 'llama2' });
      expect(await screen.findByTestId('agent-toggle')).toBeInTheDocument();
    });

    it('ignores the local model profile for a chosen endpoint provider', async () => {
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ selected: selection(openAiOffer, 'gpt-4o') })
      );
      await openChat({ model_name: 'llama3.1', provider: 'openai', provider_model: 'gpt-4o' });
      await waitFor(() => expect(screen.getByTestId('chat-provider')).toHaveTextContent('OpenAI'));
      expect(screen.queryByTestId('agent-toggle')).not.toBeInTheDocument();
    });

    it('offers agent models and efforts on an agent default and saves a model change', async () => {
      mockGetChatProviders.mockResolvedValue(chatProviders({ default: claudeOffer }));
      await openChat({ model_name: 'auto', reasoning_effort: 'auto' });
      await screen.findByTestId('chat-provider-model');
      expect(optionNames('chat-provider-model')).toEqual(['Automatic', 'Sonnet', 'Haiku']);
      expect(optionNames('reasoning-effort')).toEqual([
        'Auto',
        'Low',
        'Medium',
        'High',
        'Extra high',
        'Max',
      ]);
      const providerLoads = mockGetChatProviders.mock.calls.length;
      const chatLoads = mockClient.getChat.mock.calls.length;
      mockClient.updateChat.mockResolvedValue({ ...mockChatWithMessages, model_name: 'sonnet' });

      chooseSelectOption('chat-provider-model', 'Sonnet');

      await waitFor(() =>
        expect(mockClient.updateChat).toHaveBeenCalledWith('chat-1', { model_name: 'sonnet' })
      );
      await waitFor(() => {
        expect(mockGetChatProviders.mock.calls.length).toBeGreaterThan(providerLoads);
        expect(mockClient.getChat.mock.calls.length).toBeGreaterThan(chatLoads);
      });
      expect(mockClient.updateChat).toHaveBeenCalledTimes(1);
    });

    it('hides reasoning for an agent model without efforts', async () => {
      mockGetChatProviders.mockResolvedValue(chatProviders({ default: claudeOffer }));
      await openChat({ model_name: 'haiku', reasoning: true });
      await screen.findByTestId('chat-provider-model');
      expect(screen.queryByTestId('reasoning-effort')).not.toBeInTheDocument();
    });

    it('resets an effort the new codex model does not support', async () => {
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ selected: selection(codexOffer, 'gpt-6-sol') })
      );
      await openChat({ provider: 'codex', provider_model: 'gpt-6-sol', reasoning_effort: 'max' });
      await screen.findByTestId('chat-provider-model');
      expect(optionNames('reasoning-effort')).toContain('Ultra');
      expect(badge()).toHaveTextContent('Codex·gpt-6-sol');
      mockSetChatProvider.mockResolvedValue({ ...mockChatWithMessages, provider: 'codex' });
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ selected: selection(codexOffer, 'gpt-5.5') })
      );
      mockClient.updateChat.mockResolvedValue({
        ...mockChatWithMessages,
        reasoning_effort: 'auto',
      });

      chooseSelectOption('chat-provider-model', 'gpt-5.5');

      await waitFor(() =>
        expect(mockSetChatProvider).toHaveBeenCalledWith('chat-1', {
          provider: 'codex',
          model: 'gpt-5.5',
        })
      );
      await waitFor(() =>
        expect(mockClient.updateChat).toHaveBeenCalledWith('chat-1', { reasoning_effort: 'auto' })
      );
      await waitFor(() => expect(badge()).toHaveTextContent('Codex·gpt-5.5'));
      expect(optionNames('reasoning-effort')).not.toContain('Ultra');
    });

    it('keeps an effort the new codex model supports', async () => {
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ selected: selection(codexOffer, 'gpt-6-sol') })
      );
      await openChat({ provider: 'codex', provider_model: 'gpt-6-sol', reasoning_effort: 'high' });
      await screen.findByTestId('chat-provider-model');
      mockSetChatProvider.mockResolvedValue({ ...mockChatWithMessages, provider: 'codex' });
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ selected: selection(codexOffer, 'gpt-5.5') })
      );

      chooseSelectOption('chat-provider-model', 'gpt-5.5');

      await waitFor(() => expect(badge()).toHaveTextContent('Codex·gpt-5.5'));
      expect(mockClient.updateChat).not.toHaveBeenCalled();
    });

    it('shows why a chosen provider cannot be used', async () => {
      const reason =
        'Codex is not signed in for this organization. An organization admin can sign in under Organization Settings > AI Settings.';
      mockGetChatProviders.mockResolvedValue(
        chatProviders({
          providers: [selfHostedOffer, openAiOffer, claudeOffer],
          selected: selection(codexOffer, null, { usable: false, reason }),
        })
      );
      await openChat({ provider: 'codex' });
      expect(await screen.findByTestId('chat-provider-error')).toHaveTextContent(reason);
      expect(screen.getByTestId('chat-provider-error')).toHaveAttribute('role', 'alert');
      expect(screen.getByTestId('chat-provider')).toHaveTextContent('Codex (unavailable)');
    });

    it("offers the unusable chosen provider's efforts rather than the default's", async () => {
      mockGetChatProviders.mockResolvedValue(
        chatProviders({
          selected: selection(codexOffer, 'gpt-6-sol', { usable: false, reason: 'Signed out.' }),
        })
      );
      await openChat({ provider: 'codex', provider_model: 'gpt-6-sol', reasoning_effort: 'ultra' });
      await screen.findByTestId('chat-provider-error');
      expect(optionNames('reasoning-effort')).toEqual([
        'Auto',
        'Low',
        'Medium',
        'High',
        'Extra high',
        'Max',
        'Ultra',
      ]);
    });

    it('hides the picker and says why when the providers fail to load', async () => {
      mockGetChatProviders.mockRejectedValue(new Error('Failed to load AI providers: 502'));
      await openChat();
      expect(await screen.findByTestId('chat-providers-error')).toHaveTextContent(
        'Failed to load AI providers: 502'
      );
      expect(screen.getByTestId('chat-providers-error')).toHaveAttribute('role', 'alert');
      expect(screen.queryByTestId('chat-provider')).not.toBeInTheDocument();
      expect(badge()).toHaveTextContent('llama2');
    });

    it('keeps the picker and says why when the providers fail to reload', async () => {
      await openChat();
      await screen.findByTestId('chat-provider');
      mockSetChatProvider.mockResolvedValue({ ...mockChatWithMessages, provider: 'claude_code' });
      mockGetChatProviders.mockRejectedValue(new Error('Failed to load AI providers: 502'));

      chooseSelectOption('chat-provider', 'Claude Code');

      expect(await screen.findByTestId('chat-providers-error')).toHaveTextContent(
        'Failed to load AI providers: 502'
      );
      expect(screen.getByTestId('chat-provider')).toBeInTheDocument();
    });

    it('follows the authoritative Default choice while the chat still names the old provider', async () => {
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ selected: selection(codexOffer, 'gpt-5.5') })
      );
      const stale = {
        context_tokens: 32768,
        provider: 'codex' as const,
        provider_model: 'gpt-5.5',
      };
      await openChat(stale);
      await screen.findByTestId('chat-provider-model');
      expect(screen.queryByTestId('context-tokens')).not.toBeInTheDocument();
      mockSetChatProvider.mockResolvedValue({ ...mockChatWithMessages, ...stale });
      mockGetChatProviders.mockResolvedValue(chatProviders());

      chooseSelectOption('chat-provider', 'Default (Self-Hosted)');

      await waitFor(() =>
        expect(mockSetChatProvider).toHaveBeenCalledWith('chat-1', { provider: null, model: null })
      );
      await waitFor(() => expect(screen.getByTestId('context-tokens')).toBeInTheDocument());
      expect(screen.queryByTestId('chat-provider-model')).not.toBeInTheDocument();
      expect(badge()).toHaveTextContent('llama2');
    });

    it('refreshes the chat and reports only the effort when its reset fails', async () => {
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ selected: selection(codexOffer, 'gpt-6-sol') })
      );
      await openChat({ provider: 'codex', provider_model: 'gpt-6-sol', reasoning_effort: 'max' });
      await screen.findByTestId('chat-provider-model');
      const chatLoads = mockClient.getChat.mock.calls.length;
      mockSetChatProvider.mockResolvedValue({ ...mockChatWithMessages, provider: 'codex' });
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ selected: selection(codexOffer, 'gpt-5.5') })
      );
      mockClient.updateChat.mockRejectedValue(new Error('Server unavailable'));

      chooseSelectOption('chat-provider-model', 'gpt-5.5');

      expect(
        await screen.findByText(
          'AI provider changed, but its reasoning effort could not be reset: Server unavailable'
        )
      ).toBeInTheDocument();
      expect(screen.queryByText(/Failed to change AI provider/)).not.toBeInTheDocument();
      await waitFor(() => expect(mockClient.getChat.mock.calls.length).toBeGreaterThan(chatLoads));
      await waitFor(() => expect(badge()).toHaveTextContent('Codex·gpt-5.5'));
    });

    it('refreshes after a saved model change and reports only the effort when its reset fails', async () => {
      mockGetChatProviders.mockResolvedValue(chatProviders({ default: codexOffer }));
      await openChat({ model_name: 'gpt-6-sol', reasoning_effort: 'max' });
      await screen.findByTestId('chat-provider-model');
      const providerLoads = mockGetChatProviders.mock.calls.length;
      const chatLoads = mockClient.getChat.mock.calls.length;
      mockClient.updateChat.mockImplementation(async (_id: string, update: UpdateChatRequest) => {
        if (update.reasoning_effort) throw new Error('Server unavailable');
        return { ...mockChatWithMessages, model_name: 'gpt-5.5' };
      });

      chooseSelectOption('chat-provider-model', 'gpt-5.5');

      expect(
        await screen.findByText(
          'Model changed, but its reasoning effort could not be reset: Server unavailable'
        )
      ).toBeInTheDocument();
      expect(screen.queryByText(/Failed to change model/)).not.toBeInTheDocument();
      expect(mockClient.updateChat).toHaveBeenCalledWith('chat-1', { model_name: 'gpt-5.5' });
      await waitFor(() => {
        expect(mockGetChatProviders.mock.calls.length).toBeGreaterThan(providerLoads);
        expect(mockClient.getChat.mock.calls.length).toBeGreaterThan(chatLoads);
      });
    });

    it('asks for a typed model name before choosing a provider with none saved', async () => {
      const openAi = providerOffer({
        provider: 'openai',
        label: 'OpenAI',
        requires_model: true,
      });
      mockGetChatProviders.mockResolvedValue(
        chatProviders({ providers: [selfHostedOffer, openAi] })
      );
      await openChat();
      await screen.findByTestId('chat-provider');

      chooseSelectOption('chat-provider', 'OpenAI');

      const field = screen.getByRole('textbox', { name: 'Provider model name' });
      expect(field).toHaveValue('');
      expect(mockSetChatProvider).not.toHaveBeenCalled();
      mockSetChatProvider.mockResolvedValue({ ...mockChatWithMessages, provider: 'openai' });

      fireEvent.change(field, { target: { value: 'my-deployment' } });
      fireEvent.keyDown(field, { key: 'Enter' });

      await waitFor(() =>
        expect(mockSetChatProvider).toHaveBeenCalledWith('chat-1', {
          provider: 'openai',
          model: 'my-deployment',
        })
      );
      expect(mockSetChatProvider).toHaveBeenCalledTimes(1);
    });
  });

  describe('attached sources', () => {
    const openChat = async () => {
      mockGetChats.mockResolvedValue(mockChats);
      mockGetChat.mockResolvedValue(mockChatWithMessages);
      window.history.pushState({}, '', '/?id=chat-1');
      renderChatsPage();
      await waitFor(() => {
        expect(screen.getByTestId('chat-sources')).toBeInTheDocument();
      });
    };

    it('keeps the attach button, the draft and Send on one composer row and Sources under it', async () => {
      await openChat();
      const row = screen.getByPlaceholderText('Type a message, or drop a file...').parentElement;
      expect(row).toHaveClass('message-form-row');
      expect(screen.getByRole('button', { name: 'Attach files' }).parentElement).toBe(row);
      expect(screen.getByRole('button', { name: 'Send' }).parentElement).toBe(row);
      const sources = screen.getByRole('button', { name: 'Sources: whole workspace' });
      expect(sources.closest('.message-form-row')).toBeNull();
      expect(sources.closest('.chat-composer-footer')).toBeTruthy();
    });

    it('offers a Sources chip in the composer that reads whole-workspace when nothing is attached', async () => {
      await openChat();
      expect(mockGetChatSources).toHaveBeenCalledWith('chat-1');
      const toggle = screen.getByRole('button', { name: 'Sources: whole workspace' });
      expect(toggle).toBeInTheDocument();
      expect(screen.queryAllByTestId('chat-source-chip')).toHaveLength(0);
    });

    it('attaches a workspace source for this chat and shows it as a chip', async () => {
      mockSetChatSources.mockResolvedValue([
        { id: 'source-repo', name: 'abnegate/zone-tests', source_type: 'github' },
      ]);
      await openChat();
      fireEvent.click(screen.getByRole('button', { name: 'Sources: whole workspace' }));
      fireEvent.click(screen.getByLabelText(/abnegate\/zone-tests/));
      await waitFor(() => {
        expect(mockSetChatSources).toHaveBeenCalledWith('chat-1', ['source-repo']);
      });
      await waitFor(() => {
        expect(screen.getByTestId('chat-source-chip')).toHaveTextContent('abnegate/zone-tests');
      });
      expect(screen.getByRole('button', { name: 'Sources: 1 attached' })).toBeInTheDocument();
    });

    it('shows the attachment the server already holds and detaches from the chip', async () => {
      mockGetChatSources.mockResolvedValue([
        { id: 'source-notes', name: 'Team notes', source_type: 'text' },
      ]);
      mockSetChatSources.mockResolvedValue([]);
      await openChat();
      await waitFor(() => {
        expect(screen.getByTestId('chat-source-chip')).toHaveTextContent('Team notes');
      });
      fireEvent.click(screen.getByRole('button', { name: 'Detach Team notes' }));
      await waitFor(() => {
        expect(mockSetChatSources).toHaveBeenCalledWith('chat-1', []);
      });
      await waitFor(() => {
        expect(screen.queryAllByTestId('chat-source-chip')).toHaveLength(0);
      });
    });
  });

  describe('chat search', () => {
    const mockSearchResults: ChatSearchResult[] = [
      {
        message_id: 'msg-search-1',
        chat_id: 'chat-1',
        chat_title: 'Chat 1',
        content: 'This is a test message about TypeScript',
        snippet: '...test message about TypeScript...',
        relevance_score: 0.95,
        created_at: '2024-01-01T00:00:00Z',
      },
      {
        message_id: 'msg-search-2',
        chat_id: 'chat-2',
        chat_title: 'Chat 2',
        content: 'Another TypeScript example',
        snippet: '...TypeScript example...',
        relevance_score: 0.85,
        created_at: '2024-01-02T00:00:00Z',
      },
    ];

    it('renders search input', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });
    });

    it('searches messages when form is submitted', async () => {
      mockClient.searchChatMessages.mockResolvedValueOnce({
        results: mockSearchResults,
        total: 2,
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'TypeScript' } });
      fireEvent.submit(searchInput.closest('form')!);

      await waitFor(() => {
        expect(mockClient.searchChatMessages).toHaveBeenCalledWith({
          query: 'TypeScript',
          workspace_id: 'ws-1',
          limit: 20,
        });
      });
    });

    it('displays search results', async () => {
      mockClient.searchChatMessages.mockResolvedValueOnce({
        results: mockSearchResults,
        total: 2,
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'TypeScript' } });
      fireEvent.submit(searchInput.closest('form')!);

      await waitFor(() => {
        expect(screen.getByTestId('search-results-list')).toBeInTheDocument();
      });

      expect(screen.getByText('Chat 1')).toBeInTheDocument();
      expect(screen.getByText('...test message about TypeScript...')).toBeInTheDocument();
      expect(screen.queryByText('95%')).not.toBeInTheDocument();
    });

    it('shows a snippet as prose, without the markdown marks or collapsed line breaks', async () => {
      mockClient.searchChatMessages.mockResolvedValueOnce({
        results: [
          {
            ...mockSearchResults[0],
            snippet:
              'Here are the documents in the wiki: ⏎ - **Deployment checklist mua9iw0u1qg** — knowledge base note',
          },
        ],
        total: 1,
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'deployment' } });
      fireEvent.submit(searchInput.closest('form')!);

      await waitFor(() => {
        expect(screen.getByTestId('search-results-list')).toBeInTheDocument();
      });

      expect(
        screen.getByText(
          'Here are the documents in the wiki: Deployment checklist mua9iw0u1qg — knowledge base note'
        )
      ).toBeInTheDocument();
      expect(screen.queryByText(/\*\*|⏎/)).not.toBeInTheDocument();
    });

    it('names a result after the loaded chat when the server sent no title', async () => {
      mockClient.searchChatMessages.mockResolvedValueOnce({
        results: [
          { ...mockSearchResults[0], chat_title: '', chat_id: 'chat-2' },
          { ...mockSearchResults[1], chat_title: '', chat_id: 'chat-gone' },
        ],
        total: 2,
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'TypeScript' } });
      fireEvent.submit(searchInput.closest('form')!);

      const titles = await waitFor(() => {
        const list = screen.getByTestId('search-results-list');
        return [...list.querySelectorAll('.search-result-chat')].map((node) => node.textContent);
      });
      expect(titles).toEqual(['Chat 2', 'Untitled chat']);
      expect(screen.queryByText('Chat', { selector: '.search-result-chat' })).toBeNull();
    });

    it('shows no results message when search returns empty', async () => {
      mockClient.searchChatMessages.mockResolvedValueOnce({
        results: [],
        total: 0,
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'nonexistent' } });
      fireEvent.submit(searchInput.closest('form')!);

      await waitFor(() => {
        expect(screen.getByText('No messages match')).toBeInTheDocument();
      });
      const empty = screen.getByText('No messages match').closest('.ui-empty');
      expect(empty).not.toBeNull();
      expect(empty).toHaveTextContent('Try another search');
      expect(screen.queryByTestId('search-results-list')).not.toBeInTheDocument();

      fireEvent.click(empty?.querySelector('.ui-empty-action button') as HTMLButtonElement);

      await waitFor(() => {
        expect(screen.queryByText('No messages match')).not.toBeInTheDocument();
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });
      expect(searchInput).toHaveValue('');
    });

    it('shows searching state while searching', async () => {
      mockClient.searchChatMessages.mockImplementation(() => new Promise(() => {}));

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'test' } });
      fireEvent.submit(searchInput.closest('form')!);

      await waitFor(() => {
        expect(screen.getByText('Searching...')).toBeInTheDocument();
      });
    });

    it('navigates to chat when search result is clicked', async () => {
      mockClient.searchChatMessages.mockResolvedValueOnce({
        results: mockSearchResults,
        total: 2,
      });
      mockClient.getChat.mockResolvedValueOnce(mockChatWithMessages);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'TypeScript' } });
      fireEvent.submit(searchInput.closest('form')!);

      await waitFor(() => {
        expect(screen.getByTestId('search-results-list')).toBeInTheDocument();
      });

      const searchResults = screen.getAllByTestId('search-result-item');
      fireEvent.click(searchResults[0]);

      await waitFor(() => {
        expect(mockClient.getChat).toHaveBeenCalledWith('chat-1');
      });
    });

    it('clears search when clear button is clicked', async () => {
      mockClient.searchChatMessages.mockResolvedValueOnce({
        results: mockSearchResults,
        total: 2,
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });

      const searchInput = screen.getByTestId('chat-search-input') as HTMLInputElement;
      fireEvent.change(searchInput, { target: { value: 'TypeScript' } });
      fireEvent.submit(searchInput.closest('form')!);

      await waitFor(() => {
        expect(screen.getByTestId('clear-search-btn')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByTestId('clear-search-btn'));

      await waitFor(() => {
        expect(searchInput.value).toBe('');
        expect(screen.queryByTestId('search-results-list')).not.toBeInTheDocument();
      });
    });

    it('shows clear button only when search query exists', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });

      expect(screen.queryByTestId('clear-search-btn')).not.toBeInTheDocument();

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'test' } });

      expect(screen.getByTestId('clear-search-btn')).toBeInTheDocument();
    });

    it('keeps the filter tabs mounted but disabled while showing search results', async () => {
      mockClient.searchChatMessages.mockResolvedValueOnce({
        results: mockSearchResults,
        total: 2,
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Active' })).toBeEnabled();
        expect(screen.getByRole('tab', { name: 'Archived' })).toBeEnabled();
      });

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'TypeScript' } });
      fireEvent.submit(searchInput.closest('form')!);

      await waitFor(() => {
        expect(screen.getByTestId('search-results-list')).toBeInTheDocument();
      });

      expect(screen.getByRole('tab', { name: 'Active' })).toBeDisabled();
      expect(screen.getByRole('tab', { name: 'Archived' })).toBeDisabled();

      fireEvent.click(screen.getByTestId('clear-search-btn'));

      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Active' })).toBeEnabled();
      });
    });

    it('does not search with empty query', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: '   ' } });
      fireEvent.submit(searchInput.closest('form')!);

      expect(mockClient.searchChatMessages).not.toHaveBeenCalled();
    });

    it('shows error when search fails', async () => {
      mockClient.searchChatMessages.mockRejectedValueOnce(new Error('Search failed'));

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'test' } });
      fireEvent.submit(searchInput.closest('form')!);

      await waitFor(() => {
        expect(screen.getByText('Search failed')).toBeInTheDocument();
      });
    });

    it('navigates result via keyboard Enter', async () => {
      mockClient.searchChatMessages.mockResolvedValueOnce({
        results: mockSearchResults,
        total: 2,
      });
      mockClient.getChat.mockResolvedValueOnce(mockChatWithMessages);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByTestId('chat-search-input')).toBeInTheDocument();
      });

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'TypeScript' } });
      fireEvent.submit(searchInput.closest('form')!);

      await waitFor(() => {
        expect(screen.getByTestId('search-results-list')).toBeInTheDocument();
      });

      const searchResults = screen.getAllByTestId('search-result-item');
      fireEvent.keyDown(searchResults[0], { key: 'Enter' });

      await waitFor(() => {
        expect(mockClient.getChat).toHaveBeenCalledWith('chat-1');
      });
    });
  });

  describe('message links', () => {
    const linkedChat = (overrides: Partial<Message> = {}): ChatWithMessages => ({
      ...mockChatWithMessages,
      messages: [
        {
          id: 'msg-link',
          chat_id: 'chat-1',
          role: 'assistant',
          content: 'Confirmed in [the audit](https://audit.example/x)',
          created_at: '2024-01-01T00:00:00Z',
          metadata: {
            citations: [
              {
                kind: 'github_file',
                title: 'an unrelated source',
                url: 'https://sourced.example/other',
                observed_at: '2024-01-01T00:00:00Z',
                complete: true,
                outcome: 'success',
                provenance: 'server_execution',
              },
            ],
          },
          ...overrides,
        },
      ],
    });

    it('keeps a link the user wrote themselves clickable', async () => {
      mockGetChat.mockResolvedValue(
        linkedChat({ role: 'user', content: 'See [the runbook](https://runbook.example/x)' })
      );
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));

      expect(await screen.findByRole('link', { name: 'the runbook' })).toHaveAttribute(
        'href',
        'https://runbook.example/x'
      );
    });

    it('renders an assistant link that resolves to no citation inert', async () => {
      mockGetChat.mockResolvedValue(linkedChat());
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));

      expect(await screen.findByText(/Confirmed in/)).toBeInTheDocument();
      expect(screen.queryByRole('link', { name: 'the audit' })).not.toBeInTheDocument();
    });
  });

  describe('action receipts', () => {
    const receiptChat: ChatWithMessages = {
      ...mockChatWithMessages,
      messages: [
        mockChatWithMessages.messages[0],
        {
          ...mockChatWithMessages.messages[1],
          metadata: {
            action_receipts: [
              {
                id: 'call_1',
                action: 'create_task',
                target_type: 'task',
                target_id: 'task-1',
                target_label: 'Ship the billing export',
                actor_id: 'user-1',
                actor_name: 'Alice',
                occurred_at: '2026-09-05T10:47:00.000Z',
                success: true,
                outcome: 'Task created',
                href: '/tasks?id=task-1',
              },
            ],
          },
        },
      ],
    };

    it('renders a stored receipt with a link to the changed item', async () => {
      mockGetChat.mockResolvedValue(receiptChat);
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));

      expect(await screen.findByTestId('action-receipt')).toBeInTheDocument();
      expect(screen.getByText('Created task')).toBeInTheDocument();
      expect(screen.getByText('Ship the billing export')).toBeInTheDocument();
      expect(screen.getByText('Alice')).toBeInTheDocument();
      expect(screen.getByText('Task created')).toBeInTheDocument();
      expect(screen.getByTestId('action-receipt-link')).toHaveAttribute('href', '/tasks?id=task-1');
    });

    it('selects a chat from the page URL', async () => {
      mockGetChat.mockResolvedValue(receiptChat);
      window.history.pushState({}, '', '/chats?id=chat-1');
      renderChatsPage();

      expect(await screen.findByText('Hi there!')).toBeInTheDocument();
      expect(mockGetChat).toHaveBeenCalledWith('chat-1');
    });
  });

  describe('account handover', () => {
    const resetsAt = () => new Date(Date.now() + (2 * 60 + 10) * 60_000).toISOString();
    const NOTICE =
      'Switched to Codex · b@example.com — a@example.com reached its usage limit; resets in 2h 10m';

    it('draws the divider inside the answer where the new account took over', async () => {
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      const input = await screen.findByPlaceholderText<HTMLTextAreaElement>(
        'Type a message, or drop a file...'
      );
      fireEvent.change(input, { target: { value: 'Explain the deploy' } });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));
      await waitFor(() => expect(input.value).toBe(''));

      act(() => {
        socket.emit({ type: 'message_start', message_id: 'moved', role: 'assistant' });
        socket.emit({ type: 'chunk', content: 'Before the switch. ', index: 0 });
        socket.emit({ type: 'status', message: 'Switching to b@example.com…' });
        socket.emit({
          type: 'handover',
          message_id: 'moved',
          from: 'a@example.com',
          to: 'b@example.com',
          from_agent: 'claude',
          agent: 'codex',
          reason: 'limit',
          resets_at: resetsAt(),
          carried: false,
          at: Array.from('Before the switch. ').length,
        });
        socket.emit({ type: 'chunk', content: 'After the switch.', index: 1 });
      });

      const note = await screen.findByRole('note');
      expect(note).toHaveTextContent(NOTICE);
      expect(screen.getByRole('status')).toHaveTextContent('Switching to b@example.com…');
      const answer = note.closest('.message-content');
      expect(answer?.closest('.message-assistant')).not.toBeNull();
      await waitFor(() =>
        expect(answer?.textContent).toMatch(
          /^Before the switch\..*Switched to.*After the switch\.$/
        )
      );

      act(() =>
        socket.emit({
          type: 'message_end',
          message_id: 'moved',
          content: 'Before the switch. After the switch.',
        })
      );
      await waitFor(() => expect(screen.queryByRole('status')).not.toBeInTheDocument());
      expect(screen.getByRole('note')).toHaveTextContent(NOTICE);
    });

    it('rebuilds the divider from the stored reply on reload', async () => {
      mockGetChat.mockResolvedValue({
        ...mockChatWithMessages,
        messages: [
          mockChatWithMessages.messages[0],
          {
            ...mockChatWithMessages.messages[1],
            content: 'Started here. Finished there.',
            metadata: {
              handovers: [
                {
                  kind: 'handover',
                  from: 'a@example.com',
                  to: 'b@example.com',
                  from_agent: 'claude',
                  agent: 'codex',
                  reason: 'limit',
                  resets_at: resetsAt(),
                  carried: false,
                  at: 14,
                },
              ],
            },
          },
        ],
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));

      const note = await screen.findByRole('note');
      expect(note).toHaveTextContent(NOTICE);
      const answer = note.closest('.message-content');
      expect(answer?.firstElementChild?.textContent).toBe('Started here.');
      expect(answer?.lastElementChild?.textContent).toBe('Finished there.');
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
    });

    it('leaves the agent out of a divider whose replay stayed on the same agent', async () => {
      mockGetChat.mockResolvedValue({
        ...mockChatWithMessages,
        messages: [
          mockChatWithMessages.messages[0],
          {
            ...mockChatWithMessages.messages[1],
            content: 'Started here. Finished there.',
            metadata: {
              handovers: [
                {
                  kind: 'handover',
                  from: 'a@example.com',
                  to: 'b@example.com',
                  from_agent: 'claude',
                  agent: 'claude',
                  reason: 'limit',
                  resets_at: resetsAt(),
                  carried: false,
                  at: 14,
                },
              ],
            },
          },
        ],
      });
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));

      const note = await screen.findByRole('note');
      expect(note).toHaveTextContent(
        'Switched to b@example.com — a@example.com reached its usage limit; resets in 2h 10m'
      );
      expect(note).not.toHaveTextContent('Claude');
    });

    it('draws no divider on a reply that stayed on one account', async () => {
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));

      expect(await screen.findByText('Hi there!')).toBeInTheDocument();
      expect(screen.queryByRole('note')).not.toBeInTheDocument();
    });
  });

  describe('grouping and sorting', () => {
    it('groups chats by project and sorts titles inside each group', async () => {
      mockClient.getChats.mockResolvedValue([
        { ...mockChats[0], id: 'loose', title: 'Loose' },
        { ...mockChats[1], id: 'zeta', title: 'Zeta chat', project_id: 'proj-zeta' },
        {
          ...mockChats[2],
          id: 'alpha-z',
          title: 'Zed',
          project_id: 'proj-alpha',
          updated_at: getDateString(0),
        },
        {
          ...mockChats[3],
          id: 'alpha-a',
          title: 'Able',
          project_id: 'proj-alpha',
          updated_at: getDateString(1),
        },
        { ...mockChats[0], id: 'gone', title: 'Gone', project_id: 'missing' },
      ]);

      renderChatsPage();
      await screen.findByRole('combobox', { name: 'Group chats' });
      chooseSelect('Group chats', 'Project');
      chooseSelect('Sort chats', 'Title A-Z');

      const headers = screen.getAllByRole('heading', { level: 2 }).map((node) => node.textContent);
      expect(headers).toEqual(['Alpha', 'Zeta', 'Unknown project', 'No project']);
      const titles = [...document.querySelectorAll('.chat-title')].map((node) => node.textContent);
      expect(titles).toEqual(['Able', 'Zed', 'Zeta chat', 'Gone', 'Loose']);
      expect(localStorage.getItem(CHAT_GROUP_STORAGE_KEY)).toBe('project');
      expect(localStorage.getItem(CHAT_SORT_STORAGE_KEY)).toBe('title_asc');
    });

    it('groups chats by the local calendar day of last update', async () => {
      const today = new Date();
      const atNoon = (daysAgo: number) => {
        const date = new Date(today.getFullYear(), today.getMonth(), today.getDate() - daysAgo, 12);
        return date.toISOString();
      };
      mockClient.getChats.mockResolvedValue([
        { ...mockChats[0], id: 'today', title: 'Today chat', updated_at: atNoon(0) },
        { ...mockChats[1], id: 'yesterday', title: 'Yesterday chat', updated_at: atNoon(1) },
      ]);

      renderChatsPage();
      await screen.findByRole('combobox', { name: 'Group chats' });
      chooseSelect('Group chats', 'Date');

      expect(screen.getByRole('heading', { name: 'Today' })).toBeInTheDocument();
      expect(screen.getByRole('heading', { name: 'Yesterday' })).toBeInTheDocument();
    });

    it('restores grouping from localStorage and hides it while searching', async () => {
      localStorage.setItem(CHAT_GROUP_STORAGE_KEY, 'project');
      mockClient.searchChatMessages.mockResolvedValue({ results: [], total: 0 });

      renderChatsPage();
      expect(await screen.findByRole('heading', { name: 'No project' })).toBeInTheDocument();

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'TypeScript' } });
      fireEvent.submit(searchInput.closest('form')!);

      await waitFor(() => {
        expect(screen.queryByRole('combobox', { name: 'Group chats' })).not.toBeInTheDocument();
      });
      expect(screen.queryByRole('heading', { name: 'No project' })).not.toBeInTheDocument();
    });

    it('hides arrange controls when there are no chats', async () => {
      mockClient.getChats.mockResolvedValue([]);
      renderChatsPage();
      expect(await screen.findByText('No chats yet')).toBeInTheDocument();
      expect(screen.queryByRole('combobox', { name: 'Group chats' })).not.toBeInTheDocument();
    });
  });
});
