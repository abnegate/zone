import { client } from './client';

// Mock fetch globally
const mockFetch = jest.fn();
global.fetch = mockFetch;

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

  send = jest.fn();
  close = jest.fn();
}

(global as unknown as { WebSocket: typeof MockWebSocket }).WebSocket = MockWebSocket;

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
        models: [
          { name: 'llama2', size: 3800000000, modified_at: '2024-01-01', digest: 'abc123' },
        ],
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
      const mockResponse = { models: [], has_more: false };
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

    it('includes query parameters', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ models: [], has_more: false }),
      });

      await client.browseModels('huggingface', 'llama', 20, 10);

      const url = mockFetch.mock.calls[0][0];
      expect(url).toContain('source=huggingface');
      expect(url).toContain('q=llama');
      expect(url).toContain('offset=20');
      expect(url).toContain('limit=10');
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
    it('getChats fetches all chats', async () => {
      const mockChats = { chats: [{ id: '1', title: 'Test Chat' }] };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockChats,
      });

      const result = await client.getChats();

      expect(result).toEqual(mockChats.chats);
    });

    it('getChats with archived filter', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ chats: [] }),
      });

      await client.getChats(true);

      expect(mockFetch).toHaveBeenCalledWith('/api/chats?archived=true', expect.any(Object));
    });

    it('getChat fetches single chat', async () => {
      const mockChat = { chat: { id: '1', title: 'Test' } };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockChat,
      });

      const result = await client.getChat('1');

      expect(result).toEqual(mockChat.chat);
    });

    it('createChat creates new chat', async () => {
      const mockChat = { chat: { id: '1', title: 'New Chat' } };
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockChat,
      });

      const result = await client.createChat({ title: 'New Chat', model: 'llama2' });

      expect(result).toEqual(mockChat.chat);
      expect(mockFetch).toHaveBeenCalledWith(
        '/api/chats',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ title: 'New Chat', model: 'llama2' }),
        })
      );
    });

    it('updateChatTitle updates chat', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ chat: { id: '1', title: 'Updated' } }),
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
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ chat: { id: '1', archived: true } }),
      });

      await client.archiveChat('1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/chats/1/archive',
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('unarchiveChat unarchives chat', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ chat: { id: '1', archived: false } }),
      });

      await client.unarchiveChat('1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/chats/1/unarchive',
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('getMessages fetches messages', async () => {
      const mockMessages = { messages: [{ id: '1', content: 'Hello' }] };
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
        json: async () => ({ message: { id: '1', content: 'Hello' } }),
      });

      await client.sendMessage('chat-1', { content: 'Hello', role: 'user' });

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/chats/chat-1/messages',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ content: 'Hello', role: 'user' }),
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
  });

  describe('Projects API', () => {
    it('getProjects fetches projects', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ projects: [{ id: '1', name: 'Test' }] }),
      });

      const result = await client.getProjects();

      expect(result).toHaveLength(1);
    });

    it('getProjects with status filter', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ projects: [] }),
      });

      await client.getProjects('active');

      expect(mockFetch).toHaveBeenCalledWith('/api/projects?status=active', expect.any(Object));
    });

    it('getProject fetches single project', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ project: { id: '1', name: 'Test' } }),
      });

      const result = await client.getProject('1');

      expect(result.id).toBe('1');
    });

    it('createProject creates project', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ project: { id: '1', name: 'New' } }),
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
        json: async () => ({ project: { id: '1', name: 'Updated' } }),
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
        json: async () => ({ project: { id: '1', github_repo_url: 'https://github.com/test' } }),
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
        json: async () => ({ project: { id: '1' } }),
      });

      await client.unlinkGitHub('1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/projects/1/github',
        expect.objectContaining({ method: 'DELETE' })
      );
    });
  });

  describe('Tasks API', () => {
    it('getTasks fetches tasks', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ tasks: [{ id: '1', name: 'Task' }] }),
      });

      const result = await client.getTasks();

      expect(result).toHaveLength(1);
    });

    it('getTasks with filters', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ tasks: [] }),
      });

      await client.getTasks('project-1', 'pending');

      const url = mockFetch.mock.calls[0][0];
      expect(url).toContain('project_id=project-1');
      expect(url).toContain('status=pending');
    });

    it('createTask creates task', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ task: { id: '1' } }),
      });

      await client.createTask({ project_id: 'p1', name: 'Task', prompt: 'Do something' });

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/tasks',
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
        json: async () => ({ run_id: 'run-1' }),
      });

      const result = await client.stopTask('task-1');

      expect(result.run_id).toBe('run-1');
    });

    it('getTaskRuns fetches runs', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ runs: [{ id: 'run-1' }] }),
      });

      const result = await client.getTaskRuns('task-1');

      expect(result).toHaveLength(1);
    });

    it('getTaskRunLogs fetches logs', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ logs: [{ id: 'log-1', message: 'test' }] }),
      });

      const result = await client.getTaskRunLogs('task-1', 'run-1');

      expect(result).toHaveLength(1);
    });
  });

  describe('Sources API', () => {
    it('getSourceTypes fetches source types', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ types: [{ name: 'github' }] }),
      });

      const result = await client.getSourceTypes();

      expect(result).toHaveLength(1);
    });

    it('getSources fetches sources', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ sources: [{ id: '1' }] }),
      });

      const result = await client.getSources();

      expect(result).toHaveLength(1);
    });

    it('getSources with filters', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ sources: [] }),
      });

      await client.getSources('github', true);

      const url = mockFetch.mock.calls[0][0];
      expect(url).toContain('type=github');
      expect(url).toContain('active=true');
    });

    it('createSource creates source', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ source: { id: '1' } }),
      });

      await client.createSource({ type: 'github', name: 'Test', config: {} });

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/sources',
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('verifySource verifies source', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ success: true }),
      });

      const result = await client.verifySource('1');

      expect(result.success).toBe(true);
    });
  });

  describe('Organizations API', () => {
    it('getOrganizations fetches organizations', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ organizations: [{ id: '1', name: 'Org' }] }),
      });

      const result = await client.getOrganizations();

      expect(result).toHaveLength(1);
    });

    it('createOrganization creates organization', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ organization: { id: '1' } }),
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
        json: async () => ({ organization: { id: '1' } }),
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
    it('getWorkspaces fetches workspaces', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ workspaces: [{ id: '1' }] }),
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
        json: async () => ({ workspace: { id: '1' } }),
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
        json: async () => ({ workspace: { id: '1' } }),
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
    it('getWorkspaceTheme fetches theme', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ theme: { primary_color_light: '#007bff' } }),
      });

      const result = await client.getWorkspaceTheme('org-1', 'ws-1');

      expect(result.primary_color_light).toBe('#007bff');
    });

    it('updateWorkspaceTheme updates theme', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ theme: { primary_color_light: '#ff0000' } }),
      });

      await client.updateWorkspaceTheme('org-1', 'ws-1', { primary_color_light: '#ff0000' });

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/organizations/org-1/workspaces/ws-1/settings/theme',
        expect.objectContaining({ method: 'PUT' })
      );
    });

    it('resetWorkspaceTheme resets theme', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ theme: {} }),
      });

      await client.resetWorkspaceTheme('org-1', 'ws-1');

      expect(mockFetch).toHaveBeenCalledWith(
        '/api/organizations/org-1/workspaces/ws-1/settings/theme',
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

      await expect(client.getModelInfo('author/model')).rejects.toThrow('Failed to fetch model info: 404');
    });

    it('getChats throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 401 });

      await expect(client.getChats()).rejects.toThrow('Failed to fetch chats: 401');
    });

    it('getChat throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.getChat('1')).rejects.toThrow('Failed to fetch chat: 404');
    });

    it('createChat throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(client.createChat({ title: 'Test', model: 'llama2' })).rejects.toThrow('Failed to create chat: 400');
    });

    it('updateChatTitle throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.updateChatTitle('1', 'New Title')).rejects.toThrow('Failed to update chat: 500');
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

      await expect(client.sendMessage('chat-1', { content: 'Test', role: 'user' })).rejects.toThrow('Failed to send message: 400');
    });

    it('deleteMessage throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.deleteMessage('chat-1', 'msg-1')).rejects.toThrow('Failed to delete message: 404');
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

      await expect(client.createProject({ name: 'Test', description: 'Desc' })).rejects.toThrow('Failed to create project: 400');
    });

    it('updateProject throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.updateProject('1', { name: 'Updated' })).rejects.toThrow('Failed to update project: 500');
    });

    it('deleteProject throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.deleteProject('1')).rejects.toThrow('Failed to delete project: 404');
    });

    it('linkGitHub throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(client.linkGitHub('1', 'https://github.com/test')).rejects.toThrow('Failed to link GitHub: 400');
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

      await expect(client.createTask({ project_id: 'p1', name: 'Task', prompt: 'Do something' })).rejects.toThrow('Failed to create task: 400');
    });

    it('startTask throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.startTask('task-1')).rejects.toThrow('Failed to start task: 500');
    });

    it('stopTask throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.stopTask('task-1')).rejects.toThrow('Failed to stop task: 500');
    });

    it('getTaskRuns throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.getTaskRuns('task-1')).rejects.toThrow('Failed to fetch task runs: 404');
    });

    it('getTaskRunLogs throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.getTaskRunLogs('task-1', 'run-1')).rejects.toThrow('Failed to fetch task run logs: 404');
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

      await expect(client.createSource({ type: 'github', name: 'Test', config: {} })).rejects.toThrow('Failed to create source: 400');
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

      await expect(client.createOrganization({ name: 'Org', slug: 'org' })).rejects.toThrow('Failed to create organization: 400');
    });

    it('updateOrganization throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.updateOrganization('1', { name: 'Updated' })).rejects.toThrow('Failed to update organization: 500');
    });

    it('deleteOrganization throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.deleteOrganization('1')).rejects.toThrow('Failed to delete organization: 404');
    });

    it('getWorkspaces throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.getWorkspaces('org-1')).rejects.toThrow('Failed to fetch workspaces: 500');
    });

    it('createWorkspace throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(client.createWorkspace('org-1', { name: 'WS', slug: 'ws' })).rejects.toThrow('Failed to create workspace: 400');
    });

    it('updateWorkspace throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.updateWorkspace('org-1', 'ws-1', { name: 'Updated' })).rejects.toThrow('Failed to update workspace: 500');
    });

    it('deleteWorkspace throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 404 });

      await expect(client.deleteWorkspace('org-1', 'ws-1')).rejects.toThrow('Failed to delete workspace: 404');
    });

    it('getWorkspaceTheme throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.getWorkspaceTheme('org-1', 'ws-1')).rejects.toThrow('Failed to fetch workspace theme: 500');
    });

    it('updateWorkspaceTheme throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 400 });

      await expect(client.updateWorkspaceTheme('org-1', 'ws-1', {})).rejects.toThrow('Failed to update workspace theme: 400');
    });

    it('resetWorkspaceTheme throws on failed request', async () => {
      mockFetch.mockResolvedValueOnce({ ok: false, status: 500 });

      await expect(client.resetWorkspaceTheme('org-1', 'ws-1')).rejects.toThrow('Failed to reset workspace theme: 500');
    });
  });
});
