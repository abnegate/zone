import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import type { Chat, ChatSearchResult, ChatWithMessages, Message } from '../types';

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
    setGetAccessToken: mock(),
    getMessages: mock(),
    updateChat: mockUpdateChat,
    deleteMessage: mock(),
    chatAccessToken: () => 'test-token',
    createChatWebSocket: () => {
      socket = new FakeSocket();
      return socket;
    },
  },
}));

// Mock useModels - include all exports from models module for proper mocking
mock.module('../../models', () => ({
  useModels: () => ({
    models: [
      { name: 'llama2', size: 1, modified_at: '' },
      { name: 'mistral', size: 1, modified_at: '' },
    ],
    loading: false,
    error: null,
    refresh: mock(),
    deleteModel: mock(),
  }),
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
    agent_sandboxed: true,
    created_at: '2024-01-01T00:00:00Z',
  },
  {
    id: 'chat-2',
    title: 'Chat 2',
    model_name: 'mistral',
    updated_at: getDateString(1),
    archived: false,
    agent_enabled: false,
    agent_sandboxed: true,
    created_at: '2024-01-02T00:00:00Z',
  },
  {
    id: 'chat-3',
    title: 'Chat 3',
    model_name: 'llama2',
    updated_at: getDateString(3),
    archived: false,
    agent_enabled: false,
    agent_sandboxed: true,
    created_at: '2024-01-03T00:00:00Z',
  },
  {
    id: 'chat-4',
    title: 'Chat 4',
    model_name: 'mistral',
    updated_at: getDateString(10),
    archived: false,
    agent_enabled: false,
    agent_sandboxed: true,
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
  agent_sandboxed: true,
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
  agent_sandboxed: true,
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
  agent_sandboxed: true,
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
      fireEvent.click(screen.getByRole('button', { name: 'Rename Chat 1' }));
      expect(screen.getByLabelText('Chat name')).toHaveValue('Chat 1');
      fireEvent.change(screen.getByLabelText('Chat name'), { target: { value: '  New name  ' } });
      fireEvent.click(screen.getByRole('button', { name: 'Save name' }));
      await screen.findByRole('heading', { name: 'New name' });
      expect(mockUpdateChat).toHaveBeenCalledWith('chat-1', { title: 'New name' });
      expect(screen.getAllByText('New name')).toHaveLength(2);
      expect(screen.getByText('Still writing')).toBeInTheDocument();
    });

    it('keeps failed input, rejects blank names, and cancels without an update', async () => {
      mockUpdateChat.mockRejectedValue(new Error('Rename unavailable'));
      renderChatsPage();
      fireEvent.click(await screen.findByRole('button', { name: 'Rename Chat 1' }));
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
      fireEvent.click(screen.getByRole('button', { name: 'Rename Chat 2' }));
      fireEvent.click(screen.getByRole('button', { name: 'Save name' }));
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(mockUpdateChat).toHaveBeenCalledWith('chat-2', { title: 'Chat 2' });
      expect(screen.getByRole('heading', { name: mockChatWithMessages.title })).toBeInTheDocument();
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
      expect(screen.getByText('First chunk continues')).toBeInTheDocument();
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
    fireEvent.click(screen.getByRole('button', { name: 'Rename Chat 1' }));
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
    fireEvent.click(screen.getByRole('button', { name: 'Rename Chat 1' }));
    fireEvent.change(screen.getByLabelText('Chat name'), { target: { value: 'My first name' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save name' }));
    await screen.findByRole('heading', { name: 'My first name' });
    act(() => socket.emit({ type: 'title_updated', chat_id: 'chat-1', title: 'Late summary' }));
    expect(screen.getAllByText('My first name')).toHaveLength(2);
    expect(screen.queryByText('Late summary')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Rename Chat 2' }));
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
        expect(screen.getByRole('button', { name: 'New chat' })).toBeInTheDocument();
      });
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
        expect(screen.getByRole('button', { name: 'Start New Chat' })).toBeInTheDocument();
      });
    });

    it('opens new chat modal from start new chat button', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Start New Chat' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Start New Chat' }));

      expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
    });
  });

  describe('new chat modal', () => {
    it('opens new chat modal', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'New chat' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'New chat' }));

      expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      expect(screen.getByLabelText('Select Model')).toBeInTheDocument();
    });

    it('closes new chat modal on cancel', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'New chat' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'New chat' }));
      expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();

      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      await waitFor(() => {
        expect(screen.queryByRole('heading', { name: 'New Chat' })).not.toBeInTheDocument();
      });
    });

    it('closes modal on backdrop click', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'New chat' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'New chat' }));

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
        expect(screen.getByRole('button', { name: 'New chat' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'New chat' }));

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
        agent_sandboxed: true,
        created_at: '2024-01-05T00:00:00Z',
      };
      mockClient.createChat.mockResolvedValueOnce(newChat);
      mockClient.getChat.mockResolvedValueOnce({ ...newChat, messages: [] });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'New chat' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'New chat' }));

      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      });

      // Open the select dropdown by clicking the trigger
      const selectTrigger = screen.getByRole('combobox');
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
          agent_sandboxed: true,
        });
      });
    });

    it('disables create button when no model selected', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'New chat' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'New chat' }));

      expect(screen.getByRole('button', { name: 'Create Chat' })).toBeDisabled();
    });

    it('shows error when create chat fails', async () => {
      mockClient.createChat.mockRejectedValueOnce(new Error('Failed to create'));

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'New chat' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'New chat' }));

      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      });

      // Open the select dropdown by clicking the trigger
      const selectTrigger = screen.getByRole('combobox');
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
    async function sendPrompt(): Promise<HTMLTextAreaElement> {
      renderChatsPage();
      fireEvent.click(await screen.findByText('Chat 1'));
      const input = await screen.findByPlaceholderText<HTMLTextAreaElement>(
        'Type a message, or drop a file...'
      );
      fireEvent.change(input, { target: { value: 'Generate an image of a rooster' } });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));
      await waitFor(() => expect(input.value).toBe(''));
      return input;
    }

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
        agent_sandboxed: true,
        created_at: '2024-01-01T00:00:00Z',
      };
      mockClient.archiveChat.mockResolvedValueOnce(archivedChat);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      // Find the archive button for the first chat
      const archiveButtons = screen.getAllByTitle('Archive');
      fireEvent.click(archiveButtons[0]);

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

      const archiveButtons = screen.getAllByTitle('Archive');
      fireEvent.click(archiveButtons[0]);

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
          agent_sandboxed: true,
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
        agent_sandboxed: true,
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

      const unarchiveButton = screen.getByTitle('Unarchive');
      fireEvent.click(unarchiveButton);

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
          agent_sandboxed: true,
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

      fireEvent.click(screen.getByTitle('Unarchive'));

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
        agent_sandboxed: true,
        created_at: '2024-01-01T00:00:00Z',
      };
      mockClient.archiveChat.mockResolvedValueOnce(archivedChat);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      // Select the chat first
      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByText('Hello')).toBeInTheDocument();
      });

      // Archive it
      const archiveButtons = screen.getAllByTitle('Archive');
      fireEvent.click(archiveButtons[0]);

      await waitFor(() => {
        expect(mockClient.archiveChat).toHaveBeenCalledWith('chat-1');
      });
    });
  });

  describe('delete chat', () => {
    it('opens delete confirmation modal', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      const deleteButtons = screen.getAllByTitle('Delete');
      fireEvent.click(deleteButtons[0]);

      expect(screen.getByRole('heading', { name: 'Delete Chat' })).toBeInTheDocument();
      expect(
        screen.getByText('Are you sure you want to delete this chat? This action cannot be undone.')
      ).toBeInTheDocument();
    });

    it('cancels delete on cancel button', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      const deleteButtons = screen.getAllByTitle('Delete');
      fireEvent.click(deleteButtons[0]);

      // Click the Cancel button in the modal (not the icon buttons)
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

      const deleteButtons = screen.getAllByTitle('Delete');
      fireEvent.click(deleteButtons[0]);

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

      const deleteButtons = screen.getAllByTitle('Delete');
      fireEvent.click(deleteButtons[0]);

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

      const deleteButtons = screen.getAllByTitle('Delete');
      fireEvent.click(deleteButtons[0]);

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

      const deleteButtons = screen.getAllByTitle('Delete');
      fireEvent.click(deleteButtons[0]);

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

      // Select the chat
      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByText('Hello')).toBeInTheDocument();
      });

      // Delete it
      const deleteButtons = screen.getAllByTitle('Delete');
      fireEvent.click(deleteButtons[0]);

      // Click the Delete button in the modal using accessible query
      const deleteModalButtons = screen.getAllByRole('button', { name: 'Delete' });
      fireEvent.click(deleteModalButtons[deleteModalButtons.length - 1]);

      await waitFor(() => {
        expect(mockClient.deleteChat).toHaveBeenCalledWith('chat-1');
      });
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
        expect(screen.getByText('No messages yet. Start a conversation!')).toBeInTheDocument();
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

    it('displays chat model name', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByText('llama2')).toBeInTheDocument();
      });
    });
  });

  describe('sandbox toggle', () => {
    const openAgentChat = async (agentSandboxed: boolean) => {
      mockClient.getChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        agent_enabled: true,
        agent_sandboxed: agentSandboxed,
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByTestId('sandbox-toggle')).toBeInTheDocument();
      });
    };

    it('is hidden until agent mode is on', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });
      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByTestId('agent-toggle')).toBeInTheDocument();
      });
      // There is nothing to sandbox while replies come straight from the model.
      expect(screen.queryByTestId('sandbox-toggle')).not.toBeInTheDocument();
    });

    it('says which mode the chat is in', async () => {
      await openAgentChat(true);
      expect(screen.getByTestId('sandbox-toggle')).toHaveTextContent('Sandboxed');
    });

    it('calls out host access rather than reading as just another setting', async () => {
      await openAgentChat(false);

      const toggle = screen.getByTestId('sandbox-toggle');
      expect(toggle).toHaveTextContent('Host access');
      expect(toggle).toHaveAttribute('aria-pressed', 'true');
      expect(toggle.className).toContain('agent-toggle-unsandboxed');
      expect(toggle.getAttribute('title')).toContain('run shell commands and write files');
    });

    it('leaves the sandbox when clicked', async () => {
      await openAgentChat(true);

      mockClient.updateChat.mockResolvedValueOnce({
        ...mockChatWithMessages,
        agent_enabled: true,
        agent_sandboxed: false,
      });

      fireEvent.click(screen.getByTestId('sandbox-toggle'));

      await waitFor(() => {
        expect(mockClient.updateChat).toHaveBeenCalledWith('chat-1', {
          agent_sandboxed: false,
        });
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
      expect(screen.getByText('95%')).toBeInTheDocument();
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
        expect(screen.getByText('No messages found')).toBeInTheDocument();
      });
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

    it('hides filter buttons when showing search results', async () => {
      mockClient.searchChatMessages.mockResolvedValueOnce({
        results: mockSearchResults,
        total: 2,
      });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('tab', { name: 'Active' })).toBeInTheDocument();
        expect(screen.getByRole('tab', { name: 'Archived' })).toBeInTheDocument();
      });

      const searchInput = screen.getByTestId('chat-search-input');
      fireEvent.change(searchInput, { target: { value: 'TypeScript' } });
      fireEvent.submit(searchInput.closest('form')!);

      await waitFor(() => {
        expect(screen.getByTestId('search-results-list')).toBeInTheDocument();
      });

      expect(screen.queryByRole('tab', { name: 'Active' })).not.toBeInTheDocument();
      expect(screen.queryByRole('tab', { name: 'Archived' })).not.toBeInTheDocument();
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
});
