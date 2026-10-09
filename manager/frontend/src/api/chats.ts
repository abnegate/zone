import {
  ChatResponseSchema,
  ChatSearchResponseSchema,
  ChatSourcesResponseSchema,
  ChatsResponseSchema,
  ContextResponseSchema,
  MessageResponseSchema,
  MessagesResponseSchema,
} from '../features/chats/schemas';
import type {
  Chat,
  ChatSearchOptions,
  ChatSearchResponse,
  ChatSource,
  ChatWithMessages,
  ContextUsage,
  CreateChatRequest,
  Message,
  SendMessageRequest,
  UpdateChatRequest,
} from '../features/chats/types';
import { parse } from '../validation';
import { ApiError } from './ApiError';
import { API_BASE } from './client';
import { deviceHeaders } from './device';

class ChatsApi {
  private getAccessToken: () => string | null = () => null;
  private ensureAccessTokenFn: (() => Promise<string | null>) | null = null;

  setGetAccessToken(fn: () => string | null) {
    this.getAccessToken = fn;
  }

  setEnsureAccessToken(fn: (() => Promise<string | null>) | null) {
    this.ensureAccessTokenFn = fn;
  }

  async ensureAccessToken(): Promise<string | null> {
    if (this.ensureAccessTokenFn) {
      return this.ensureAccessTokenFn();
    }
    return this.getAccessToken();
  }

  private getHeaders(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
      ...deviceHeaders(),
    };
    const token = this.getAccessToken();
    if (token) {
      headers.Authorization = `Bearer ${token}`;
    }
    return headers;
  }

  async previewContext(
    id: string,
    request: SendMessageRequest,
    signal: AbortSignal
  ): Promise<ContextUsage> {
    const response = await fetch(`${API_BASE}/api/chats/${encodeURIComponent(id)}/context`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
      signal,
    });
    if (!response.ok)
      throw new Error('Context preview is unavailable. Sending is still available.');
    return parse(ContextResponseSchema, await response.json()).context;
  }

  async getChats(workspaceId: string, archived?: boolean): Promise<Chat[]> {
    const params = new URLSearchParams({ workspace_id: workspaceId });
    if (archived !== undefined) {
      params.set('archived', String(archived));
    }
    const response = await fetch(`${API_BASE}/api/chats?${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to fetch chats');
    }
    const data = parse(ChatsResponseSchema, await response.json());
    return data.chats;
  }

  async getChat(id: string): Promise<ChatWithMessages> {
    const response = await fetch(`${API_BASE}/api/chats/${id}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to fetch chat');
    }
    const data = parse(ChatResponseSchema, await response.json());
    return data.chat;
  }

  async createChat(request: CreateChatRequest): Promise<Chat> {
    const response = await fetch(`${API_BASE}/api/chats`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to create chat');
    }
    const data = parse(ChatResponseSchema, await response.json());
    return data.chat;
  }

  async updateChat(id: string, request: UpdateChatRequest): Promise<Chat> {
    const response = await fetch(`${API_BASE}/api/chats/${id}`, {
      method: 'PUT',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to update chat');
    }
    const data = parse(ChatResponseSchema, await response.json());
    return data.chat;
  }

  async deleteChat(id: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/chats/${id}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to delete chat');
    }
  }

  async archiveChat(id: string): Promise<Chat> {
    const response = await fetch(`${API_BASE}/api/chats/${id}/archive`, {
      method: 'POST',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to archive chat');
    }
    const data = parse(ChatResponseSchema, await response.json());
    return data.chat;
  }

  async unarchiveChat(id: string): Promise<Chat> {
    const response = await fetch(`${API_BASE}/api/chats/${id}/unarchive`, {
      method: 'POST',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to unarchive chat');
    }
    const data = parse(ChatResponseSchema, await response.json());
    return data.chat;
  }

  async getMessages(chatId: string): Promise<Message[]> {
    const response = await fetch(`${API_BASE}/api/chats/${chatId}/messages`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to fetch messages');
    }
    const data = parse(MessagesResponseSchema, await response.json());
    return data.messages;
  }

  async sendMessage(chatId: string, request: SendMessageRequest): Promise<Message> {
    const response = await fetch(`${API_BASE}/api/chats/${chatId}/messages`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to send message');
    }
    const data = parse(MessageResponseSchema, await response.json());
    return data.message;
  }

  createChatWebSocket(chatId: string): WebSocket {
    let wsUrl: string;
    if (API_BASE) {
      const wsBase = API_BASE.replace(/^http/, 'ws');
      wsUrl = `${wsBase}/ws/chats/${encodeURIComponent(chatId)}`;
    } else {
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      wsUrl = `${protocol}//${window.location.host}/ws/chats/${encodeURIComponent(chatId)}`;
    }
    return new WebSocket(wsUrl);
  }

  chatAccessToken(): string | null {
    return this.getAccessToken();
  }

  async deleteMessage(chatId: string, messageId: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/chats/${chatId}/messages/${messageId}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to delete message');
    }
  }

  async getChatSources(chatId: string): Promise<ChatSource[]> {
    const response = await fetch(`${API_BASE}/api/chats/${encodeURIComponent(chatId)}/sources`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to load attached sources');
    }
    return parse(ChatSourcesResponseSchema, await response.json()).sources;
  }

  async setChatSources(chatId: string, sourceIds: string[]): Promise<ChatSource[]> {
    const response = await fetch(`${API_BASE}/api/chats/${encodeURIComponent(chatId)}/sources`, {
      method: 'PUT',
      headers: this.getHeaders(),
      body: JSON.stringify({ source_ids: sourceIds }),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to attach sources');
    }
    return parse(ChatSourcesResponseSchema, await response.json()).sources;
  }

  async searchChatMessages(options: ChatSearchOptions): Promise<ChatSearchResponse> {
    const params = new URLSearchParams();
    params.set('query', options.query);
    params.set('workspace_id', options.workspace_id);
    if (options.chat_id) params.set('chat_id', options.chat_id);
    if (options.limit !== undefined) params.set('limit', options.limit.toString());

    const response = await fetch(`${API_BASE}/api/chats/search?${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw await ApiError.from(response, 'Failed to search chat messages');
    }
    return parse(ChatSearchResponseSchema, await response.json());
  }
}

export const chatsApi = new ChatsApi();
