import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import ChatsPage from './ChatsPage';
import { client } from '../api/client';
import type { Chat, ChatWithMessages, Message } from '../types';

// Mock scrollIntoView
Element.prototype.scrollIntoView = jest.fn();

// Mock the client
jest.mock('../api/client', () => ({
  client: {
    getChats: jest.fn(),
    getChat: jest.fn(),
    createChat: jest.fn(),
    sendMessage: jest.fn(),
    archiveChat: jest.fn(),
    unarchiveChat: jest.fn(),
    deleteChat: jest.fn(),
    setAccessToken: jest.fn(),
  },
}));

// Mock useModels
jest.mock('../hooks/useModels', () => ({
  useModels: () => ({
    models: [
      { name: 'llama2', size: 1, modified_at: '' },
      { name: 'mistral', size: 1, modified_at: '' },
    ],
    loading: false,
    error: null,
  }),
}));

// Mock useAuth to always return authenticated
jest.mock('../context/AuthContext', () => ({
  useAuth: () => ({
    isAuthenticated: true,
    user: { id: '1', email: 'test@test.com' },
    roles: ['user'],
    permissions: ['chats:read', 'chats:create', 'chats:delete'],
    hasPermission: (p: string) => ['chats:read', 'chats:create', 'chats:delete'].includes(p),
    hasAnyPermission: () => true,
    hasRole: () => true,
    logout: jest.fn(),
    login: jest.fn(),
    register: jest.fn(),
    setAccessToken: jest.fn(),
  }),
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}));

const mockClient = client as jest.Mocked<typeof client>;

// Helper to create date strings relative to now for testing formatDate
const getDateString = (daysAgo: number, hours = 0): string => {
  const date = new Date();
  date.setDate(date.getDate() - daysAgo);
  date.setHours(date.getHours() - hours);
  return date.toISOString();
};

const mockChats: Chat[] = [
  { id: 'chat-1', title: 'Chat 1', model_name: 'llama2', updated_at: getDateString(0), archived: false, created_at: '2024-01-01T00:00:00Z' },
  { id: 'chat-2', title: 'Chat 2', model_name: 'mistral', updated_at: getDateString(1), archived: false, created_at: '2024-01-02T00:00:00Z' },
  { id: 'chat-3', title: 'Chat 3', model_name: 'llama2', updated_at: getDateString(3), archived: false, created_at: '2024-01-03T00:00:00Z' },
  { id: 'chat-4', title: 'Chat 4', model_name: 'mistral', updated_at: getDateString(10), archived: false, created_at: '2024-01-04T00:00:00Z' },
];

const mockChatWithMessages: ChatWithMessages = {
  id: 'chat-1',
  title: 'Chat 1',
  model_name: 'llama2',
  updated_at: '2024-01-01T00:00:00Z',
  archived: false,
  created_at: '2024-01-01T00:00:00Z',
  messages: [
    { id: 'msg-1', chat_id: 'chat-1', role: 'user', content: 'Hello', created_at: '2024-01-01T00:00:00Z' },
    { id: 'msg-2', chat_id: 'chat-1', role: 'assistant', content: 'Hi there!', created_at: '2024-01-01T00:01:00Z' },
  ],
};

const mockChatEmpty: ChatWithMessages = {
  id: 'chat-2',
  title: 'Empty Chat',
  model_name: 'mistral',
  updated_at: '2024-01-02T00:00:00Z',
  archived: false,
  created_at: '2024-01-02T00:00:00Z',
  messages: [],
};

const mockChatWithSystemMessage: ChatWithMessages = {
  id: 'chat-3',
  title: 'System Chat',
  model_name: 'llama2',
  updated_at: '2024-01-03T00:00:00Z',
  archived: false,
  created_at: '2024-01-03T00:00:00Z',
  messages: [
    { id: 'msg-3', chat_id: 'chat-3', role: 'system', content: 'System message', created_at: '2024-01-03T00:00:00Z' },
  ],
};

const renderChatsPage = () => {
  return render(
    <BrowserRouter>
      <ChatsPage />
    </BrowserRouter>
  );
};

describe('ChatsPage', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    mockClient.getChats.mockResolvedValue(mockChats);
    mockClient.getChat.mockResolvedValue(mockChatWithMessages);
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
        expect(screen.getByRole('button', { name: '+ New' })).toBeInTheDocument();
      });
    });

    it('renders filter buttons', async () => {
      renderChatsPage();
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Active' })).toBeInTheDocument();
        expect(screen.getByRole('button', { name: 'Archived' })).toBeInTheDocument();
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
    });
  });

  describe('archive filter', () => {
    it('switches to archived filter', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Archived' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Archived' }));

      await waitFor(() => {
        expect(mockClient.getChats).toHaveBeenCalledWith(true);
      });
    });

    it('switches back to active filter', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Archived' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Archived' }));
      fireEvent.click(screen.getByRole('button', { name: 'Active' }));

      await waitFor(() => {
        expect(mockClient.getChats).toHaveBeenLastCalledWith(false);
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
        expect(screen.getByRole('button', { name: '+ New' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ New' }));

      expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();
      expect(screen.getByLabelText('Select Model')).toBeInTheDocument();
    });

    it('closes new chat modal on cancel', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ New' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ New' }));
      expect(screen.getByRole('heading', { name: 'New Chat' })).toBeInTheDocument();

      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      await waitFor(() => {
        expect(screen.queryByRole('heading', { name: 'New Chat' })).not.toBeInTheDocument();
      });
    });

    it('closes modal on backdrop click', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ New' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ New' }));

      const backdrop = screen.getByLabelText('Close modal');
      fireEvent.click(backdrop);

      await waitFor(() => {
        expect(screen.queryByRole('heading', { name: 'New Chat' })).not.toBeInTheDocument();
      });
    });

    it('closes modal on escape key', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ New' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ New' }));

      const backdrop = screen.getByLabelText('Close modal');
      fireEvent.keyDown(backdrop, { key: 'Escape' });

      await waitFor(() => {
        expect(screen.queryByRole('heading', { name: 'New Chat' })).not.toBeInTheDocument();
      });
    });

    it('creates chat when form is submitted', async () => {
      const newChat: Chat = {
        id: 'new-chat',
        title: 'New Chat',
        model_name: 'llama2',
        updated_at: '2024-01-05T00:00:00Z',
        archived: false,
        created_at: '2024-01-05T00:00:00Z',
      };
      mockClient.createChat.mockResolvedValueOnce(newChat);
      mockClient.getChat.mockResolvedValueOnce({ ...newChat, messages: [] });

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ New' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ New' }));

      fireEvent.change(screen.getByLabelText('Select Model'), { target: { value: 'llama2' } });
      fireEvent.click(screen.getByRole('button', { name: 'Create Chat' }));

      await waitFor(() => {
        expect(mockClient.createChat).toHaveBeenCalledWith({ model_name: 'llama2' });
      });
    });

    it('disables create button when no model selected', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ New' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ New' }));

      expect(screen.getByRole('button', { name: 'Create Chat' })).toBeDisabled();
    });

    it('shows error when create chat fails', async () => {
      mockClient.createChat.mockRejectedValueOnce(new Error('Failed to create'));

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: '+ New' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: '+ New' }));
      fireEvent.change(screen.getByLabelText('Select Model'), { target: { value: 'llama2' } });
      fireEvent.click(screen.getByRole('button', { name: 'Create Chat' }));

      await waitFor(() => {
        expect(screen.getByText('Failed to create')).toBeInTheDocument();
      });
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
        expect(screen.getByPlaceholderText('Type a message...')).toBeInTheDocument();
      });

      fireEvent.change(screen.getByPlaceholderText('Type a message...'), {
        target: { value: 'Test message' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));

      await waitFor(() => {
        expect(mockClient.sendMessage).toHaveBeenCalledWith('chat-1', { content: 'Test message' });
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
        expect(screen.getByPlaceholderText('Type a message...')).toBeInTheDocument();
      });

      const textarea = screen.getByPlaceholderText('Type a message...');
      fireEvent.change(textarea, { target: { value: 'Enter test' } });
      fireEvent.keyDown(textarea, { key: 'Enter', shiftKey: false });

      await waitFor(() => {
        expect(mockClient.sendMessage).toHaveBeenCalled();
      });
    });

    it('does not send message on Shift+Enter', async () => {
      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByPlaceholderText('Type a message...')).toBeInTheDocument();
      });

      const textarea = screen.getByPlaceholderText('Type a message...');
      fireEvent.change(textarea, { target: { value: 'Shift enter test' } });
      fireEvent.keyDown(textarea, { key: 'Enter', shiftKey: true });

      expect(mockClient.sendMessage).not.toHaveBeenCalled();
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
      mockClient.sendMessage.mockRejectedValueOnce(new Error('Send failed'));

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByText('Chat 1')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('Chat 1'));

      await waitFor(() => {
        expect(screen.getByPlaceholderText('Type a message...')).toBeInTheDocument();
      });

      fireEvent.change(screen.getByPlaceholderText('Type a message...'), {
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
      const archivedChat: Chat = { id: 'chat-1', title: 'Chat 1', model_name: 'llama2', updated_at: '2024-01-01T00:00:00Z', archived: true, created_at: '2024-01-01T00:00:00Z' };
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
        { id: 'chat-archived', title: 'Archived Chat', model_name: 'llama2', updated_at: '2024-01-01T00:00:00Z', archived: true, created_at: '2024-01-01T00:00:00Z' },
      ];
      mockClient.getChats
        .mockResolvedValueOnce(mockChats)
        .mockResolvedValueOnce(archivedChats);
      const unarchivedChat: Chat = { id: 'chat-archived', title: 'Archived Chat', model_name: 'llama2', updated_at: '2024-01-01T00:00:00Z', archived: false, created_at: '2024-01-01T00:00:00Z' };
      mockClient.unarchiveChat.mockResolvedValueOnce(unarchivedChat);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Archived' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Archived' }));

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
        { id: 'chat-archived', title: 'Archived Chat', model_name: 'llama2', updated_at: '2024-01-01T00:00:00Z', archived: true, created_at: '2024-01-01T00:00:00Z' },
      ];
      mockClient.getChats
        .mockResolvedValueOnce(mockChats)
        .mockResolvedValueOnce(archivedChats);
      mockClient.unarchiveChat.mockRejectedValueOnce(new Error('Unarchive failed'));

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Archived' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Archived' }));

      await waitFor(() => {
        expect(screen.getByText('Archived Chat')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByTitle('Unarchive'));

      await waitFor(() => {
        expect(screen.getByText('Unarchive failed')).toBeInTheDocument();
      });
    });

    it('shows no archived chats message', async () => {
      mockClient.getChats
        .mockResolvedValueOnce(mockChats)
        .mockResolvedValueOnce([]);

      renderChatsPage();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Archived' })).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole('button', { name: 'Archived' }));

      await waitFor(() => {
        expect(screen.getByText('No archived chats')).toBeInTheDocument();
      });
    });

    it('clears active chat when archiving it', async () => {
      const archivedChat: Chat = { id: 'chat-1', title: 'Chat 1', model_name: 'llama2', updated_at: '2024-01-01T00:00:00Z', archived: true, created_at: '2024-01-01T00:00:00Z' };
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
      expect(screen.getByText('Are you sure you want to delete this chat? This action cannot be undone.')).toBeInTheDocument();
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

      const backdrops = screen.getAllByLabelText('Close modal');
      fireEvent.click(backdrops[0]);

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

      const backdrops = screen.getAllByLabelText('Close modal');
      fireEvent.keyDown(backdrops[0], { key: 'Escape' });

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

      // Click the Delete button in the modal
      const modalContent = document.querySelector('.modal-content');
      const confirmDeleteBtn = modalContent?.querySelector('.btn-danger') as HTMLButtonElement;
      fireEvent.click(confirmDeleteBtn);

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

      // Click the Delete button in the modal
      const modalContent = document.querySelector('.modal-content');
      const confirmDeleteBtn = modalContent?.querySelector('.btn-danger') as HTMLButtonElement;
      fireEvent.click(confirmDeleteBtn);

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

      // Click the Delete button in the modal
      const modalContent = document.querySelector('.modal-content');
      const confirmDeleteBtn = modalContent?.querySelector('.btn-danger') as HTMLButtonElement;
      fireEvent.click(confirmDeleteBtn);

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
});
