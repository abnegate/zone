import { parse } from '../validation';
import type {
  Chat,
  ChatSearchOptions,
  ChatSearchResponse,
  ChatWithMessages,
  CreateChatRequest,
  Message,
  SendMessageRequest,
} from '../features/chats/types';
import {
  ChatResponseSchema,
  ChatsResponseSchema,
  ChatSearchResponseSchema,
  MessageResponseSchema,
  MessagesResponseSchema,
} from '../features/chats/schemas';
import { API_BASE } from './client';

class ChatsApi {
  private getAccessToken: () => string | null = () => null;

  setGetAccessToken(fn: () => string | null) {
    this.getAccessToken = fn;
  }

  private getHeaders(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
    };
    const token = this.getAccessToken();
    if (token) {
      headers.Authorization = `Bearer ${token}`;
    }
    return headers;
  }

  async getChats(archived?: boolean): Promise<Chat[]> {
    const params = archived !== undefined ? `?archived=${archived}` : '';
    const response = await fetch(`${API_BASE}/api/chats${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch chats: ${response.status}`);
    }
    const data = parse(ChatsResponseSchema, await response.json());
    return data.chats;
  }

  async getChat(id: string): Promise<ChatWithMessages> {
    const response = await fetch(`${API_BASE}/api/chats/${id}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch chat: ${response.status}`);
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
      throw new Error(`Failed to create chat: ${response.status}`);
    }
    const data = parse(ChatResponseSchema, await response.json());
    return data.chat;
  }

  async updateChatTitle(id: string, title: string): Promise<Chat> {
    const response = await fetch(`${API_BASE}/api/chats/${id}`, {
      method: 'PATCH',
      headers: this.getHeaders(),
      body: JSON.stringify({ title }),
    });
    if (!response.ok) {
      throw new Error(`Failed to update chat: ${response.status}`);
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
      throw new Error(`Failed to delete chat: ${response.status}`);
    }
  }

  async archiveChat(id: string): Promise<Chat> {
    const response = await fetch(`${API_BASE}/api/chats/${id}/archive`, {
      method: 'POST',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to archive chat: ${response.status}`);
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
      throw new Error(`Failed to unarchive chat: ${response.status}`);
    }
    const data = parse(ChatResponseSchema, await response.json());
    return data.chat;
  }

  async getMessages(chatId: string): Promise<Message[]> {
    const response = await fetch(`${API_BASE}/api/chats/${chatId}/messages`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch messages: ${response.status}`);
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
      throw new Error(`Failed to send message: ${response.status}`);
    }
    const data = parse(MessageResponseSchema, await response.json());
    return data.message;
  }

  async deleteMessage(chatId: string, messageId: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/chats/${chatId}/messages/${messageId}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to delete message: ${response.status}`);
    }
  }

  async searchChatMessages(options: ChatSearchOptions): Promise<ChatSearchResponse> {
    const params = new URLSearchParams();
    params.set('query', options.query);
    if (options.chat_id) params.set('chat_id', options.chat_id);
    if (options.limit !== undefined) params.set('limit', options.limit.toString());

    const response = await fetch(`${API_BASE}/api/chats/search?${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to search chat messages: ${response.status}`);
    }
    return parse(ChatSearchResponseSchema, await response.json());
  }
}

export const chatsApi = new ChatsApi();
