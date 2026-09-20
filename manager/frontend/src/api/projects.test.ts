import { afterEach, beforeEach, describe, expect, it, spyOn } from 'bun:test';
import { projectsApi } from './projects';

const project = {
  id: 'proj-1',
  name: 'Alpha',
  description: null,
  status: 'active',
  github_repo_url: null,
  source_id: 'src-1',
  created_at: '2026-09-20T00:00:00Z',
  updated_at: '2026-09-20T00:00:00Z',
};

const answer = (status: number, body: unknown) =>
  Promise.resolve(new Response(JSON.stringify(body), { status }));

describe('ProjectsApi', () => {
  let mockFetch: ReturnType<typeof spyOn>;

  beforeEach(() => {
    mockFetch = spyOn(global, 'fetch');
    projectsApi.setGetAccessToken(() => 'token');
  });

  afterEach(() => {
    mockFetch.mockRestore();
  });

  it('surfaces the server error text rather than a bare status', async () => {
    mockFetch.mockImplementation(() => answer(403, { error: 'Workspace write access required' }));

    await expect(projectsApi.updateProject('proj-1', { status: 'on_hold' })).rejects.toThrow(
      'Workspace write access required'
    );
  });

  it('saves an edit with PATCH carrying only the changed fields', async () => {
    mockFetch.mockImplementation(() => answer(200, { project: { ...project, status: 'on_hold' } }));

    const updated = await projectsApi.updateProject('proj-1', { status: 'on_hold' });

    expect(updated.status).toBe('on_hold');
    const [url, init] = mockFetch.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/projects/proj-1');
    expect(init.method).toBe('PATCH');
    expect(JSON.parse(init.body as string)).toEqual({ status: 'on_hold' });
  });

  it('sends the chosen source when creating a project', async () => {
    mockFetch.mockImplementation(() => answer(201, { project }));

    const created = await projectsApi.createProject({
      name: 'Alpha',
      workspace_id: 'ws-1',
      source_id: 'src-1',
    });

    expect(created.source_id).toBe('src-1');
    const [, init] = mockFetch.mock.calls[0] as [string, RequestInit];
    expect(JSON.parse(init.body as string).source_id).toBe('src-1');
  });

  it('links and unlinks a source through /source', async () => {
    mockFetch.mockImplementation(() => answer(200, { project }));

    await projectsApi.linkSource('proj-1', 'src-1');
    await projectsApi.unlinkSource('proj-1');

    const calls = mockFetch.mock.calls as [string, RequestInit][];
    expect(calls[0][0]).toBe('/api/projects/proj-1/source');
    expect(calls[0][1].method).toBe('PUT');
    expect(JSON.parse(calls[0][1].body as string)).toEqual({ source_id: 'src-1' });
    expect(calls[1][0]).toBe('/api/projects/proj-1/source');
    expect(calls[1][1].method).toBe('DELETE');
  });

  it('reads a sync configuration with the state the server reports', async () => {
    mockFetch.mockImplementation(() =>
      answer(200, {
        configs: [
          {
            id: 'sync-1',
            project_id: 'proj-1',
            provider: 'github',
            direction: 'outbound',
            external_repo_url: 'https://github.com/acme/project',
            is_active: true,
            created_at: '2026-09-20T04:13:23.000Z',
            status: 'configured',
            last_synced_at: null,
            webhook_path: '/api/webhooks/sync/sync-1/github',
          },
        ],
      })
    );

    const [config] = await projectsApi.getSyncConfigs('proj-1');

    expect(config.status).toBe('configured');
    expect(config.last_synced_at).toBeNull();
    expect(config.webhook_path).toBe('/api/webhooks/sync/sync-1/github');
  });
});
