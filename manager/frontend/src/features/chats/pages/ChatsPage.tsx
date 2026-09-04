import { Button, Checkbox, EmptyState, Modal, Select, Tabs, TabsList, TabsTrigger } from '@zone/ui';
import { type FormEvent, useCallback, useEffect, useRef, useState } from 'react';
import { useAuth } from '../../../features/auth';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { useModels } from '../../models';
import { isProtectedArtifactUrl } from '../api/protectedImages';
import { AuthenticatedImage, Generation, MessageContent, ToolTrace } from '../components';
import { useChat, useChatSearch, useChats } from '../hooks';
import type { ChatSearchResult } from '../types';
import {
  type Attachment,
  attachmentMetadata,
  buildMessageWithAttachments,
  formatBytes,
  formatDate,
  imageAttachments,
  isSendable,
  isStartingImage,
  readAttachment,
  sourceAttachment,
} from '../utils';
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
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [renameId, setRenameId] = useState<string | null>(null);
  const [title, setTitle] = useState('');
  const [renameError, setRenameError] = useState<string | null>(null);
  const [renaming, setRenaming] = useState(false);
  const renamePending = useRef(false);
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

  const useAsStartingImage = (image: { name: string; mime: string; url: string }) => {
    const next = sourceAttachment(image);
    setAttachments((prev) => (prev.some((item) => item.id === next.id) ? prev : [...prev, next]));
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
    renameChat,
    updateTitle: updateListTitle,
  } = useChats({ archived: showArchived });

  const {
    chat: activeChat,
    error: chatError,
    streaming,
    status: chatStatus,
    sendMessage: sendMessageFn,
    cancelGeneration,
    setAgentEnabled: setAgentEnabledFn,
    updateTitle,
  } = useChat(selectedChatId, updateListTitle);

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

  // biome-ignore lint/correctness/useExhaustiveDependencies: keep new messages and generation feedback visible
  useEffect(() => {
    scrollToBottom();
  }, [activeChat?.messages, chatError, chatStatus, streaming, scrollToBottom]);

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
        automatic_title: true,
        model_name: newChatModel,
        agent_enabled: newChatAgent,
      });
      setShowNewChatModal(false);
      setNewChatModel('');
      setNewChatAgent(false);
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

  const handleRename = async (event: FormEvent): Promise<void> => {
    event.preventDefault();
    if (!renameId || renamePending.current || !isAuthenticated) return;
    const trimmed = title.trim();
    if (!trimmed) {
      setRenameError('Enter a chat name');
      return;
    }
    renamePending.current = true;
    setRenaming(true);
    setRenameError(null);
    try {
      const updated = await renameChat(renameId, trimmed);
      updateTitle(updated.id, updated.title);
      setRenameId(null);
    } catch (error) {
      setRenameError(error instanceof Error ? error.message : 'Failed to rename chat');
    } finally {
      renamePending.current = false;
      setRenaming(false);
    }
  };

  const handleSendMessage = async (e: FormEvent) => {
    e.preventDefault();
    const sendable = attachments.filter(isSendable);
    if (!isAuthenticated || !displayedChat || sending || streaming) return;
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
    <div className={`page page--workspace chats-page ${selectedChatId ? 'has-chat' : ''}`}>
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
                onKeyDown={(e) =>
                  e.target === e.currentTarget && e.key === 'Enter' && selectChat(chat.id)
                }
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
                  <button
                    className="btn btn-icon btn-xs"
                    type="button"
                    title="Rename"
                    aria-label={`Rename ${chat.title}`}
                    onClick={(event) => {
                      event.stopPropagation();
                      setRenameId(chat.id);
                      setTitle(chat.title);
                      setRenameError(null);
                    }}
                  >
                    <svg
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      width="14"
                      height="14"
                      aria-hidden="true"
                    >
                      <path d="m16 3 5 5-12 12H4v-5L16 3ZM14 5l5 5" />
                    </svg>
                  </button>
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
        {selectedChatId && !displayedChat && (
          <Button
            variant="ghost"
            size="sm"
            className="chat-back chat-back-state"
            onClick={() => setSelectedChatId(null)}
          >
            Back to chats
          </Button>
        )}
        {displayedChat ? (
          <>
            <div className="chat-header">
              <Button
                variant="ghost"
                size="sm"
                className="chat-back"
                onClick={() => setSelectedChatId(null)}
              >
                Back to chats
              </Button>
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
                        <div
                          className="message-images"
                          data-image-count={Math.min(images.length, 3)}
                        >
                          {images.map((a, index) => {
                            const starting = attachments.some(
                              (item) => item.id === `source:${a.url}`
                            );
                            return (
                              <div key={a.url} className="message-image-frame">
                                <AuthenticatedImage
                                  src={a.url}
                                  alt={a.name || `Generated image ${index + 1}`}
                                  openLabel={`Open ${a.name || `image ${index + 1}`} full size`}
                                  linkClassName="message-image-link"
                                  title="Open full size"
                                  loading="lazy"
                                  decoding="async"
                                  data-testid="message-image"
                                />
                                <button
                                  type="button"
                                  className="message-image-use"
                                  onClick={() => useAsStartingImage(a)}
                                  disabled={starting}
                                >
                                  {starting ? 'Added as starting image' : 'Use as starting image'}
                                </button>
                              </div>
                            );
                          })}
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
              {chatError ? (
                <div className="chats-error" role="alert">
                  {chatError}
                </div>
              ) : null}
              {streaming || chatStatus ? (
                <Generation key={selectedChatId} status={chatStatus || 'Generating response…'} />
              ) : null}
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
                        isProtectedArtifactUrl(attachment.url) ? (
                          <AuthenticatedImage
                            src={attachment.url}
                            alt=""
                            className="attachment-chip-thumb"
                            linked={false}
                            compact
                          />
                        ) : (
                          <img className="attachment-chip-thumb" src={attachment.url} alt="" />
                        )
                      ) : null}
                      <span className="attachment-chip-name">{attachment.name}</span>
                      <span className="attachment-chip-size">
                        {attachment.rejected ? (
                          <span className="attachment-chip-note">{attachment.rejected}</span>
                        ) : isStartingImage(attachment) ? (
                          <span className="attachment-chip-source">Starting image</span>
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
              {attachments.some((attachment) => attachment.url && !attachment.rejected) ? (
                <p className="message-form-hint">
                  Ask to generate or edit and this image will be the starting point.
                </p>
              ) : null}

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
                {streaming ? (
                  <Button type="button" variant="secondary" onClick={cancelGeneration}>
                    Stop
                  </Button>
                ) : (
                  <Button
                    type="submit"
                    variant="primary"
                    loading={sending}
                    disabled={!messageInput.trim() && !attachments.some(isSendable)}
                  >
                    Send
                  </Button>
                )}
              </div>
            </form>
          </>
        ) : selectedChatId && chatError ? (
          <div className="chat-placeholder">
            <div className="chats-error" role="alert">
              {chatError}
            </div>
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
                width="40"
                height="40"
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
        <form className="ui-form" onSubmit={handleCreateChat}>
          {operationError && <div className="modal-error">{operationError}</div>}
          <Select
            label="Select Model"
            value={newChatModel}
            onChange={(e: React.ChangeEvent<HTMLSelectElement>) => setNewChatModel(e.target.value)}
            placeholder="Choose a model..."
            helpText="Embedding models are not available for chat."
            options={models
              .filter((model) => model.completion !== false)
              .map((model) => ({ value: model.name, label: model.name }))}
          />
          <Checkbox
            label="Agent mode"
            helpText="Let replies search workspace content, check connected GitHub data and manage workspace work, run shell commands and read and write server files when requested. Requires a model that supports tool calling."
            checked={newChatAgent}
            onCheckedChange={setNewChatAgent}
          />
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

      <Modal
        isOpen={renameId !== null}
        onClose={() => {
          if (!renamePending.current) setRenameId(null);
        }}
        title="Rename chat"
      >
        <form onSubmit={handleRename}>
          <label htmlFor="chat-name">Chat name</label>
          <input
            id="chat-name"
            className="form-input"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            disabled={renaming}
            aria-invalid={renameError ? true : undefined}
            aria-describedby={renameError ? 'rename-error' : undefined}
          />
          {renameError && (
            <div id="rename-error" className="chats-error" role="alert">
              {renameError}
            </div>
          )}
          <div className="modal-actions">
            <Button variant="secondary" disabled={renaming} onClick={() => setRenameId(null)}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={renaming || !title.trim()}>
              {renaming ? 'Saving…' : 'Save name'}
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
