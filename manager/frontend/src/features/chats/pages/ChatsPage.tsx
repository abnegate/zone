import { Button, Checkbox, EmptyState, Modal, Select, Tabs, TabsList, TabsTrigger } from '@zone/ui';
import { type FormEvent, useCallback, useEffect, useRef, useState } from 'react';
import { useAuth } from '../../../features/auth';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { useModels } from '../../models';
import { MessageContent, ToolTrace } from '../components';
import { useChat, useChatSearch, useChats } from '../hooks';
import type { ChatSearchResult } from '../types';
import {
  type Attachment,
  attachmentMetadata,
  buildMessageWithAttachments,
  formatBytes,
  imageAttachments,
  isSendable,
  readAttachment,
} from '../utils';
import { formatDate } from '../utils';
import './ChatsPage.css';

export default function ChatsPage() {
  const { isAuthenticated } = useAuth();
  const { currentWorkspace } = useWorkspace();
  const { models } = useModels();

  const [showArchived, setShowArchived] = useState(false);
  const [selectedChatId, setSelectedChatId] = useState<string | null>(null);
  const [showNewChatModal, setShowNewChatModal] = useState(false);
  const [newChatModel, setNewChatModel] = useState('');
  const [newChatAgent, setNewChatAgent] = useState(false);
  const [newChatSandboxed, setNewChatSandboxed] = useState(true);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [showSearchResults, setShowSearchResults] = useState(false);
  const [messageInput, setMessageInput] = useState('');
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const addFiles = async (files: FileList | null) => {
    if (!files || files.length === 0) return;
    const read = await Promise.all(Array.from(files).map(readAttachment));
    setAttachments((prev) => {
      const seen = new Set(prev.map((a) => a.id));
      return [...prev, ...read.filter((a) => !seen.has(a.id))];
    });
  };

  const removeAttachment = (id: string) => {
    setAttachments((prev) => prev.filter((a) => a.id !== id));
  };
  const [sending, setSending] = useState(false);
  const [operationError, setOperationError] = useState<string | null>(null);

  // Use feature hooks
  const {
    chats,
    loading: chatsLoading,
    error: chatsError,
    createChat,
    deleteChat: deleteChatFn,
    archiveChat: archiveChatFn,
    unarchiveChat: unarchiveChatFn,
    refresh: refreshChats,
  } = useChats({ archived: showArchived });

  const {
    chat: activeChat,
    error: chatError,
    sendMessage: sendMessageFn,
    setAgentEnabled: setAgentEnabledFn,
    setAgentSandboxed: setAgentSandboxedFn,
  } = useChat(selectedChatId);

  const {
    results: searchResults,
    searching,
    error: searchError,
    search,
    clear: clearSearch,
  } = useChatSearch();

  const messagesEndRef = useRef<HTMLDivElement>(null);

  // Only render a conversation that matches the current selection so a
  // previous chat never flashes in the main pane while the next one loads.
  const displayedChat = activeChat?.id === selectedChatId ? activeChat : null;

  const scrollToBottom = useCallback(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll when messages change
  useEffect(() => {
    scrollToBottom();
  }, [activeChat?.messages, scrollToBottom]);

  const selectChat = (chatId: string) => {
    setSelectedChatId(chatId);
  };

  const handleCreateChat = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated) {
      setOperationError('You must be logged in to create a chat');
      return;
    }
    if (!currentWorkspace) {
      setOperationError('No workspace selected. Please select or create a workspace first.');
      return;
    }
    if (!newChatModel) {
      setOperationError('Please select a model');
      return;
    }

    setOperationError(null);
    try {
      const chat = await createChat({
        workspace_id: currentWorkspace.id,
        title: `Chat with ${newChatModel}`,
        model_name: newChatModel,
        agent_enabled: newChatAgent,
        agent_sandboxed: newChatSandboxed,
      });
      setShowNewChatModal(false);
      setNewChatModel('');
      setNewChatAgent(false);
      setNewChatSandboxed(true);
      selectChat(chat.id);
    } catch (err) {
      setOperationError(err instanceof Error ? err.message : 'Failed to create chat');
    }
  };

  const handleToggleAgent = async () => {
    if (!isAuthenticated || !displayedChat) return;
    setOperationError(null);
    try {
      await setAgentEnabledFn(!displayedChat.agent_enabled);
    } catch (err) {
      setOperationError(err instanceof Error ? err.message : 'Failed to change agent mode');
    }
  };

  const handleToggleSandbox = async () => {
    if (!isAuthenticated || !displayedChat) return;
    setOperationError(null);
    try {
      await setAgentSandboxedFn(!displayedChat.agent_sandboxed);
    } catch (err) {
      setOperationError(err instanceof Error ? err.message : 'Failed to change sandbox mode');
    }
  };

  const handleSendMessage = async (e: FormEvent) => {
    e.preventDefault();
    const sendable = attachments.filter(isSendable);
    if (!isAuthenticated || !activeChat || sending) return;
    if (!messageInput.trim() && sendable.length === 0) return;

    setSending(true);
    setOperationError(null);
    try {
      await sendMessageFn({
        content: buildMessageWithAttachments(messageInput.trim(), sendable),
        metadata: attachmentMetadata(sendable),
      });
      setMessageInput('');
      setAttachments([]);
      // Refresh chat list to update last message time
      await refreshChats();
    } catch (err) {
      setOperationError(err instanceof Error ? err.message : 'Failed to send message');
    } finally {
      setSending(false);
    }
  };

  const handleArchiveChat = async (chatId: string) => {
    if (!isAuthenticated) return;
    setOperationError(null);
    try {
      await archiveChatFn(chatId);
      if (selectedChatId === chatId) {
        setSelectedChatId(null);
      }
    } catch (err) {
      setOperationError(err instanceof Error ? err.message : 'Failed to archive chat');
    }
  };

  const handleUnarchiveChat = async (chatId: string) => {
    if (!isAuthenticated) return;
    setOperationError(null);
    try {
      await unarchiveChatFn(chatId);
    } catch (err) {
      setOperationError(err instanceof Error ? err.message : 'Failed to unarchive chat');
    }
  };

  const handleDeleteChat = async (chatId: string) => {
    if (!isAuthenticated) return;
    setOperationError(null);
    try {
      await deleteChatFn(chatId);
      if (selectedChatId === chatId) {
        setSelectedChatId(null);
      }
      setDeleteConfirm(null);
    } catch (err) {
      setOperationError(err instanceof Error ? err.message : 'Failed to delete chat');
    }
  };

  const handleSearch = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated || !searchQuery.trim()) return;

    setShowSearchResults(true);
    await search(searchQuery, { limit: 20 });
  };

  const handleSearchResultClick = async (result: ChatSearchResult) => {
    selectChat(result.chat_id);
    setShowSearchResults(false);
    setSearchQuery('');
    clearSearch();
  };

  const handleClearSearch = () => {
    setSearchQuery('');
    clearSearch();
    setShowSearchResults(false);
  };

  return (
    <div className="page page--workspace chats-page">
      <div className="chats-sidebar">
        <div className="chats-sidebar-header">
          <h1>Chats</h1>
          <Button
            variant="primary"
            size="sm"
            onClick={() => {
              setOperationError(null);
              setShowNewChatModal(true);
            }}
          >
            New chat
          </Button>
        </div>

        <form className="chat-search" onSubmit={handleSearch}>
          <svg
            className="chat-search-icon"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            width="16"
            height="16"
            aria-hidden="true"
          >
            <circle cx="11" cy="11" r="7" />
            <path d="M21 21l-4.3-4.3" />
          </svg>
          <input
            type="text"
            placeholder="Search messages..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="chat-search-input"
            data-testid="chat-search-input"
          />
          {searchQuery && (
            <button
              type="button"
              className="chat-search-clear"
              onClick={handleClearSearch}
              aria-label="Clear search"
              data-testid="clear-search-btn"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                width="16"
                height="16"
              >
                <path d="M18 6L6 18M6 6l12 12" />
              </svg>
            </button>
          )}
        </form>

        {!showSearchResults && (
          <Tabs
            value={showArchived ? 'archived' : 'active'}
            onValueChange={(v) => setShowArchived(v === 'archived')}
            className="chats-filter"
          >
            <TabsList className="w-full">
              <TabsTrigger value="active" className="flex-1">
                Active
              </TabsTrigger>
              <TabsTrigger value="archived" className="flex-1">
                Archived
              </TabsTrigger>
            </TabsList>
          </Tabs>
        )}

        {operationError && !showNewChatModal && (
          <div className="chats-error" role="alert">
            {operationError}
          </div>
        )}

        {showSearchResults ? (
          searching ? (
            <div className="chats-loading">
              <span className="spinner" /> Searching...
            </div>
          ) : searchError ? (
            <div className="chats-error">{searchError}</div>
          ) : searchResults.length === 0 ? (
            <div className="chats-empty">No messages found</div>
          ) : (
            <div className="chats-list" data-testid="search-results-list">
              {searchResults.map((result) => (
                <div
                  key={result.message_id}
                  className="search-result-item"
                  onClick={() => handleSearchResultClick(result)}
                  onKeyDown={(e) => e.key === 'Enter' && handleSearchResultClick(result)}
                  role="button"
                  tabIndex={0}
                  data-testid="search-result-item"
                >
                  <div className="search-result-header">
                    <span className="search-result-chat">{result.chat_title}</span>
                    <span className="search-result-score">
                      {Math.round(result.relevance_score * 100)}%
                    </span>
                  </div>
                  <div className="search-result-snippet">{result.snippet}</div>
                  <span className="search-result-date">{formatDate(result.created_at)}</span>
                </div>
              ))}
            </div>
          )
        ) : chatsLoading ? (
          <div className="chats-loading">
            <span className="spinner" /> Loading...
          </div>
        ) : chatsError ? (
          <div className="chats-error">{chatsError}</div>
        ) : chats.length === 0 ? (
          <EmptyState
            icon={
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                width="48"
                height="48"
              >
                <path d="M8 10h.01M12 10h.01M16 10h.01M9 16H5a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v8a2 2 0 01-2 2h-5l-5 4z" />
              </svg>
            }
            title={showArchived ? 'No archived chats' : 'No chats yet'}
            description={
              showArchived
                ? 'Your archived conversations will appear here'
                : 'Start a new conversation to get started'
            }
            action={
              !showArchived ? (
                <Button
                  onClick={() => {
                    setOperationError(null);
                    setShowNewChatModal(true);
                  }}
                >
                  New Chat
                </Button>
              ) : undefined
            }
          />
        ) : (
          <div className="chats-list">
            {chats.map((chat) => (
              <div
                key={chat.id}
                className={`chat-item ${selectedChatId === chat.id ? 'active' : ''}`}
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
        {displayedChat ? (
          <>
            <div className="chat-header">
              <div className="chat-header-info">
                <h3>{displayedChat.title}</h3>
                <span className="chat-model">{displayedChat.model_name}</span>
              </div>
              <button
                type="button"
                className="agent-toggle"
                onClick={handleToggleAgent}
                aria-pressed={displayedChat.agent_enabled}
                title={
                  displayedChat.agent_enabled
                    ? 'Agent mode on: replies can search this workspace before answering'
                    : 'Agent mode off: replies come straight from the model'
                }
                data-testid="agent-toggle"
              >
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.75"
                  width="14"
                  height="14"
                  aria-hidden="true"
                >
                  <path d="M12 3v2M12 19v2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M3 12h2M19 12h2M5.6 18.4L7 17M17 7l1.4-1.4" />
                  <circle cx="12" cy="12" r="3.5" />
                </svg>
                Agent
              </button>
              {displayedChat.agent_enabled && (
                <button
                  type="button"
                  className={
                    displayedChat.agent_sandboxed
                      ? 'agent-toggle'
                      : 'agent-toggle agent-toggle-unsandboxed'
                  }
                  onClick={handleToggleSandbox}
                  aria-pressed={!displayedChat.agent_sandboxed}
                  title={
                    displayedChat.agent_sandboxed
                      ? 'Sandboxed: the agent can only read this workspace'
                      : 'Unsandboxed: the agent can run shell commands and write files on the server'
                  }
                  data-testid="sandbox-toggle"
                >
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.75"
                    width="14"
                    height="14"
                    aria-hidden="true"
                  >
                    {displayedChat.agent_sandboxed ? (
                      <>
                        <rect x="4" y="10.5" width="16" height="10" rx="2" />
                        <path d="M8 10.5V7a4 4 0 0 1 8 0v3.5" />
                      </>
                    ) : (
                      <>
                        <rect x="4" y="10.5" width="16" height="10" rx="2" />
                        <path d="M8 10.5V7a4 4 0 0 1 7.5-2" />
                      </>
                    )}
                  </svg>
                  {displayedChat.agent_sandboxed ? 'Sandboxed' : 'Host access'}
                </button>
              )}
            </div>

            <div className="messages-container">
              {displayedChat.messages.length === 0 ? (
                <div className="messages-empty">
                  <p>No messages yet. Start a conversation!</p>
                </div>
              ) : (
                displayedChat.messages.map((message) => {
                  const images = imageAttachments(message.metadata);
                  const toolCalls = message.metadata?.tool_calls ?? [];
                  return (
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
                      {images.length > 0 && (
                        <div className="message-images">
                          {images.map((a) => (
                            <img
                              key={a.url}
                              src={a.url}
                              alt={a.name}
                              title={a.name}
                              data-testid="message-image"
                            />
                          ))}
                        </div>
                      )}
                      {toolCalls.length > 0 && <ToolTrace calls={toolCalls} />}
                      {message.content.trim() ? (
                        <div className="message-content">
                          <MessageContent content={message.content} />
                        </div>
                      ) : null}
                    </div>
                  );
                })
              )}
              <div ref={messagesEndRef} />
            </div>

            <form
              className={`message-form${isDragging ? ' is-dragging' : ''}`}
              onSubmit={handleSendMessage}
              onDragOver={(e) => {
                e.preventDefault();
                setIsDragging(true);
              }}
              onDragLeave={(e) => {
                if (e.currentTarget.contains(e.relatedTarget as Node)) return;
                setIsDragging(false);
              }}
              onDrop={(e) => {
                e.preventDefault();
                setIsDragging(false);
                addFiles(e.dataTransfer.files);
              }}
            >
              {attachments.length > 0 && (
                <div className="message-attachments">
                  {attachments.map((attachment) => (
                    <span
                      key={attachment.id}
                      className={`attachment-chip${attachment.rejected ? ' is-rejected' : ''}${attachment.url ? ' has-thumb' : ''}`}
                    >
                      {attachment.url ? (
                        <img className="attachment-chip-thumb" src={attachment.url} alt="" />
                      ) : null}
                      <span className="attachment-chip-name">{attachment.name}</span>
                      <span className="attachment-chip-size">
                        {attachment.rejected ? (
                          <span className="attachment-chip-note">{attachment.rejected}</span>
                        ) : (
                          formatBytes(attachment.size)
                        )}
                      </span>
                      <button
                        type="button"
                        className="attachment-chip-remove"
                        onClick={() => removeAttachment(attachment.id)}
                        aria-label={`Remove ${attachment.name}`}
                      >
                        ×
                      </button>
                    </span>
                  ))}
                </div>
              )}

              <div className="message-form-row">
                <input
                  ref={fileInputRef}
                  type="file"
                  multiple
                  hidden
                  onChange={(e) => {
                    addFiles(e.target.files);
                    e.target.value = '';
                  }}
                />
                <button
                  type="button"
                  className="btn-icon"
                  onClick={() => fileInputRef.current?.click()}
                  aria-label="Attach files"
                  title="Attach files"
                >
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.75"
                    width="18"
                    height="18"
                    aria-hidden="true"
                  >
                    <rect x="4.5" y="4.5" width="15" height="15" rx="3.5" />
                    <path d="M12 8.75v6.5M8.75 12h6.5" strokeLinecap="square" />
                  </svg>
                </button>
                <textarea
                  placeholder="Type a message, or drop a file..."
                  value={messageInput}
                  onChange={(e) => setMessageInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && !e.shiftKey) {
                      e.preventDefault();
                      handleSendMessage(e);
                    }
                  }}
                  onPaste={(e) => {
                    if (e.clipboardData.files.length > 0) {
                      e.preventDefault();
                      addFiles(e.clipboardData.files);
                    }
                  }}
                  disabled={sending}
                  rows={1}
                />
                <Button
                  type="submit"
                  variant="primary"
                  loading={sending}
                  disabled={!messageInput.trim() && !attachments.some(isSendable)}
                >
                  Send
                </Button>
              </div>
            </form>
          </>
        ) : selectedChatId && chatError ? (
          <div className="chat-placeholder">
            <div className="chats-error">{chatError}</div>
          </div>
        ) : selectedChatId ? (
          <div className="chat-placeholder">
            <div className="chats-loading">
              <span className="spinner" /> Loading...
            </div>
          </div>
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
            <Button
              variant="primary"
              onClick={() => {
                setOperationError(null);
                setShowNewChatModal(true);
              }}
            >
              Start New Chat
            </Button>
          </div>
        )}
      </div>

      {/* New Chat Modal */}
      <Modal isOpen={showNewChatModal} onClose={() => setShowNewChatModal(false)} title="New Chat">
        <form onSubmit={handleCreateChat}>
          {operationError && <div className="modal-error">{operationError}</div>}
          <Select
            label="Select Model"
            value={newChatModel}
            onChange={(e: React.ChangeEvent<HTMLSelectElement>) => setNewChatModel(e.target.value)}
            placeholder="Choose a model..."
            options={models.map((model) => ({ value: model.name, label: model.name }))}
          />
          <Checkbox
            label="Agent mode"
            helpText="Let replies search this workspace's knowledge, sources, projects and tasks before answering. Requires a model that supports tool calling."
            checked={newChatAgent}
            onCheckedChange={setNewChatAgent}
          />
          {newChatAgent && (
            <Checkbox
              label="Sandboxed"
              helpText="Sandboxed, the agent can only read this workspace. Unsandboxed, it can also run shell commands and read and write files on the machine running the server, as the user that runs it. Those changes are real and are not undone when the chat ends."
              checked={newChatSandboxed}
              onCheckedChange={setNewChatSandboxed}
            />
          )}
          <div className="modal-actions">
            <Button variant="secondary" onClick={() => setShowNewChatModal(false)}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={!newChatModel}>
              Create Chat
            </Button>
          </div>
        </form>
      </Modal>

      {/* Delete Confirmation Modal */}
      <Modal
        isOpen={deleteConfirm !== null}
        onClose={() => setDeleteConfirm(null)}
        title="Delete Chat"
      >
        <p>Are you sure you want to delete this chat? This action cannot be undone.</p>
        <div className="modal-actions">
          <Button variant="secondary" onClick={() => setDeleteConfirm(null)}>
            Cancel
          </Button>
          <Button variant="danger" onClick={() => deleteConfirm && handleDeleteChat(deleteConfirm)}>
            Delete
          </Button>
        </div>
      </Modal>
    </div>
  );
}
