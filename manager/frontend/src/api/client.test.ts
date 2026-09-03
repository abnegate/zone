import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import type { Limits, Plan, Subscription, Usage } from '../types';
import { client } from './client';

// Mock fetch globally
const mockFetch = mock();
global.fetch = mockFetch as typeof fetch;

// Mock WebSocket
class MockWebSocket {
  url: string;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  readyState = 1;

  constructor(url: string) {
    this.url = url;
  }

  send = mock();
  close = mock();
}

// Every test file shares one process, so a global left swapped out here reaches
// suites that expect a real WebSocket.
const realWebSocket = global.WebSocket;

beforeAll(() => {
  (global as unknown as { WebSocket: typeof MockWebSocket }).WebSocket = MockWebSocket;
});

afterAll(() => {
  global.WebSocket = realWebSocket;
});

describe('Client', () => {
  beforeEach(() => {
    mockFetch.mockClear();
    client.setAccessToken(null);
  });

  describe('setAccessToken', () => {
    it('sets the access token for authenticated requests', async () => {
      client.setAccessToken('test-token');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        text: async () => JSON.stringify({ models: [] }),
      });

      await client.getModels();

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/models',
        expect.objectContaining({
          headers: expect.objectContaining({
            Authorization: 'Bearer test-token',
          }),
        })
      );
    });

    it('does not include Authorization header when token is null', async () => {
      client.setAccessToken(null);

      mockFetch.mockResolvedValueOnce({
        ok: true,
        text: async () => JSON.stringify({ models: [] }),
      });

      await client.getModels();

      const headers = mockFetch.mock.calls[0][1].headers;
      expect(headers.Authorization).toBeUndefined();
    });
  });

  describe('getModels', () => {
    it('fetches models successfully', async () => {
      const mockModels = {
        models: [{ name: 'llama2', size: 3800000000, modified_at: '2024-01-01' }],
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        text: async () => JSON.stringify(mockModels),
      });

      const result = await client.getModels();

      expect(result).toEqual(mockModels);
      expect(mockFetch).toHaveBeenCalledWith('/api/models', expect.any(Object));
    });

    it('returns empty models array for empty response', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        text: async () => '',
      });

      const result = await client.getModels();

      expect(result).toEqual({ models: [] });
    });

    it('throws error for invalid JSON response', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        text: async () => 'invalid json',
      });

      await expect(client.getModels()).rejects.toThrow('Invalid response from server');
    });

    it('throws error on failed request', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 500,
      });

      await expect(client.getModels()).rejects.toThrow('Failed to fetch models: 500');
    });
  });

  describe('deleteModel', () => {
    it('deletes a model successfully', async () => {
      mockFetch.mockResolvedValueOnce({ ok: true });

      await client.deleteModel('llama2');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/models/llama2',
        expect.objectContaining({ method: 'DELETE' })
      );
    });

    it('encodes model name in URL', async () => {
      mockFetch.mockResolvedValueOnce({ ok: true });

      await client.deleteModel('model/with/slashes');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/models/model%2Fwith%2Fslashes',
        expect.any(Object)
      );
    });

    it('throws error on failed delete', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.deleteModel('nonexistent')).rejects.toThrow(
        'Failed to delete model: 404'
      );
    });
  });

  describe('browseModels', () => {
    it('browses models with default parameters', async () => {
      const mockResponse = { models: [{ name: 'llama2:7b', size: 3800000000 }], next_cursor: null };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockResponse,
      });

      const result = await client.browseModels('ollama');

      expect(result).toEqual(mockResponse);
      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/models?'),
        expect.any(Object)
      );
    });

    it('includes query parameters with cursor', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ models: [], next_cursor: null }),
      });

      await client.browseModels('huggingface', 'llama', 'cursor-abc123', 10);

      const url = mockFetch.mock.calls[0][0];
      expect(url).toContain('source=huggingface');
      expect(url).toContain('q=llama');
      expect(url).toContain('cursor=cursor-abc123');
      expect(url).toContain('limit=10');
    });

    it('omits cursor when null', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ models: [], next_cursor: null }),
      });

      await client.browseModels('ollama', 'test', null, 20);

      const url = mockFetch.mock.calls[0][0];
      expect(url).not.toContain('cursor=');
    });

    it('includes sort and filter parameters', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ models: [], next_cursor: null }),
      });

      await client.browseModels('ollama', 'llama', null, 20, {
        sort: 'name_asc',
        family: 'llama',
        size: 'medium',
      });

      const url = mockFetch.mock.calls[0][0];
      expect(url).toContain('sort=name_asc');
      expect(url).toContain('family=llama');
      expect(url).toContain('size=medium');
    });
  });

  describe('getModelInfo', () => {
    it('fetches model info successfully', async () => {
      const mockInfo = { content: '# Model README', gguf_size: 4000000000 };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockInfo,
      });

      const result = await client.getModelInfo('author/model');

      expect(result).toEqual(mockInfo);
      expect(mockFetch).toHaveBeenCalledWith('/api/models/author/model', expect.any(Object));
    });
  });

  describe('createPullWebSocket', () => {
    it('creates WebSocket with correct URL', () => {
      const ws = client.createPullWebSocket('llama2');

      expect(ws).toBeInstanceOf(MockWebSocket);
      expect((ws as unknown as MockWebSocket).url).toContain('ws://');
      expect((ws as unknown as MockWebSocket).url).toContain('/ws/pull?model=llama2');
    });
  });

  describe('Chats API', () => {
    const testWorkspaceId = 'ws-test-123';
    const mockChat = {
      id: '1',
      title: 'Test Chat',
      model_name: 'llama2',
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      archived: false,
    };

    const mockMessage = {
      id: '1',
      chat_id: 'chat-1',
      role: 'user' as const,
      content: 'Hello',
      created_at: '2024-01-01T00:00:00Z',
    };

    it('getChats fetches all chats', async () => {
      const mockChats = { chats: [mockChat] };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockChats,
      });

      const result = await client.getChats(testWorkspaceId);

      expect(result).toEqual(mockChats.chats);
    });

    it('getChats with archived filter', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ chats: [] }),
      });

      await client.getChats(testWorkspaceId, true);

      expect(mockFetch).toHaveBeenCalledWith(
        `/api/chats?workspace_id=${testWorkspaceId}&archived=true`,
        expect.any(Object)
      );
    });

    it('getChat fetches single chat', async () => {
      const mockChatWithMessages = { chat: { ...mockChat, messages: [mockMessage] } };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockChatWithMessages,
      });

      const result = await client.getChat('1');

      expect(result).toEqual(mockChatWithMessages.chat);
    });

    it('createChat creates new chat', async () => {
      const mockChatWithMessages = { chat: { ...mockChat, messages: [] } };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockChatWithMessages,
      });

      const result = await client.createChat({ model_name: 'llama2' });

      expect(result).toEqual(mockChatWithMessages.chat);
      expect(mockFetch).toHaveBeenCalledWith(
        '/api/chats',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ model_name: 'llama2' }),
        })
      );
    });

    it('updateChatTitle updates chat', async () => {
      const updatedChat = { ...mockChat, title: 'Updated', messages: [] };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ chat: updatedChat }),
      });

      await client.updateChatTitle('1', 'Updated');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/chats/1',
        expect.objectContaining({
          method: 'PATCH',
          body: JSON.stringify({ title: 'Updated' }),
        })
      );
    });

    it('deleteChat deletes chat', async () => {
      mockFetch.mockResolvedValueOnce({ ok: true });

      await client.deleteChat('1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/chats/1',
        expect.objectContaining({ method: 'DELETE' })
      );
    });

    it('archiveChat archives chat', async () => {
      const archivedChat = { ...mockChat, archived: true, messages: [] };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ chat: archivedChat }),
      });

      await client.archiveChat('1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/chats/1/archive',
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('unarchiveChat unarchives chat', async () => {
      const unarchivedChat = { ...mockChat, archived: false, messages: [] };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ chat: unarchivedChat }),
      });

      await client.unarchiveChat('1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/chats/1/unarchive',
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('getMessages fetches messages', async () => {
      const mockMessages = { messages: [mockMessage] };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockMessages,
      });

      const result = await client.getMessages('chat-1');

      expect(result).toEqual(mockMessages.messages);
    });

    it('sendMessage sends a message', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ message: mockMessage }),
      });

      await client.sendMessage('chat-1', { content: 'Hello' });

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/chats/chat-1/messages',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ content: 'Hello' }),
        })
      );
    });

    it('deleteMessage deletes a message', async () => {
      mockFetch.mockResolvedValueOnce({ ok: true });

      await client.deleteMessage('chat-1', 'msg-1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/chats/chat-1/messages/msg-1',
        expect.objectContaining({ method: 'DELETE' })
      );
    });

    describe('searchChatMessages', () => {
      const mockSearchResult = {
        message_id: 'msg-1',
        chat_id: 'chat-1',
        chat_title: 'Test Chat',
        content: 'This is a test message about TypeScript',
        snippet: '...test message about TypeScript...',
        relevance_score: 0.95,
        created_at: '2024-01-01T00:00:00Z',
      };

      it('searches messages with query only', async () => {
        const mockResponse = {
          results: [mockSearchResult],
          total: 1,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        const result = await client.searchChatMessages({ query: 'TypeScript' });

        expect(result).toEqual(mockResponse);
        expect(mockFetch).toHaveBeenCalledWith(
          expect.stringContaining('/api/chats/search?'),
          expect.any(Object)
        );
        expect(mockFetch).toHaveBeenCalledWith(
          expect.stringContaining('query=TypeScript'),
          expect.any(Object)
        );
      });

      it('searches messages with chat_id filter', async () => {
        const mockResponse = {
          results: [mockSearchResult],
          total: 1,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        await client.searchChatMessages({ query: 'test', chat_id: 'chat-1' });

        const url = mockFetch.mock.calls[0][0];
        expect(url).toContain('query=test');
        expect(url).toContain('chat_id=chat-1');
      });

      it('searches messages with limit', async () => {
        const mockResponse = {
          results: [mockSearchResult],
          total: 1,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        await client.searchChatMessages({ query: 'test', limit: 10 });

        const url = mockFetch.mock.calls[0][0];
        expect(url).toContain('query=test');
        expect(url).toContain('limit=10');
      });

      it('searches messages with all options', async () => {
        const mockResponse = {
          results: [mockSearchResult],
          total: 1,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        await client.searchChatMessages({
          query: 'test',
          chat_id: 'chat-1',
          limit: 5,
        });

        const url = mockFetch.mock.calls[0][0];
        expect(url).toContain('query=test');
        expect(url).toContain('chat_id=chat-1');
        expect(url).toContain('limit=5');
      });

      it('returns empty results when no matches found', async () => {
        const mockResponse = {
          results: [],
          total: 0,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        const result = await client.searchChatMessages({ query: 'nonexistent' });

        expect(result.results).toHaveLength(0);
        expect(result.total).toBe(0);
      });

      it('returns multiple results ordered by relevance', async () => {
        const mockResponse = {
          results: [
            { ...mockSearchResult, message_id: 'msg-1', relevance_score: 0.95 },
            { ...mockSearchResult, message_id: 'msg-2', relevance_score: 0.85 },
            { ...mockSearchResult, message_id: 'msg-3', relevance_score: 0.75 },
          ],
          total: 3,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        const result = await client.searchChatMessages({ query: 'TypeScript' });

        expect(result.results).toHaveLength(3);
        expect(result.results[0].relevance_score).toBeGreaterThanOrEqual(
          result.results[1].relevance_score
        );
        expect(result.results[1].relevance_score).toBeGreaterThanOrEqual(
          result.results[2].relevance_score
        );
      });

      it('throws error on failed request', async () => {
        mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

        await expect(client.searchChatMessages({ query: 'test' })).rejects.toThrow(
          'Failed to search chat messages: 500'
        );
      });

      it('includes Authorization header when authenticated', async () => {
        client.setAccessToken('test-token');
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ results: [], total: 0 }),
        });

        await client.searchChatMessages({ query: 'test' });

        expect(mockFetch).toHaveBeenCalledWith(
          expect.any(String),
          expect.objectContaining({
            headers: expect.objectContaining({
              Authorization: 'Bearer test-token',
            }),
          })
        );
      });
    });
  });

  describe('Projects API', () => {
    const testWorkspaceId = 'ws-test-123';
    const mockProject = {
      id: '1',
      name: 'Test',
      description: null,
      status: 'active' as const,
      github_repo_url: null,
      source_id: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    };

    it('getProjects fetches projects', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ projects: [mockProject] }),
      });

      const result = await client.getProjects(testWorkspaceId);

      expect(result).toHaveLength(1);
    });

    it('getProjects with status filter', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ projects: [] }),
      });

      await client.getProjects(testWorkspaceId, 'active');

      expect(mockFetch).toHaveBeenCalledWith(
        `/api/projects?workspace_id=${testWorkspaceId}&status=active`,
        expect.any(Object)
      );
    });

    it('getProject fetches single project', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ project: mockProject }),
      });

      const result = await client.getProject('1');

      expect(result.id).toBe('1');
    });

    it('createProject creates project', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ project: { ...mockProject, name: 'New' } }),
      });

      await client.createProject({ name: 'New', description: 'Desc' });

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/projects',
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('updateProject updates project', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ project: { ...mockProject, name: 'Updated' } }),
      });

      await client.updateProject('1', { name: 'Updated' });

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/projects/1',
        expect.objectContaining({ method: 'PATCH' })
      );
    });

    it('deleteProject deletes project', async () => {
      mockFetch.mockResolvedValueOnce({ ok: true });

      await client.deleteProject('1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/projects/1',
        expect.objectContaining({ method: 'DELETE' })
      );
    });

    it('linkGitHub links GitHub repo', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          project: { ...mockProject, github_repo_url: 'https://github.com/test' },
        }),
      });

      await client.linkGitHub('1', 'https://github.com/test');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/projects/1/github',
        expect.objectContaining({ method: 'PUT' })
      );
    });

    it('unlinkGitHub unlinks GitHub repo', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ project: mockProject }),
      });

      await client.unlinkGitHub('1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/projects/1/github',
        expect.objectContaining({ method: 'DELETE' })
      );
    });
  });

  describe('Tasks API', () => {
    const testWorkspaceId = 'ws-test-123';
    const mockTask = {
      id: '1',
      workspace_id: testWorkspaceId,
      project_ids: ['p1'],
      title: 'Task',
      description: 'Do something',
      acceptance_criteria: null,
      status: 'created' as const,
      priority: 0,
      model_name: null,
      dependencies: [],
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      started_at: null,
      completed_at: null,
      is_agentic: false,
      github_repo_url: null,
      source_id: null,
      source_ids: [],
      queued_at: null,
      worker_id: null,
      pr_url: null,
      branch_name: null,
      pr_status: null,
      pr_created_at: null,
    };

    const mockTaskRun = {
      id: 'run-1',
      task_id: '1',
      status: 'running' as const,
      current_phase: null,
      progress_percent: 0,
      error_message: null,
      started_at: '2024-01-01T00:00:00Z',
      completed_at: null,
    };

    const mockTaskRunLog = {
      id: 'log-1',
      run_id: 'run-1',
      phase: 'init',
      agent_type: 'executor',
      level: 'info' as const,
      message: 'test',
      created_at: '2024-01-01T00:00:00Z',
    };

    it('getTasks fetches tasks', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ tasks: [mockTask] }),
      });

      const result = await client.getTasks(testWorkspaceId);

      expect(result).toHaveLength(1);
    });

    it('getTasks with filters', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ tasks: [] }),
      });

      await client.getTasks(testWorkspaceId, 'project-1', 'pending');

      const url = mockFetch.mock.calls[0][0];
      expect(url).toContain(`/api/workspaces/${testWorkspaceId}/tasks`);
      expect(url).toContain('project_id=project-1');
      expect(url).toContain('status=pending');
    });

    it('createTask creates task', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ task: mockTask }),
      });

      await client.createTask(testWorkspaceId, {
        project_ids: ['p1'],
        title: 'Task',
        description: 'Do something',
      });

      expect(mockFetch).toHaveBeenCalledWith(
        `/api/workspaces/${testWorkspaceId}/tasks`,
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('startTask starts task', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ run_id: 'run-1' }),
      });

      const result = await client.startTask('task-1');

      expect(result.run_id).toBe('run-1');
    });

    it('stopTask stops task', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({}),
      });

      await client.stopTask('task-1');

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/tasks/task-1/stop'),
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('getTaskRuns fetches runs', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ runs: [mockTaskRun] }),
      });

      const result = await client.getTaskRuns('task-1');

      expect(result).toHaveLength(1);
    });

    it('getTaskRunLogs fetches logs', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ logs: [mockTaskRunLog] }),
      });

      const result = await client.getTaskRunLogs('task-1', 'run-1');

      expect(result).toHaveLength(1);
    });
  });

  describe('Sources API', () => {
    const testWorkspaceId = 'ws-test-123';
    const mockSourceType = {
      id: 'github' as const,
      name: 'GitHub',
      category: 'file' as const,
      enabled: true,
    };

    const mockSource = {
      id: '1',
      name: 'Test',
      source_type: 'github' as const,
      category: 'file' as const,
      config: { owner: 'test', repo: 'test' },
      description: null,
      url: 'https://github.com/test/test',
      is_active: true,
      last_verified_at: null,
      last_error: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    };

    it('getSourceTypes fetches source types', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ types: [mockSourceType] }),
      });

      const result = await client.getSourceTypes();

      expect(result).toHaveLength(1);
    });

    it('getSources fetches sources', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ sources: [mockSource] }),
      });

      const result = await client.getSources(testWorkspaceId);

      expect(result).toHaveLength(1);
    });

    it('getSources with filters', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ sources: [] }),
      });

      await client.getSources(testWorkspaceId, 'github', true);

      const url = mockFetch.mock.calls[0][0];
      expect(url).toContain(`/api/workspaces/${testWorkspaceId}/sources`);
      expect(url).toContain('source_type=github');
      expect(url).toContain('is_active=true');
    });

    it('createSource creates source', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ source: mockSource }),
      });

      await client.createSource(testWorkspaceId, {
        name: 'Test',
        source_type: 'github',
        config: { owner: 'test', repo: 'test' },
      });

      expect(mockFetch).toHaveBeenCalledWith(
        `/api/workspaces/${testWorkspaceId}/sources`,
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('createSource accepts an unwrapped API response without category', async () => {
      const { category: _category, url: _url, ...unwrapped } = mockSource;
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ...unwrapped, url: null }),
      });

      const source = await client.createSource(testWorkspaceId, {
        name: 'Test',
        source_type: 'github',
        config: { owner: 'test', repo: 'test' },
      });

      expect(source.category).toBe('file');
      expect(source.url).toBe('https://github.com/test/test');
    });

    it('verifySource verifies source', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ success: true, message: 'Verified' }),
      });

      const result = await client.verifySource(testWorkspaceId, '1');

      expect(result.success).toBe(true);
    });
  });

  describe('Organizations API', () => {
    const mockOrganization = {
      id: '1',
      name: 'Org',
      slug: 'org',
      description: null,
      is_active: true,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    };

    it('getOrganizations fetches organizations', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ organizations: [mockOrganization] }),
      });

      const result = await client.getOrganizations();

      expect(result).toHaveLength(1);
    });

    it('createOrganization creates organization', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ organization: mockOrganization }),
      });

      await client.createOrganization({ name: 'New Org', slug: 'new-org' });

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/organizations',
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('updateOrganization updates organization', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ organization: { ...mockOrganization, name: 'Updated' } }),
      });

      await client.updateOrganization('1', { name: 'Updated' });

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/organizations/1',
        expect.objectContaining({ method: 'PATCH' })
      );
    });

    it('deleteOrganization deletes organization', async () => {
      mockFetch.mockResolvedValueOnce({ ok: true });

      await client.deleteOrganization('1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/organizations/1',
        expect.objectContaining({ method: 'DELETE' })
      );
    });
  });

  describe('Workspaces API', () => {
    const mockWorkspace = {
      id: '1',
      organization_id: 'org-1',
      name: 'Workspace',
      slug: 'ws',
      description: null,
      is_active: true,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    };

    it('getWorkspaces fetches workspaces', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ workspaces: [mockWorkspace] }),
      });

      const result = await client.getWorkspaces('org-1');

      expect(result).toHaveLength(1);
      expect(mockFetch).toHaveBeenCalledWith(
        '/api/organizations/org-1/workspaces',
        expect.any(Object)
      );
    });

    it('createWorkspace creates workspace', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ workspace: mockWorkspace }),
      });

      await client.createWorkspace('org-1', { name: 'New', slug: 'new' });

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/organizations/org-1/workspaces',
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('updateWorkspace updates workspace', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ workspace: { ...mockWorkspace, name: 'Updated' } }),
      });

      await client.updateWorkspace('org-1', 'ws-1', { name: 'Updated' });

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/organizations/org-1/workspaces/ws-1',
        expect.objectContaining({ method: 'PATCH' })
      );
    });

    it('deleteWorkspace deletes workspace', async () => {
      mockFetch.mockResolvedValueOnce({ ok: true });

      await client.deleteWorkspace('org-1', 'ws-1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/organizations/org-1/workspaces/ws-1',
        expect.objectContaining({ method: 'DELETE' })
      );
    });
  });

  describe('Workspace Theme API', () => {
    const mockTheme = {
      id: 'theme-1',
      workspace_id: 'ws-1',
      primary_color_light: '#007bff',
      secondary_color_light: '#6c757d',
      primary_color_dark: '#0d6efd',
      secondary_color_dark: '#adb5bd',
      font_family: 'system' as const,
      font_size_base: '16px',
      border_radius: 'medium' as const,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    };

    it('getWorkspaceTheme fetches theme', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ theme: mockTheme }),
      });

      const result = await client.getWorkspaceTheme('org-1', 'ws-1');

      expect(result.primary_color_light).toBe('#007bff');
    });

    it('updateWorkspaceTheme updates theme', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ theme: { ...mockTheme, primary_color_light: '#ff0000' } }),
      });

      await client.updateWorkspaceTheme('org-1', 'ws-1', { primary_color_light: '#ff0000' });

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/workspaces/ws-1/theme',
        expect.objectContaining({ method: 'PUT' })
      );
    });

    it('resetWorkspaceTheme resets theme', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ theme: mockTheme }),
      });

      await client.resetWorkspaceTheme('org-1', 'ws-1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/workspaces/ws-1/theme',
        expect.objectContaining({ method: 'DELETE' })
      );
    });
  });

  describe('Error Handling', () => {
    it('browseModels throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.browseModels('ollama')).rejects.toThrow('Failed to browse models: 500');
    });

    it('getModelInfo throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.getModelInfo('author/model')).rejects.toThrow(
        'Failed to fetch model info: 404'
      );
    });

    it('getChats throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 401 });

      await expect(client.getChats('ws-test')).rejects.toThrow('Failed to fetch chats: 401');
    });

    it('getChat throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.getChat('1')).rejects.toThrow('Failed to fetch chat: 404');
    });

    it('createChat throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(client.createChat({ model_name: 'llama2' })).rejects.toThrow(
        'Failed to create chat: 400'
      );
    });

    it('updateChatTitle throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.updateChatTitle('1', 'New Title')).rejects.toThrow(
        'Failed to update chat: 500'
      );
    });

    it('deleteChat throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.deleteChat('1')).rejects.toThrow('Failed to delete chat: 404');
    });

    it('archiveChat throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.archiveChat('1')).rejects.toThrow('Failed to archive chat: 500');
    });

    it('unarchiveChat throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.unarchiveChat('1')).rejects.toThrow('Failed to unarchive chat: 500');
    });

    it('getMessages throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.getMessages('chat-1')).rejects.toThrow('Failed to fetch messages: 404');
    });

    it('sendMessage throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(client.sendMessage('chat-1', { content: 'Test' })).rejects.toThrow(
        'Failed to send message: 400'
      );
    });

    it('deleteMessage throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.deleteMessage('chat-1', 'msg-1')).rejects.toThrow(
        'Failed to delete message: 404'
      );
    });

    it('getProjects throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.getProjects()).rejects.toThrow('Failed to fetch projects: 500');
    });

    it('getProject throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.getProject('1')).rejects.toThrow('Failed to fetch project: 404');
    });

    it('createProject throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(client.createProject({ name: 'Test', description: 'Desc' })).rejects.toThrow(
        'Failed to create project: 400'
      );
    });

    it('updateProject throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.updateProject('1', { name: 'Updated' })).rejects.toThrow(
        'Failed to update project: 500'
      );
    });

    it('deleteProject throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.deleteProject('1')).rejects.toThrow('Failed to delete project: 404');
    });

    it('linkGitHub throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(client.linkGitHub('1', 'https://github.com/test')).rejects.toThrow(
        'Failed to link GitHub: 400'
      );
    });

    it('unlinkGitHub throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.unlinkGitHub('1')).rejects.toThrow('Failed to unlink GitHub: 500');
    });

    it('getTasks throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.getTasks()).rejects.toThrow('Failed to fetch tasks: 500');
    });

    it('createTask throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(
        client.createTask({ project_id: 'p1', title: 'Task', description: 'Do something' })
      ).rejects.toThrow('Failed to create task: 400');
    });

    it('startTask throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.startTask('task-1')).rejects.toThrow('Failed to run task: 500');
    });

    it('stopTask throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.stopTask('task-1')).rejects.toThrow('Failed to cancel task run: 500');
    });

    it('getTaskRuns throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.getTaskRuns('task-1')).rejects.toThrow('Failed to fetch task runs: 404');
    });

    it('getTaskRunLogs throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.getTaskRunLogs('task-1', 'run-1')).rejects.toThrow(
        'Failed to fetch task run logs: 404'
      );
    });

    it('getSourceTypes throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.getSourceTypes()).rejects.toThrow('Failed to fetch source types: 500');
    });

    it('getSources throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.getSources()).rejects.toThrow('Failed to fetch sources: 500');
    });

    it('createSource throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(
        client.createSource({
          name: 'Test',
          source_type: 'github',
          config: { owner: 'test', repo: 'test' },
        })
      ).rejects.toThrow('Failed to create source: 400');
    });

    it('verifySource throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.verifySource('1')).rejects.toThrow('Failed to verify source: 500');
    });

    it('getOrganizations throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.getOrganizations()).rejects.toThrow('Failed to fetch organizations: 500');
    });

    it('createOrganization throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(client.createOrganization({ name: 'Org', slug: 'org' })).rejects.toThrow(
        'Failed to create organization: 400'
      );
    });

    it('updateOrganization throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.updateOrganization('1', { name: 'Updated' })).rejects.toThrow(
        'Failed to update organization: 500'
      );
    });

    it('deleteOrganization throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.deleteOrganization('1')).rejects.toThrow(
        'Failed to delete organization: 404'
      );
    });

    it('getWorkspaces throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.getWorkspaces('org-1')).rejects.toThrow(
        'Failed to fetch workspaces: 500'
      );
    });

    it('createWorkspace throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(client.createWorkspace('org-1', { name: 'WS', slug: 'ws' })).rejects.toThrow(
        'Failed to create workspace: 400'
      );
    });

    it('updateWorkspace throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.updateWorkspace('org-1', 'ws-1', { name: 'Updated' })).rejects.toThrow(
        'Failed to update workspace: 500'
      );
    });

    it('deleteWorkspace throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.deleteWorkspace('org-1', 'ws-1')).rejects.toThrow(
        'Failed to delete workspace: 404'
      );
    });

    it('getWorkspaceTheme throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.getWorkspaceTheme('org-1', 'ws-1')).rejects.toThrow(
        'Failed to fetch workspace theme: 500'
      );
    });

    it('updateWorkspaceTheme throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(client.updateWorkspaceTheme('org-1', 'ws-1', {})).rejects.toThrow(
        'Failed to update workspace theme: 400'
      );
    });

    it('resetWorkspaceTheme throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.resetWorkspaceTheme('org-1', 'ws-1')).rejects.toThrow(
        'Failed to reset workspace theme: 500'
      );
    });
  });

  describe('AI Settings API', () => {
    const mockAiSettings = {
      provider: 'openai' as const,
      has_litellm_key: false,
      litellm_host: null,
      has_openai_api_key: true,
      openai_base_url: null,
      has_anthropic_api_key: false,
      anthropic_base_url: null,
      bedrock_region: null,
      bedrock_use_iam_role: false,
      has_bedrock_credentials: false,
      model_fast: 'gpt-4o-mini',
      model_reasoning: 'gpt-4o',
      model_embedding: 'text-embedding-3-small',
    };

    describe('Organization AI Settings', () => {
      it('getOrgAiSettings fetches org AI settings', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockAiSettings,
        });

        const result = await client.getOrgAiSettings('org-1');

        expect(result.provider).toBe('openai');
        expect(result.has_openai_api_key).toBe(true);
        expect(mockFetch).toHaveBeenCalledWith(
          '/api/organizations/org-1/settings/ai',
          expect.any(Object)
        );
      });

      it('updateOrgAiSettings updates org AI settings', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockAiSettings,
        });

        await client.updateOrgAiSettings('org-1', {
          provider: 'openai',
          openai_api_key: 'sk-test',
          model_fast: 'gpt-4o-mini',
        });

        expect(mockFetch).toHaveBeenCalledWith(
          '/api/organizations/org-1/settings/ai',
          expect.objectContaining({ method: 'PUT' })
        );
      });

      it('resetOrgAiSettings resets org AI settings', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ ...mockAiSettings, provider: 'self_hosted' }),
        });

        await client.resetOrgAiSettings('org-1');

        expect(mockFetch).toHaveBeenCalledWith(
          '/api/organizations/org-1/settings/ai',
          expect.objectContaining({ method: 'DELETE' })
        );
      });
    });

    describe('Workspace AI Settings', () => {
      it('getWorkspaceAiSettings fetches workspace AI settings', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockAiSettings,
        });

        const result = await client.getWorkspaceAiSettings('org-1', 'ws-1');

        expect(result.provider).toBe('openai');
        expect(mockFetch).toHaveBeenCalledWith(
          '/api/organizations/org-1/workspaces/ws-1/settings/ai',
          expect.any(Object)
        );
      });

      it('updateWorkspaceAiSettings updates workspace AI settings', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockAiSettings,
        });

        await client.updateWorkspaceAiSettings('org-1', 'ws-1', {
          provider: 'anthropic',
          anthropic_api_key: 'sk-ant-test',
        });

        expect(mockFetch).toHaveBeenCalledWith(
          '/api/organizations/org-1/workspaces/ws-1/settings/ai',
          expect.objectContaining({ method: 'PUT' })
        );
      });

      it('resetWorkspaceAiSettings resets workspace AI settings', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockAiSettings,
        });

        await client.resetWorkspaceAiSettings('org-1', 'ws-1');

        expect(mockFetch).toHaveBeenCalledWith(
          '/api/organizations/org-1/workspaces/ws-1/settings/ai',
          expect.objectContaining({ method: 'DELETE' })
        );
      });

      it('getEffectiveAiSettings fetches effective AI settings', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockAiSettings,
        });

        const result = await client.getEffectiveAiSettings('org-1', 'ws-1');

        expect(result.provider).toBe('openai');
        expect(mockFetch).toHaveBeenCalledWith(
          '/api/organizations/org-1/workspaces/ws-1/settings/ai/effective',
          expect.any(Object)
        );
      });
    });

    describe('AI Settings Error Handling', () => {
      it('getOrgAiSettings throws on failed request', async () => {
        mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

        await expect(client.getOrgAiSettings('org-1')).rejects.toThrow(
          'Failed to fetch org AI settings: 500'
        );
      });

      it('updateOrgAiSettings throws on failed request', async () => {
        mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

        await expect(client.updateOrgAiSettings('org-1', {})).rejects.toThrow(
          'Failed to update org AI settings: 400'
        );
      });

      it('resetOrgAiSettings throws on failed request', async () => {
        mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

        await expect(client.resetOrgAiSettings('org-1')).rejects.toThrow(
          'Failed to reset org AI settings: 500'
        );
      });

      it('getWorkspaceAiSettings throws on failed request', async () => {
        mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

        await expect(client.getWorkspaceAiSettings('org-1', 'ws-1')).rejects.toThrow(
          'Failed to fetch workspace AI settings: 500'
        );
      });

      it('updateWorkspaceAiSettings throws on failed request', async () => {
        mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

        await expect(client.updateWorkspaceAiSettings('org-1', 'ws-1', {})).rejects.toThrow(
          'Failed to update workspace AI settings: 400'
        );
      });

      it('resetWorkspaceAiSettings throws on failed request', async () => {
        mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

        await expect(client.resetWorkspaceAiSettings('org-1', 'ws-1')).rejects.toThrow(
          'Failed to reset workspace AI settings: 500'
        );
      });

      it('getEffectiveAiSettings throws on failed request', async () => {
        mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

        await expect(client.getEffectiveAiSettings('org-1', 'ws-1')).rejects.toThrow(
          'Failed to fetch effective AI settings: 500'
        );
      });
    });
  });

  describe('Email Verification & Password Reset API', () => {
    describe('verifyEmail', () => {
      it('verifies email successfully', async () => {
        const mockResponse = { success: true, message: 'Email verified' };
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        const result = await client.verifyEmail('valid-token-12345');

        expect(result).toEqual(mockResponse);
        expect(mockFetch).toHaveBeenCalledWith(
          '/api/auth/verify-email',
          expect.objectContaining({
            method: 'POST',
            body: JSON.stringify({ token: 'valid-token-12345' }),
          })
        );
      });

      it('throws error with backend message on failed verification', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 400,
          json: async () => ({ message: 'Token expired' }),
        });

        await expect(client.verifyEmail('expired-token')).rejects.toThrow('Token expired');
      });

      it('throws error with status code when no backend message', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: async () => ({}),
        });

        await expect(client.verifyEmail('token')).rejects.toThrow('Failed to verify email: 500');
      });
    });

    describe('resendVerification', () => {
      it('resends verification email successfully', async () => {
        const mockResponse = { success: true, message: 'Verification email sent' };
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        const result = await client.resendVerification('user@example.com');

        expect(result).toEqual(mockResponse);
        expect(mockFetch).toHaveBeenCalledWith(
          '/api/auth/resend-verification',
          expect.objectContaining({
            method: 'POST',
            body: JSON.stringify({ email: 'user@example.com' }),
          })
        );
      });

      it('throws error with backend message on failed resend', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 429,
          json: async () => ({ message: 'Too many requests' }),
        });

        await expect(client.resendVerification('user@example.com')).rejects.toThrow(
          'Too many requests'
        );
      });

      it('throws error with status code when no backend message', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: async () => ({}),
        });

        await expect(client.resendVerification('user@example.com')).rejects.toThrow(
          'Failed to resend verification email: 500'
        );
      });
    });

    describe('forgotPassword', () => {
      it('sends password reset email successfully', async () => {
        const mockResponse = { success: true, message: 'Reset email sent' };
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        const result = await client.forgotPassword('user@example.com');

        expect(result).toEqual(mockResponse);
        expect(mockFetch).toHaveBeenCalledWith(
          '/api/auth/forgot-password',
          expect.objectContaining({
            method: 'POST',
            body: JSON.stringify({ email: 'user@example.com' }),
          })
        );
      });

      it('throws error with backend message on failed request', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
          json: async () => ({ message: 'Email not found' }),
        });

        await expect(client.forgotPassword('nonexistent@example.com')).rejects.toThrow(
          'Email not found'
        );
      });

      it('throws error with status code when no backend message', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: async () => ({}),
        });

        await expect(client.forgotPassword('user@example.com')).rejects.toThrow(
          'Failed to request password reset: 500'
        );
      });
    });

    describe('resetPassword', () => {
      it('resets password successfully', async () => {
        const mockResponse = { success: true, message: 'Password reset' };
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        const result = await client.resetPassword('reset-token-123', 'NewPassword123');

        expect(result).toEqual(mockResponse);
        expect(mockFetch).toHaveBeenCalledWith(
          '/api/auth/reset-password',
          expect.objectContaining({
            method: 'POST',
            body: JSON.stringify({ token: 'reset-token-123', new_password: 'NewPassword123' }),
          })
        );
      });

      it('throws error with backend message on failed reset', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 400,
          json: async () => ({ message: 'Invalid or expired token' }),
        });

        await expect(client.resetPassword('invalid-token', 'NewPassword123')).rejects.toThrow(
          'Invalid or expired token'
        );
      });

      it('throws error with status code when no backend message', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: async () => ({}),
        });

        await expect(client.resetPassword('token', 'NewPassword123')).rejects.toThrow(
          'Failed to reset password: 500'
        );
      });
    });
  });

  describe('Session Management', () => {
    describe('getSessions', () => {
      it('fetches sessions successfully', async () => {
        const mockSessions = {
          sessions: [
            {
              id: 'session-1',
              user_id: 'user-1',
              ip_address: '192.168.1.1',
              user_agent: 'Mozilla/5.0',
              device_info: 'Chrome on Windows',
              location: 'New York, US',
              created_at: '2024-01-01T00:00:00Z',
              last_active_at: '2024-01-01T12:00:00Z',
              expires_at: '2024-01-08T00:00:00Z',
              is_current: true,
            },
            {
              id: 'session-2',
              user_id: 'user-1',
              ip_address: '192.168.1.2',
              user_agent: 'Safari/537.36',
              device_info: 'Safari on MacOS',
              location: null,
              created_at: '2023-12-31T00:00:00Z',
              last_active_at: '2024-01-01T10:00:00Z',
              expires_at: '2024-01-07T00:00:00Z',
              is_current: false,
            },
          ],
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockSessions,
        });

        const result = await client.getSessions();

        expect(result).toEqual(mockSessions);
        expect(mockFetch).toHaveBeenCalledWith(
          '/api/auth/sessions',
          expect.objectContaining({
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
          })
        );
      });

      it('throws error when fetch fails', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 401,
        });

        await expect(client.getSessions()).rejects.toThrow('Failed to fetch sessions: 401');
      });

      it('validates response schema', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ invalid: 'data' }),
        });

        await expect(client.getSessions()).rejects.toThrow();
      });
    });

    describe('revokeSession', () => {
      it('revokes a specific session successfully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.revokeSession('session-123');

        expect(mockFetch).toHaveBeenCalledWith(
          '/api/auth/sessions/session-123',
          expect.objectContaining({
            method: 'DELETE',
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
          })
        );
      });

      it('throws error when revoke fails', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
        });

        await expect(client.revokeSession('invalid-session')).rejects.toThrow(
          'Failed to revoke session: 404'
        );
      });
    });

    describe('revokeAllSessions', () => {
      it('revokes all sessions successfully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.revokeAllSessions();

        expect(mockFetch).toHaveBeenCalledWith(
          '/api/auth/sessions',
          expect.objectContaining({
            method: 'DELETE',
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
          })
        );
      });

      it('throws error when revoke all fails', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
        });

        await expect(client.revokeAllSessions()).rejects.toThrow(
          'Failed to revoke all sessions: 500'
        );
      });
    });
  });

  // =============================================================================
  // Organization Member Management Tests
  // =============================================================================

  describe('Organization Member Management', () => {
    const orgId = 'org-123';
    const userId = 'user-456';

    const mockMember = {
      id: 'member-1',
      user_id: userId,
      organization_id: orgId,
      role: 'member' as const,
      email: 'member@test.com',
      display_name: 'Test Member',
      joined_at: '2024-01-01T00:00:00Z',
    };

    const mockOwner = {
      id: 'member-2',
      user_id: 'user-999',
      organization_id: orgId,
      role: 'owner' as const,
      email: 'owner@test.com',
      display_name: 'Test Owner',
      joined_at: '2024-01-01T00:00:00Z',
    };

    describe('getOrgMembers', () => {
      it('fetches organization members successfully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({
            members: [mockMember, mockOwner],
          }),
        });

        const result = await client.getOrgMembers(orgId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${orgId}/members`,
          expect.objectContaining({
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
          })
        );
        expect(result.members).toHaveLength(2);
        expect(result.members[0]).toEqual(mockMember);
        expect(result.members[1]).toEqual(mockOwner);
      });

      it('includes Authorization header when token is set', async () => {
        client.setAccessToken('test-token');
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ members: [] }),
        });

        await client.getOrgMembers(orgId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${orgId}/members`,
          expect.objectContaining({
            headers: expect.objectContaining({
              Authorization: 'Bearer test-token',
            }),
          })
        );
      });

      it('throws error when request fails', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 403,
        });

        await expect(client.getOrgMembers(orgId)).rejects.toThrow(
          'Failed to fetch organization members: 403'
        );
      });

      it('validates response schema', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ invalid: 'data' }),
        });

        await expect(client.getOrgMembers(orgId)).rejects.toThrow();
      });
    });

    describe('addOrgMember', () => {
      const request = {
        email: 'newmember@test.com',
        role: 'member' as const,
      };

      it('adds a new member successfully', async () => {
        const newMember = {
          ...mockMember,
          email: request.email,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => newMember,
        });

        const result = await client.addOrgMember(orgId, request);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${orgId}/members`,
          expect.objectContaining({
            method: 'POST',
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
            body: JSON.stringify(request),
          })
        );
        expect(result).toEqual(newMember);
      });

      it('can add admin role', async () => {
        const adminRequest = { email: 'admin@test.com', role: 'admin' as const };
        const newAdmin = {
          ...mockMember,
          email: adminRequest.email,
          role: 'admin' as const,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => newAdmin,
        });

        const result = await client.addOrgMember(orgId, adminRequest);

        expect(result.role).toBe('admin');
      });

      it('throws error when email is invalid', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 400,
        });

        await expect(
          client.addOrgMember(orgId, { email: 'invalid', role: 'member' })
        ).rejects.toThrow('Failed to add organization member: 400');
      });

      it('throws error when user already exists', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 409,
        });

        await expect(client.addOrgMember(orgId, request)).rejects.toThrow(
          'Failed to add organization member: 409'
        );
      });
    });

    describe('updateOrgMemberRole', () => {
      const request = { role: 'admin' as const };

      it('updates member role successfully', async () => {
        const updatedMember = {
          ...mockMember,
          role: 'admin' as const,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => updatedMember,
        });

        const result = await client.updateOrgMemberRole(orgId, userId, request);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${orgId}/members/${userId}`,
          expect.objectContaining({
            method: 'PATCH',
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
            body: JSON.stringify(request),
          })
        );
        expect(result.role).toBe('admin');
      });

      it('can downgrade admin to member', async () => {
        const downgradeRequest = { role: 'member' as const };
        const downgradedMember = {
          ...mockMember,
          role: 'member' as const,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => downgradedMember,
        });

        const result = await client.updateOrgMemberRole(orgId, userId, downgradeRequest);

        expect(result.role).toBe('member');
      });

      it('throws error when trying to modify last owner', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 403,
        });

        await expect(client.updateOrgMemberRole(orgId, userId, request)).rejects.toThrow(
          'Failed to update organization member role: 403'
        );
      });

      it('throws error when member not found', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
        });

        await expect(client.updateOrgMemberRole(orgId, 'invalid-user', request)).rejects.toThrow(
          'Failed to update organization member role: 404'
        );
      });
    });

    describe('removeOrgMember', () => {
      it('removes a member successfully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.removeOrgMember(orgId, userId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${orgId}/members/${userId}`,
          expect.objectContaining({
            method: 'DELETE',
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
          })
        );
      });

      it('throws error when trying to remove last owner', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 403,
        });

        await expect(client.removeOrgMember(orgId, userId)).rejects.toThrow(
          'Failed to remove organization member: 403'
        );
      });

      it('throws error when member not found', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
        });

        await expect(client.removeOrgMember(orgId, 'invalid-user')).rejects.toThrow(
          'Failed to remove organization member: 404'
        );
      });

      it('includes Authorization header when token is set', async () => {
        client.setAccessToken('test-token');
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.removeOrgMember(orgId, userId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${orgId}/members/${userId}`,
          expect.objectContaining({
            headers: expect.objectContaining({
              Authorization: 'Bearer test-token',
            }),
          })
        );
      });
    });

    describe('URL Encoding', () => {
      it('encodes orgId with special characters in getOrgMembers', async () => {
        const specialOrgId = 'org@special#123';
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ members: [] }),
        });

        await client.getOrgMembers(specialOrgId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(specialOrgId)}/members`,
          expect.any(Object)
        );
      });

      it('encodes orgId and userId with special characters in updateOrgMemberRole', async () => {
        const specialOrgId = 'org+with+plus';
        const specialUserId = 'user/with/slash';
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockMember,
        });

        await client.updateOrgMemberRole(specialOrgId, specialUserId, { role: 'admin' });

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(specialOrgId)}/members/${encodeURIComponent(specialUserId)}`,
          expect.objectContaining({
            method: 'PATCH',
          })
        );
      });

      it('encodes orgId and userId with spaces in removeOrgMember', async () => {
        const orgWithSpace = 'org with space';
        const userWithSpace = 'user with space';
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.removeOrgMember(orgWithSpace, userWithSpace);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(orgWithSpace)}/members/${encodeURIComponent(userWithSpace)}`,
          expect.objectContaining({
            method: 'DELETE',
          })
        );
      });

      it('encodes orgId with unicode characters in addOrgMember', async () => {
        const unicodeOrgId = 'org-日本語-123';
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockMember,
        });

        await client.addOrgMember(unicodeOrgId, { email: 'test@example.com', role: 'member' });

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(unicodeOrgId)}/members`,
          expect.objectContaining({
            method: 'POST',
          })
        );
      });
    });

    describe('Error Response Parsing', () => {
      it('parses error message from getOrgMembers failure', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 403,
          json: async () => ({ message: 'Insufficient permissions' }),
        });

        await expect(client.getOrgMembers(orgId)).rejects.toThrow('Insufficient permissions');
      });

      it('parses error message from addOrgMember failure', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 409,
          json: async () => ({ error: 'User already in organization' }),
        });

        await expect(
          client.addOrgMember(orgId, { email: 'test@example.com', role: 'member' })
        ).rejects.toThrow('User already in organization');
      });

      it('parses error detail from updateOrgMemberRole failure', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 400,
          json: async () => ({ detail: 'Cannot modify last owner' }),
        });

        await expect(client.updateOrgMemberRole(orgId, userId, { role: 'member' })).rejects.toThrow(
          'Cannot modify last owner'
        );
      });

      it('falls back to status code when no error message available', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: async () => ({}),
        });

        await expect(client.removeOrgMember(orgId, userId)).rejects.toThrow(
          'Failed to remove organization member: 500'
        );
      });

      it('handles JSON parse errors gracefully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: async () => {
            throw new Error('Invalid JSON');
          },
        });

        await expect(client.getOrgMembers(orgId)).rejects.toThrow(
          'Failed to fetch organization members: 500'
        );
      });
    });
  });

  // =============================================================================
  // Workspace Member Management Tests
  // =============================================================================

  describe('Workspace Member Management', () => {
    const orgId = 'org-123';
    const workspaceId = 'ws-456';
    const userId = 'user-789';

    const mockMember = {
      id: 'ws-member-1',
      user_id: userId,
      workspace_id: workspaceId,
      role: 'member' as const,
      email: 'member@test.com',
      display_name: 'Test Member',
      joined_at: '2024-01-01T00:00:00Z',
    };

    const mockOwner = {
      id: 'ws-member-2',
      user_id: 'user-999',
      workspace_id: workspaceId,
      role: 'owner' as const,
      email: 'owner@test.com',
      display_name: 'Test Owner',
      joined_at: '2024-01-01T00:00:00Z',
    };

    const mockViewer = {
      id: 'ws-member-3',
      user_id: 'user-111',
      workspace_id: workspaceId,
      role: 'viewer' as const,
      email: 'viewer@test.com',
      display_name: null,
      joined_at: '2024-01-02T00:00:00Z',
    };

    describe('getWorkspaceMembers', () => {
      it('fetches workspace members successfully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({
            members: [mockMember, mockOwner, mockViewer],
          }),
        });

        const result = await client.getWorkspaceMembers(workspaceId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/workspaces/${workspaceId}/members`,
          expect.objectContaining({
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
          })
        );
        expect(result.members).toHaveLength(3);
        expect(result.members[0]).toEqual(mockMember);
        expect(result.members[1]).toEqual(mockOwner);
        expect(result.members[2]).toEqual(mockViewer);
      });

      it('includes Authorization header when token is set', async () => {
        client.setAccessToken('test-token');
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ members: [] }),
        });

        await client.getWorkspaceMembers(workspaceId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/workspaces/${workspaceId}/members`,
          expect.objectContaining({
            headers: expect.objectContaining({
              Authorization: 'Bearer test-token',
            }),
          })
        );
      });

      it('throws error when request fails', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 403,
        });

        await expect(client.getWorkspaceMembers(workspaceId)).rejects.toThrow(
          'Failed to fetch workspace members: 403'
        );
      });

      it('validates response schema', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ invalid: 'data' }),
        });

        await expect(client.getWorkspaceMembers(workspaceId)).rejects.toThrow();
      });

      it('URL encodes workspace ID', async () => {
        const specialWorkspaceId = 'ws@special#123';
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ members: [] }),
        });

        await client.getWorkspaceMembers(specialWorkspaceId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/workspaces/${encodeURIComponent(specialWorkspaceId)}/members`,
          expect.any(Object)
        );
      });
    });

    describe('addWorkspaceMember', () => {
      const request = {
        user_id: userId,
        role: 'member' as const,
      };

      it('adds a new member successfully', async () => {
        const newMember = {
          ...mockMember,
          user_id: userId,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => newMember,
        });

        const result = await client.addWorkspaceMember(workspaceId, request);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/workspaces/${workspaceId}/members`,
          expect.objectContaining({
            method: 'POST',
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
            body: JSON.stringify(request),
          })
        );
        expect(result).toEqual(newMember);
      });

      it('can add viewer role', async () => {
        const viewerRequest = { user_id: 'user-new', role: 'viewer' as const };
        const newViewer = {
          ...mockMember,
          user_id: viewerRequest.user_id,
          role: 'viewer' as const,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => newViewer,
        });

        const result = await client.addWorkspaceMember(workspaceId, viewerRequest);

        expect(result.role).toBe('viewer');
      });

      it('can add admin role', async () => {
        const adminRequest = { user_id: 'user-admin', role: 'admin' as const };
        const newAdmin = {
          ...mockMember,
          user_id: adminRequest.user_id,
          role: 'admin' as const,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => newAdmin,
        });

        const result = await client.addWorkspaceMember(workspaceId, adminRequest);

        expect(result.role).toBe('admin');
      });

      it('throws error when user not in organization', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
        });

        await expect(
          client.addWorkspaceMember(workspaceId, { user_id: 'invalid-user', role: 'member' })
        ).rejects.toThrow('Failed to add workspace member: 404');
      });

      it('throws error when user already in workspace', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 409,
        });

        await expect(client.addWorkspaceMember(workspaceId, request)).rejects.toThrow(
          'Failed to add workspace member: 409'
        );
      });
    });

    describe('updateWorkspaceMemberRole', () => {
      const request = { role: 'admin' as const };

      it('updates member role successfully', async () => {
        const updatedMember = {
          ...mockMember,
          role: 'admin' as const,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => updatedMember,
        });

        const result = await client.updateWorkspaceMemberRole(workspaceId, userId, request);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/workspaces/${workspaceId}/members/${userId}`,
          expect.objectContaining({
            method: 'PATCH',
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
            body: JSON.stringify(request),
          })
        );
        expect(result.role).toBe('admin');
      });

      it('can downgrade admin to member', async () => {
        const downgradeRequest = { role: 'member' as const };
        const downgradedMember = {
          ...mockMember,
          role: 'member' as const,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => downgradedMember,
        });

        const result = await client.updateWorkspaceMemberRole(
          workspaceId,
          userId,
          downgradeRequest
        );

        expect(result.role).toBe('member');
      });

      it('can update to viewer role', async () => {
        const viewerRequest = { role: 'viewer' as const };
        const viewerMember = {
          ...mockMember,
          role: 'viewer' as const,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => viewerMember,
        });

        const result = await client.updateWorkspaceMemberRole(workspaceId, userId, viewerRequest);

        expect(result.role).toBe('viewer');
      });

      it('throws error when trying to modify last owner', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 403,
        });

        await expect(
          client.updateWorkspaceMemberRole(workspaceId, userId, request)
        ).rejects.toThrow('Failed to update workspace member role: 403');
      });

      it('throws error when member not found', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
        });

        await expect(
          client.updateWorkspaceMemberRole(workspaceId, 'invalid-user', request)
        ).rejects.toThrow('Failed to update workspace member role: 404');
      });

      it('URL encodes workspace ID and user ID', async () => {
        const specialWorkspaceId = 'ws/special';
        const specialUserId = 'user+special';
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockMember,
        });

        await client.updateWorkspaceMemberRole(specialWorkspaceId, specialUserId, request);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/workspaces/${encodeURIComponent(specialWorkspaceId)}/members/${encodeURIComponent(specialUserId)}`,
          expect.objectContaining({
            method: 'PATCH',
          })
        );
      });
    });

    describe('removeWorkspaceMember', () => {
      it('removes a member successfully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.removeWorkspaceMember(workspaceId, userId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/workspaces/${workspaceId}/members/${userId}`,
          expect.objectContaining({
            method: 'DELETE',
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
          })
        );
      });

      it('throws error when trying to remove last owner', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 403,
        });

        await expect(client.removeWorkspaceMember(workspaceId, userId)).rejects.toThrow(
          'Failed to remove workspace member: 403'
        );
      });

      it('throws error when member not found', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
        });

        await expect(client.removeWorkspaceMember(workspaceId, 'invalid-user')).rejects.toThrow(
          'Failed to remove workspace member: 404'
        );
      });

      it('includes Authorization header when token is set', async () => {
        client.setAccessToken('test-token');
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.removeWorkspaceMember(workspaceId, userId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/workspaces/${workspaceId}/members/${userId}`,
          expect.objectContaining({
            headers: expect.objectContaining({
              Authorization: 'Bearer test-token',
            }),
          })
        );
      });

      it('URL encodes workspace ID and user ID with special characters', async () => {
        const specialWorkspaceId = 'ws with space';
        const specialUserId = 'user@special';
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.removeWorkspaceMember(specialWorkspaceId, specialUserId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/workspaces/${encodeURIComponent(specialWorkspaceId)}/members/${encodeURIComponent(specialUserId)}`,
          expect.objectContaining({
            method: 'DELETE',
          })
        );
      });
    });

    describe('Error Response Parsing', () => {
      it('parses error message from getWorkspaceMembers failure', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 403,
          json: async () => ({ message: 'Insufficient permissions' }),
        });

        await expect(client.getWorkspaceMembers(workspaceId)).rejects.toThrow(
          'Insufficient permissions'
        );
      });

      it('parses error message from addWorkspaceMember failure', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 409,
          json: async () => ({ error: 'User already in workspace' }),
        });

        await expect(
          client.addWorkspaceMember(workspaceId, { user_id: 'test-user', role: 'member' })
        ).rejects.toThrow('User already in workspace');
      });

      it('parses error detail from updateWorkspaceMemberRole failure', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 400,
          json: async () => ({ detail: 'Cannot modify last owner' }),
        });

        await expect(
          client.updateWorkspaceMemberRole(workspaceId, userId, { role: 'member' })
        ).rejects.toThrow('Cannot modify last owner');
      });

      it('falls back to status code when no error message available', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: async () => ({}),
        });

        await expect(client.removeWorkspaceMember(workspaceId, userId)).rejects.toThrow(
          'Failed to remove workspace member: 500'
        );
      });

      it('handles JSON parse errors gracefully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: async () => {
            throw new Error('Invalid JSON');
          },
        });

        await expect(client.getWorkspaceMembers(workspaceId)).rejects.toThrow(
          'Failed to fetch workspace members: 500'
        );
      });
    });
  });

  // =============================================================================
  // Invitation Management Tests
  // =============================================================================

  describe('Invitation Management', () => {
    const orgId = 'org-123';
    const invitationId = 'inv-456';
    const token = 'secure-token-abc123';

    const mockInvitation = {
      id: invitationId,
      organization_id: orgId,
      organization_name: 'Test Org',
      email: 'invitee@test.com',
      org_role: 'member' as const,
      workspace_id: 'ws-789',
      workspace_name: 'Test Workspace',
      workspace_role: 'member' as const,
      invited_by_email: 'admin@test.com',
      created_at: '2024-01-01T00:00:00Z',
      expires_at: '2024-01-08T00:00:00Z',
    };

    const mockInvitationOrgOnly = {
      id: 'inv-999',
      organization_id: orgId,
      organization_name: 'Test Org',
      email: 'another@test.com',
      org_role: 'admin' as const,
      workspace_id: null,
      workspace_name: null,
      workspace_role: null,
      invited_by_email: 'owner@test.com',
      created_at: '2024-01-02T00:00:00Z',
      expires_at: '2024-01-09T00:00:00Z',
    };

    describe('createInvitation', () => {
      it('creates org+workspace invitation successfully', async () => {
        const request = {
          email: 'invitee@test.com',
          org_role: 'member' as const,
          workspace_id: 'ws-789',
          workspace_role: 'member' as const,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockInvitation,
        });

        const result = await client.createInvitation(orgId, request);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${orgId}/invitations`,
          expect.objectContaining({
            method: 'POST',
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
            body: JSON.stringify(request),
          })
        );
        expect(result).toEqual(mockInvitation);
      });

      it('creates org-only invitation successfully', async () => {
        const request = {
          email: 'another@test.com',
          org_role: 'admin' as const,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockInvitationOrgOnly,
        });

        const result = await client.createInvitation(orgId, request);

        expect(result.workspace_id).toBeNull();
        expect(result.workspace_role).toBeNull();
      });

      it('throws error when email is invalid', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 400,
          json: async () => ({ message: 'Invalid email' }),
        });

        await expect(
          client.createInvitation(orgId, { email: 'invalid', org_role: 'member' })
        ).rejects.toThrow('Invalid email');
      });

      it('throws error when user already in organization', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 409,
          json: async () => ({ message: 'User already exists in organization' }),
        });

        await expect(
          client.createInvitation(orgId, { email: 'existing@test.com', org_role: 'member' })
        ).rejects.toThrow('User already exists in organization');
      });

      it('URL encodes organization ID', async () => {
        const specialOrgId = 'org@special#123';
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockInvitation,
        });

        await client.createInvitation(specialOrgId, { email: 'test@test.com', org_role: 'member' });

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(specialOrgId)}/invitations`,
          expect.any(Object)
        );
      });
    });

    describe('getInvitations', () => {
      it('fetches pending invitations successfully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({
            invitations: [mockInvitation, mockInvitationOrgOnly],
          }),
        });

        const result = await client.getInvitations(orgId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${orgId}/invitations`,
          expect.objectContaining({
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
          })
        );
        expect(result.invitations).toHaveLength(2);
        expect(result.invitations[0]).toEqual(mockInvitation);
        expect(result.invitations[1]).toEqual(mockInvitationOrgOnly);
      });

      it('returns empty array when no invitations', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ invitations: [] }),
        });

        const result = await client.getInvitations(orgId);

        expect(result.invitations).toEqual([]);
      });

      it('includes Authorization header when token is set', async () => {
        client.setAccessToken('test-token');
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ invitations: [] }),
        });

        await client.getInvitations(orgId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${orgId}/invitations`,
          expect.objectContaining({
            headers: expect.objectContaining({
              Authorization: 'Bearer test-token',
            }),
          })
        );
      });

      it('throws error when request fails', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 403,
        });

        await expect(client.getInvitations(orgId)).rejects.toThrow(
          'Failed to fetch invitations: 403'
        );
      });
    });

    describe('revokeInvitation', () => {
      it('revokes invitation successfully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.revokeInvitation(orgId, invitationId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${orgId}/invitations/${invitationId}`,
          expect.objectContaining({
            method: 'DELETE',
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
          })
        );
      });

      it('throws error when invitation not found', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
        });

        await expect(client.revokeInvitation(orgId, 'invalid-inv')).rejects.toThrow(
          'Failed to revoke invitation: 404'
        );
      });

      it('URL encodes organization ID and invitation ID', async () => {
        const specialOrgId = 'org with space';
        const specialInvId = 'inv/special';
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.revokeInvitation(specialOrgId, specialInvId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(specialOrgId)}/invitations/${encodeURIComponent(specialInvId)}`,
          expect.objectContaining({
            method: 'DELETE',
          })
        );
      });
    });

    describe('getInvitationByToken', () => {
      const mockDetails = {
        organization_name: 'Test Org',
        org_role: 'member' as const,
        workspace_name: 'Test Workspace',
        workspace_role: 'member' as const,
        invited_by_email: 'admin@test.com',
        expires_at: '2024-01-08T00:00:00Z',
      };

      it('fetches invitation details successfully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockDetails,
        });

        const result = await client.getInvitationByToken(token);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/invitations/${token}`,
          expect.objectContaining({
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
          })
        );
        expect(result).toEqual(mockDetails);
      });

      it('fetches org-only invitation details', async () => {
        const orgOnlyDetails = {
          ...mockDetails,
          workspace_name: null,
          workspace_role: null,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => orgOnlyDetails,
        });

        const result = await client.getInvitationByToken(token);

        expect(result.workspace_name).toBeNull();
        expect(result.workspace_role).toBeNull();
      });

      it('throws error when token is invalid', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
          json: async () => ({ message: 'Invitation not found or expired' }),
        });

        await expect(client.getInvitationByToken('invalid-token')).rejects.toThrow(
          'Invitation not found or expired'
        );
      });

      it('URL encodes token', async () => {
        const specialToken = 'token+with+special=chars';
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockDetails,
        });

        await client.getInvitationByToken(specialToken);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/invitations/${encodeURIComponent(specialToken)}`,
          expect.any(Object)
        );
      });
    });

    describe('acceptInvitation', () => {
      it('accepts invitation successfully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.acceptInvitation(token);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/invitations/${token}/accept`,
          expect.objectContaining({
            method: 'POST',
            headers: expect.objectContaining({
              'Content-Type': 'application/json',
            }),
          })
        );
      });

      it('includes Authorization header when token is set', async () => {
        client.setAccessToken('auth-token');
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.acceptInvitation(token);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/invitations/${token}/accept`,
          expect.objectContaining({
            headers: expect.objectContaining({
              Authorization: 'Bearer auth-token',
            }),
          })
        );
      });

      it('throws error when token is invalid or expired', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
          json: async () => ({ message: 'Invitation not found or expired' }),
        });

        await expect(client.acceptInvitation('expired-token')).rejects.toThrow(
          'Invitation not found or expired'
        );
      });

      it('throws error when user not authenticated', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 401,
          json: async () => ({ message: 'Authentication required' }),
        });

        await expect(client.acceptInvitation(token)).rejects.toThrow('Authentication required');
      });

      it('URL encodes token', async () => {
        const specialToken = 'token/with/slashes';
        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.acceptInvitation(specialToken);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/invitations/${encodeURIComponent(specialToken)}/accept`,
          expect.objectContaining({
            method: 'POST',
          })
        );
      });
    });

    describe('Error Response Parsing', () => {
      it('parses error message from createInvitation failure', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 403,
          json: async () => ({ message: 'Insufficient permissions to create invitations' }),
        });

        await expect(
          client.createInvitation(orgId, { email: 'test@test.com', org_role: 'member' })
        ).rejects.toThrow('Insufficient permissions to create invitations');
      });

      it('parses error detail from acceptInvitation failure', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 400,
          json: async () => ({ detail: 'User already in organization' }),
        });

        await expect(client.acceptInvitation(token)).rejects.toThrow(
          'User already in organization'
        );
      });

      it('falls back to status code when no error message available', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: async () => ({}),
        });

        await expect(client.revokeInvitation(orgId, invitationId)).rejects.toThrow(
          'Failed to revoke invitation: 500'
        );
      });
    });
  });

  // =============================================================================
  // Billing & Usage API Tests
  // =============================================================================

  describe('Billing & Usage API', () => {
    const orgId = 'org-123';

    describe('getPlans', () => {
      it('fetches all public plans successfully', async () => {
        const mockPlans: Plan[] = [
          {
            id: 'plan-free',
            name: 'Free',
            description: 'Free tier',
            price_monthly: 0,
            price_yearly: 0,
            features: ['5 users', '10 projects'],
            limits: {
              max_users: 5,
              max_workspaces: 3,
              max_projects: 10,
              max_storage_gb: 1,
              max_api_calls_monthly: 1000,
            },
            is_public: true,
          },
          {
            id: 'plan-pro',
            name: 'Pro',
            description: 'Professional tier',
            price_monthly: 29,
            price_yearly: 290,
            features: ['Unlimited users', 'Unlimited projects', 'Priority support'],
            limits: {
              max_users: null,
              max_workspaces: null,
              max_projects: null,
              max_storage_gb: 100,
              max_api_calls_monthly: 100000,
            },
            is_public: true,
          },
        ];

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ plans: mockPlans }),
        });

        const result = await client.getPlans();

        expect(result).toEqual(mockPlans);
        expect(mockFetch).toHaveBeenCalledWith('/api/plans', expect.any(Object));
      });

      it('throws error when fetch fails', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: async () => ({ message: 'Internal server error' }),
        });

        await expect(client.getPlans()).rejects.toThrow('Internal server error');
      });
    });

    describe('getPlan', () => {
      it('fetches a specific plan by ID', async () => {
        const planId = 'plan-pro';
        const mockPlan: Plan = {
          id: planId,
          name: 'Pro',
          description: 'Professional tier',
          price_monthly: 29,
          price_yearly: 290,
          features: ['Unlimited users', 'Unlimited projects'],
          limits: {
            max_users: null,
            max_workspaces: null,
            max_projects: null,
            max_storage_gb: 100,
            max_api_calls_monthly: 100000,
          },
          is_public: true,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ plan: mockPlan }),
        });

        const result = await client.getPlan(planId);

        expect(result).toEqual(mockPlan);
        expect(mockFetch).toHaveBeenCalledWith(
          `/api/plans/${encodeURIComponent(planId)}`,
          expect.any(Object)
        );
      });

      it('URL encodes plan ID', async () => {
        const planId = 'plan/special@id';
        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({
            plan: {
              id: planId,
              name: 'Test',
              description: null,
              price_monthly: 0,
              price_yearly: 0,
              features: [],
              limits: {
                max_users: null,
                max_workspaces: null,
                max_projects: null,
                max_storage_gb: null,
                max_api_calls_monthly: null,
              },
              is_public: true,
            },
          }),
        });

        await client.getPlan(planId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/plans/${encodeURIComponent(planId)}`,
          expect.any(Object)
        );
      });
    });

    describe('getSubscription', () => {
      it('fetches organization subscription', async () => {
        const mockSubscription: Subscription = {
          id: 'sub-123',
          organization_id: orgId,
          plan_id: 'plan-pro',
          plan_name: 'Pro',
          status: 'active',
          current_period_start: '2024-01-01T00:00:00Z',
          current_period_end: '2024-02-01T00:00:00Z',
          cancel_at_period_end: false,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ subscription: mockSubscription }),
        });

        const result = await client.getSubscription(orgId);

        expect(result).toEqual(mockSubscription);
        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(orgId)}/subscription`,
          expect.any(Object)
        );
      });

      it('handles different subscription statuses', async () => {
        const statuses: Array<'active' | 'canceled' | 'past_due' | 'trialing'> = [
          'active',
          'canceled',
          'past_due',
          'trialing',
        ];

        for (const status of statuses) {
          const mockSubscription: Subscription = {
            id: 'sub-123',
            organization_id: orgId,
            plan_id: 'plan-pro',
            plan_name: 'Pro',
            status,
            current_period_start: '2024-01-01T00:00:00Z',
            current_period_end: '2024-02-01T00:00:00Z',
            cancel_at_period_end: false,
          };

          mockFetch.mockResolvedValueOnce({
            ok: true,
            json: async () => ({ subscription: mockSubscription }),
          });

          const result = await client.getSubscription(orgId);
          expect(result.status).toBe(status);
        }
      });
    });

    describe('getUsage', () => {
      it('fetches current period usage', async () => {
        const mockUsage: Usage = {
          users: 15,
          workspaces: 3,
          projects: 42,
          storage_gb: 5.7,
          api_calls: 12543,
          period_start: '2024-01-01T00:00:00Z',
          period_end: '2024-02-01T00:00:00Z',
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockUsage,
        });

        const result = await client.getUsage(orgId);

        expect(result).toEqual(mockUsage);
        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(orgId)}/usage`,
          expect.any(Object)
        );
      });

      it('handles zero usage', async () => {
        const mockUsage: Usage = {
          users: 0,
          workspaces: 0,
          projects: 0,
          storage_gb: 0,
          api_calls: 0,
          period_start: '2024-01-01T00:00:00Z',
          period_end: '2024-02-01T00:00:00Z',
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockUsage,
        });

        const result = await client.getUsage(orgId);

        expect(result).toEqual(mockUsage);
        expect(result.users).toBe(0);
        expect(result.api_calls).toBe(0);
      });
    });

    describe('getLimits', () => {
      it('fetches organization limits', async () => {
        const mockLimits: Limits = {
          max_users: 50,
          max_workspaces: 10,
          max_projects: 100,
          max_storage_gb: 50,
          max_api_calls_monthly: 50000,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockLimits,
        });

        const result = await client.getLimits(orgId);

        expect(result).toEqual(mockLimits);
        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(orgId)}/limits`,
          expect.any(Object)
        );
      });

      it('handles unlimited limits (null values)', async () => {
        const mockLimits: Limits = {
          max_users: null,
          max_workspaces: null,
          max_projects: null,
          max_storage_gb: 100,
          max_api_calls_monthly: 100000,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockLimits,
        });

        const result = await client.getLimits(orgId);

        expect(result).toEqual(mockLimits);
        expect(result.max_users).toBeNull();
        expect(result.max_workspaces).toBeNull();
      });
    });

    describe('Error Handling', () => {
      it('handles subscription not found error', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
          json: async () => ({ message: 'No active subscription found' }),
        });

        await expect(client.getSubscription(orgId)).rejects.toThrow('No active subscription found');
      });

      it('handles authorization errors', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 403,
          json: async () => ({ message: 'Access denied to billing information' }),
        });

        await expect(client.getUsage(orgId)).rejects.toThrow(
          'Access denied to billing information'
        );
      });
    });
  });

  describe('Audit Logs API', () => {
    const orgId = 'org-123';

    describe('getAuditLogs', () => {
      it('fetches audit logs without filters', async () => {
        const mockResponse = {
          logs: [
            {
              id: 'log-1',
              organization_id: orgId,
              actor_id: 'user-1',
              actor_email: 'user@example.com',
              action: 'create',
              resource_type: 'project',
              resource_id: 'proj-1',
              metadata: { name: 'New Project' },
              created_at: '2024-01-01T00:00:00Z',
            },
          ],
          total: 1,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        const result = await client.getAuditLogs(orgId);

        expect(result).toEqual(mockResponse);
        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(orgId)}/audit-logs`,
          expect.any(Object)
        );
      });

      it('fetches audit logs with all filters', async () => {
        const filters = {
          action: 'update' as const,
          resource_type: 'task' as const,
          resource_id: 'task-123',
          actor_id: 'user-456',
          start_date: '2024-01-01',
          end_date: '2024-01-31',
          limit: 50,
          offset: 10,
        };

        const mockResponse = {
          logs: [],
          total: 0,
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        await client.getAuditLogs(orgId, filters);

        const expectedUrl = `/api/organizations/${encodeURIComponent(orgId)}/audit-logs?action=update&resource_type=task&resource_id=task-123&actor_id=user-456&start_date=2024-01-01&end_date=2024-01-31&limit=50&offset=10`;
        expect(mockFetch).toHaveBeenCalledWith(expectedUrl, expect.any(Object));
      });

      it('handles errors gracefully', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 403,
          json: async () => ({ message: 'Access denied' }),
        });

        await expect(client.getAuditLogs(orgId)).rejects.toThrow('Access denied');
      });

      it('URL encodes organization ID', async () => {
        const specialOrgId = 'org/with/slashes';

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ logs: [], total: 0 }),
        });

        await client.getAuditLogs(specialOrgId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(specialOrgId)}/audit-logs`,
          expect.any(Object)
        );
      });
    });

    describe('getAuditLog', () => {
      it('fetches a specific audit log', async () => {
        const logId = 'log-123';
        const mockLog = {
          id: logId,
          organization_id: orgId,
          actor_id: 'user-1',
          actor_email: 'user@example.com',
          action: 'delete',
          resource_type: 'source',
          resource_id: 'src-1',
          metadata: { reason: 'Cleanup' },
          created_at: '2024-01-01T00:00:00Z',
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockLog,
        });

        const result = await client.getAuditLog(orgId, logId);

        expect(result).toEqual(mockLog);
        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(orgId)}/audit-logs/${encodeURIComponent(logId)}`,
          expect.any(Object)
        );
      });

      it('URL encodes both IDs', async () => {
        const specialOrgId = 'org/123';
        const specialLogId = 'log/456';

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({
            id: specialLogId,
            organization_id: specialOrgId,
            actor_id: 'user-1',
            actor_email: 'user@example.com',
            action: 'create',
            resource_type: 'user',
            resource_id: 'user-1',
            metadata: {},
            created_at: '2024-01-01T00:00:00Z',
          }),
        });

        await client.getAuditLog(specialOrgId, specialLogId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(specialOrgId)}/audit-logs/${encodeURIComponent(specialLogId)}`,
          expect.any(Object)
        );
      });

      it('handles not found error', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
          json: async () => ({ message: 'Audit log not found' }),
        });

        await expect(client.getAuditLog(orgId, 'invalid-id')).rejects.toThrow(
          'Audit log not found'
        );
      });
    });

    describe('exportAuditLogs', () => {
      it('exports audit logs as CSV without filters', async () => {
        const mockBlob = new Blob(['csv,data'], { type: 'text/csv' });

        mockFetch.mockResolvedValueOnce({
          ok: true,
          blob: async () => mockBlob,
        });

        const result = await client.exportAuditLogs(orgId);

        expect(result).toBe(mockBlob);
        expect(mockFetch).toHaveBeenCalledWith(
          `/api/organizations/${encodeURIComponent(orgId)}/audit-logs/export`,
          expect.any(Object)
        );
      });

      it('exports audit logs with filters', async () => {
        const filters = {
          action: 'login' as const,
          resource_type: 'user' as const,
          start_date: '2024-01-01',
          end_date: '2024-01-31',
        };

        const mockBlob = new Blob(['csv,data'], { type: 'text/csv' });

        mockFetch.mockResolvedValueOnce({
          ok: true,
          blob: async () => mockBlob,
        });

        await client.exportAuditLogs(orgId, filters);

        const expectedUrl = `/api/organizations/${encodeURIComponent(orgId)}/audit-logs/export?action=login&resource_type=user&start_date=2024-01-01&end_date=2024-01-31`;
        expect(mockFetch).toHaveBeenCalledWith(expectedUrl, expect.any(Object));
      });

      it('does not include limit and offset in export', async () => {
        const filters = {
          action: 'create' as const,
          limit: 50,
          offset: 10,
        };

        const mockBlob = new Blob(['csv,data'], { type: 'text/csv' });

        mockFetch.mockResolvedValueOnce({
          ok: true,
          blob: async () => mockBlob,
        });

        await client.exportAuditLogs(orgId, filters);

        const expectedUrl = `/api/organizations/${encodeURIComponent(orgId)}/audit-logs/export?action=create`;
        expect(mockFetch).toHaveBeenCalledWith(expectedUrl, expect.any(Object));
      });

      it('handles export errors', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: async () => ({ message: 'Export failed' }),
        });

        await expect(client.exportAuditLogs(orgId)).rejects.toThrow('Export failed');
      });
    });
  });

  describe('Knowledge Base API', () => {
    const testWorkspaceId = '00000000-0000-0000-0000-000000000001';
    const mockEntry = {
      id: 'kb-1',
      workspace_id: 'ws-1',
      title: 'Test Entry',
      type: 'text' as const,
      content: 'Test content',
      fetched_content: null,
      tags: ['tag1', 'tag2'],
      last_refreshed_at: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    };

    describe('getKnowledge', () => {
      it('fetches knowledge entries successfully', async () => {
        const mockResponse = {
          entries: [mockEntry],
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        const result = await client.getKnowledge(testWorkspaceId);

        expect(result).toEqual(mockResponse);
        expect(mockFetch).toHaveBeenCalledWith(
          `/api/knowledge?workspace_id=${testWorkspaceId}`,
          expect.any(Object)
        );
      });

      it('fetches knowledge entries with different workspace', async () => {
        const workspaceId = 'ws-123';
        const mockResponse = {
          entries: [mockEntry],
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockResponse,
        });

        await client.getKnowledge(workspaceId);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/knowledge?workspace_id=${encodeURIComponent(workspaceId)}`,
          expect.any(Object)
        );
      });

      it('handles fetch errors', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 500,
          json: async () => ({ message: 'Server error' }),
        });

        await expect(client.getKnowledge(testWorkspaceId)).rejects.toThrow('Server error');
      });
    });

    describe('createKnowledge', () => {
      it('creates text knowledge entry successfully', async () => {
        const request = {
          title: 'New Entry',
          type: 'text' as const,
          content: 'Content here',
          tags: ['test'],
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockEntry,
        });

        const result = await client.createKnowledge(request);

        expect(result).toEqual(mockEntry);
        expect(mockFetch).toHaveBeenCalledWith(
          '/api/knowledge',
          expect.objectContaining({
            method: 'POST',
            body: JSON.stringify(request),
          })
        );
      });

      it('creates URL knowledge entry successfully', async () => {
        const urlEntry = {
          ...mockEntry,
          type: 'url' as const,
          content: 'https://example.com',
          fetched_content: 'Fetched content',
        };

        const request = {
          title: 'URL Entry',
          type: 'url' as const,
          content: 'https://example.com',
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => urlEntry,
        });

        const result = await client.createKnowledge(request);

        expect(result.type).toBe('url');
        expect(result.content).toBe('https://example.com');
      });

      it('creates knowledge entry without tags', async () => {
        const request = {
          title: 'Entry without tags',
          type: 'text' as const,
          content: 'Content',
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => ({ ...mockEntry, tags: [] }),
        });

        const result = await client.createKnowledge(request);

        expect(result.tags).toEqual([]);
      });

      it('handles creation errors', async () => {
        const request = {
          title: 'New Entry',
          type: 'text' as const,
          content: 'Content',
        };

        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 400,
          json: async () => ({ message: 'Invalid request' }),
        });

        await expect(client.createKnowledge(request)).rejects.toThrow('Invalid request');
      });
    });

    describe('deleteKnowledge', () => {
      it('deletes knowledge entry successfully', async () => {
        const id = 'kb-1';

        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.deleteKnowledge(id);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/knowledge/${encodeURIComponent(id)}`,
          expect.objectContaining({
            method: 'DELETE',
          })
        );
      });

      it('URL encodes the ID', async () => {
        const id = 'kb/with/slashes';

        mockFetch.mockResolvedValueOnce({
          ok: true,
        });

        await client.deleteKnowledge(id);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/knowledge/${encodeURIComponent(id)}`,
          expect.any(Object)
        );
      });

      it('handles deletion errors', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 404,
          json: async () => ({ message: 'Not found' }),
        });

        await expect(client.deleteKnowledge('kb-1')).rejects.toThrow('Not found');
      });
    });

    describe('refreshKnowledge', () => {
      it('refreshes URL knowledge entry successfully', async () => {
        const refreshedEntry = {
          ...mockEntry,
          type: 'url' as const,
          content: 'https://example.com',
          fetched_content: 'Updated content',
          last_refreshed_at: '2024-01-02T00:00:00Z',
        };

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => refreshedEntry,
        });

        const result = await client.refreshKnowledge('kb-1');

        expect(result).toEqual(refreshedEntry);
        expect(mockFetch).toHaveBeenCalledWith(
          '/api/knowledge/kb-1/refresh',
          expect.objectContaining({
            method: 'POST',
          })
        );
      });

      it('URL encodes the ID', async () => {
        const id = 'kb/special-chars';

        mockFetch.mockResolvedValueOnce({
          ok: true,
          json: async () => mockEntry,
        });

        await client.refreshKnowledge(id);

        expect(mockFetch).toHaveBeenCalledWith(
          `/api/knowledge/${encodeURIComponent(id)}/refresh`,
          expect.any(Object)
        );
      });

      it('handles refresh errors', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 400,
          json: async () => ({ message: 'Cannot refresh text entries' }),
        });

        await expect(client.refreshKnowledge('kb-1')).rejects.toThrow(
          'Cannot refresh text entries'
        );
      });

      it('handles network errors during refresh', async () => {
        mockFetch.mockResolvedValueOnce({
          ok: false,
          status: 503,
          json: async () => ({ message: 'Failed to fetch URL' }),
        });

        await expect(client.refreshKnowledge('kb-1')).rejects.toThrow('Failed to fetch URL');
      });
    });
  });
});
