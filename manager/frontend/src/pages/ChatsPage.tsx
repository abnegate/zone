import { type FormEvent, useCallback, useEffect, useRef, useState } from 'react';
import { client } from '../api/client';
import { useAuth } from '../context/AuthContext';
import { useModels } from '../hooks/useModels';
import type { Chat, ChatWithMessages } from '../types';
import './ChatsPage.css';

function formatDate(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

  if (diffDays === 0) {
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }
  if (diffDays === 1) {
    return 'Yesterday';
  }
  if (diffDays < 7) {
    return date.toLocaleDateString([], { weekday: 'short' });
  }
  return date.toLocaleDateString([], { month: 'short', day: 'numeric' });
}

export default function ChatsPage() {
  const { isAuthenticated } = useAuth();
  const { models } = useModels();

  const [chats, setChats] = useState<Chat[]>([]);
  const [activeChat, setActiveChat] = useState<ChatWithMessages | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [messageInput, setMessageInput] = useState('');
  const [sending, setSending] = useState(false);
  const [showArchived, setShowArchived] = useState(false);
  const [showNewChatModal, setShowNewChatModal] = useState(false);
  const [newChatModel, setNewChatModel] = useState('');
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);

  const messagesEndRef = useRef<HTMLDivElement>(null);

  const scrollToBottom = useCallback(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, []);

  useEffect(() => {
    scrollToBottom();
  }, [activeChat?.messages, scrollToBottom]);

  const loadChats = useCallback(async () => {
    if (!isAuthenticated) return;
    setLoading(true);
    setError(null);
    try {
      const chatList = await client.getChats(showArchived ? true : false);
      setChats(chatList);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load chats');
    } finally {
      setLoading(false);
    }
  }, [isAuthenticated, showArchived]);

  useEffect(() => {
    loadChats();
  }, [loadChats]);

  const selectChat = async (chatId: string) => {
    if (!isAuthenticated) return;
    try {
      const chat = await client.getChat(chatId);
      setActiveChat(chat);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load chat');
    }
  };

  const handleCreateChat = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated || !newChatModel) return;

    try {
      const chat = await client.createChat({ model_name: newChatModel });
      setChats((prev) => [chat, ...prev]);
      setShowNewChatModal(false);
      setNewChatModel('');
      await selectChat(chat.id);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create chat');
    }
  };

  const handleSendMessage = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated || !activeChat || !messageInput.trim() || sending) return;

    setSending(true);
    try {
      const message = await client.sendMessage(activeChat.id, { content: messageInput.trim() });
      setActiveChat((prev) => (prev ? { ...prev, messages: [...prev.messages, message] } : null));
      setMessageInput('');
      // Refresh chat list to update last message time
      loadChats();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to send message');
    } finally {
      setSending(false);
    }
  };

  const handleArchiveChat = async (chatId: string) => {
    if (!isAuthenticated) return;
    try {
      await client.archiveChat(chatId);
      if (activeChat?.id === chatId) {
        setActiveChat(null);
      }
      loadChats();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to archive chat');
    }
  };

  const handleUnarchiveChat = async (chatId: string) => {
    if (!isAuthenticated) return;
    try {
      await client.unarchiveChat(chatId);
      loadChats();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to unarchive chat');
    }
  };

  const handleDeleteChat = async (chatId: string) => {
    if (!isAuthenticated) return;
    try {
      await client.deleteChat(chatId);
      if (activeChat?.id === chatId) {
        setActiveChat(null);
      }
      setDeleteConfirm(null);
      loadChats();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete chat');
    }
  };

  return (
    <div className="page page--full chats-page">
      <div className="chats-sidebar">
        <div className="chats-sidebar-header">
          <h2>Chats</h2>
          <button
            className="btn btn-primary btn-sm"
            onClick={() => setShowNewChatModal(true)}
            type="button"
          >
            + New
          </button>
        </div>

        <div className="chats-filter">
          <button
            className={`filter-btn ${!showArchived ? 'active' : ''}`}
            onClick={() => setShowArchived(false)}
            type="button"
          >
            Active
          </button>
          <button
            className={`filter-btn ${showArchived ? 'active' : ''}`}
            onClick={() => setShowArchived(true)}
            type="button"
          >
            Archived
          </button>
        </div>

        {loading ? (
          <div className="chats-loading">
            <span className="spinner" /> Loading...
          </div>
        ) : error ? (
          <div className="chats-error">{error}</div>
        ) : chats.length === 0 ? (
          <div className="chats-empty">{showArchived ? 'No archived chats' : 'No chats yet'}</div>
        ) : (
          <div className="chats-list">
            {chats.map((chat) => (
              <div
                key={chat.id}
                className={`chat-item ${activeChat?.id === chat.id ? 'active' : ''}`}
                onClick={() => selectChat(chat.id)}
                onKeyDown={(e) => e.key === 'Enter' && selectChat(chat.id)}
                role="button"
                tabIndex={0}
              >
                <div className="chat-item-content">
                  <span className="chat-title">{chat.title}</span>
                  <span className="chat-meta">
                    {chat.model_name} · {formatDate(chat.updated_at)}
                  </span>
                </div>
                <div className="chat-item-actions">
                  {showArchived ? (
                    <button
                      className="btn btn-icon btn-xs"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleUnarchiveChat(chat.id);
                      }}
                      title="Unarchive"
                      type="button"
                    >
                      <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        width="14"
                        height="14"
                      >
                        <path d="M3 6h18M3 6v14a2 2 0 002 2h14a2 2 0 002-2V6M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2M10 11v6M14 11v6" />
                      </svg>
                    </button>
                  ) : (
                    <button
                      className="btn btn-icon btn-xs"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleArchiveChat(chat.id);
                      }}
                      title="Archive"
                      type="button"
                    >
                      <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        width="14"
                        height="14"
                      >
                        <path d="M21 8v13H3V8M1 3h22v5H1zM10 12h4" />
                      </svg>
                    </button>
                  )}
                  <button
                    className="btn btn-icon btn-xs btn-danger-icon"
                    onClick={(e) => {
                      e.stopPropagation();
                      setDeleteConfirm(chat.id);
                    }}
                    title="Delete"
                    type="button"
                  >
                    <svg
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      width="14"
                      height="14"
                    >
                      <path d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                    </svg>
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="chats-main">
        {activeChat ? (
          <>
            <div className="chat-header">
              <div className="chat-header-info">
                <h3>{activeChat.title}</h3>
                <span className="chat-model">{activeChat.model_name}</span>
              </div>
            </div>

            <div className="messages-container">
              {activeChat.messages.length === 0 ? (
                <div className="messages-empty">
                  <p>No messages yet. Start a conversation!</p>
                </div>
              ) : (
                activeChat.messages.map((message) => (
                  <div key={message.id} className={`message message-${message.role}`}>
                    <div className="message-header">
                      <span className="message-role">
                        {message.role === 'user'
                          ? 'You'
                          : message.role === 'assistant'
                            ? 'Assistant'
                            : 'System'}
                      </span>
                      <span className="message-time">{formatDate(message.created_at)}</span>
                    </div>
                    <div className="message-content">{message.content}</div>
                  </div>
                ))
              )}
              <div ref={messagesEndRef} />
            </div>

            <form className="message-form" onSubmit={handleSendMessage}>
              <textarea
                placeholder="Type a message..."
                value={messageInput}
                onChange={(e) => setMessageInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && !e.shiftKey) {
                    e.preventDefault();
                    handleSendMessage(e);
                  }
                }}
                disabled={sending}
                rows={1}
              />
              <button
                type="submit"
                className="btn btn-primary"
                disabled={sending || !messageInput.trim()}
              >
                {sending ? <span className="spinner" /> : 'Send'}
              </button>
            </form>
          </>
        ) : (
          <div className="chat-placeholder">
            <div className="placeholder-icon">
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                width="64"
                height="64"
              >
                <path d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
              </svg>
            </div>
            <h3>Select a chat to start</h3>
            <p>Choose an existing conversation or create a new one</p>
            <button
              className="btn btn-primary"
              onClick={() => setShowNewChatModal(true)}
              type="button"
            >
              Start New Chat
            </button>
          </div>
        )}
      </div>

      {/* New Chat Modal */}
      {showNewChatModal && (
        <div className="modal">
          <div
            className="modal-backdrop"
            onClick={() => setShowNewChatModal(false)}
            onKeyDown={(e) => e.key === 'Escape' && setShowNewChatModal(false)}
            role="button"
            tabIndex={0}
            aria-label="Close modal"
          />
          <div className="modal-content">
            <h3>New Chat</h3>
            <form onSubmit={handleCreateChat}>
              <div className="form-group">
                <label htmlFor="model-select">Select Model</label>
                <select
                  id="model-select"
                  value={newChatModel}
                  onChange={(e) => setNewChatModel(e.target.value)}
                  required
                >
                  <option value="">Choose a model...</option>
                  {models.map((model) => (
                    <option key={model.name} value={model.name}>
                      {model.name}
                    </option>
                  ))}
                </select>
              </div>
              <div className="modal-actions">
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setShowNewChatModal(false)}
                >
                  Cancel
                </button>
                <button type="submit" className="btn btn-primary" disabled={!newChatModel}>
                  Create Chat
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Delete Confirmation Modal */}
      {deleteConfirm && (
        <div className="modal">
          <div
            className="modal-backdrop"
            onClick={() => setDeleteConfirm(null)}
            onKeyDown={(e) => e.key === 'Escape' && setDeleteConfirm(null)}
            role="button"
            tabIndex={0}
            aria-label="Close modal"
          />
          <div className="modal-content">
            <h3>Delete Chat</h3>
            <p>Are you sure you want to delete this chat? This action cannot be undone.</p>
            <div className="modal-actions">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setDeleteConfirm(null)}
              >
                Cancel
              </button>
              <button
                type="button"
                className="btn btn-danger"
                onClick={() => handleDeleteChat(deleteConfirm)}
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
