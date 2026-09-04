import type { Page } from '@playwright/test';
import {
  createMockJwt,
  adminPermissions,
  mockAdminUser,
  routeApi,
} from './test-utils';

const mockOrganization = {
  id: 'org-1',
  name: 'Acme Corp',
  slug: 'acme-corp',
  description: null,
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const mockWorkspace = {
  id: 'ws-1',
  name: 'Engineering',
  slug: 'engineering',
  description: null,
  organization_id: 'org-1',
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const mockMembers = [
  {
    user_id: mockAdminUser.id,
    email: 'admin@example.com',
    display_name: 'Admin User',
    role: 'admin',
  },
  {
    user_id: 'user-2',
    email: 'dev@example.com',
    display_name: 'Developer',
    role: 'member',
  },
  {
    user_id: 'user-3',
    email: 'designer@example.com',
    display_name: 'Designer',
    role: 'member',
  },
].map((member, index) => ({
  ...member,
  id: `member-${index}`,
  organization_id: 'org-1',
  workspace_id: 'ws-1',
  is_active: true,
  joined_at: '2024-01-01T00:00:00Z',
}));

export const mockModels = [
  {
    name: 'llama3.2:latest',
    size: 4700000000,
    modified_at: '2024-01-15T10:30:00Z',
    details: { family: 'llama', description: 'Meta Llama 3.2' },
  },
  {
    name: 'mistral:7b',
    size: 4100000000,
    modified_at: '2024-01-14T08:00:00Z',
    details: { family: 'mistral', description: 'Mistral 7B' },
  },
  {
    name: 'codellama:13b',
    size: 7300000000,
    modified_at: '2024-01-13T14:20:00Z',
    details: { family: 'llama', description: 'Code Llama 13B' },
  },
];

export const mockChats = [
  {
    id: 'chat-1',
    title: 'Code Review Discussion',
    model: 'llama3.2:latest',
    model_name: 'llama3.2:latest',
    archived: false,
    agent_enabled: false,
    agent_sandboxed: true,
    created_at: '2024-01-15T10:00:00Z',
    updated_at: '2024-01-15T12:30:00Z',
    message_count: 12,
  },
  {
    id: 'chat-2',
    title: 'API Design Planning',
    model: 'mistral:7b',
    model_name: 'mistral:7b',
    archived: false,
    agent_enabled: false,
    agent_sandboxed: true,
    created_at: '2024-01-14T09:00:00Z',
    updated_at: '2024-01-14T16:45:00Z',
    message_count: 8,
  },
];

// Projects - matching Project interface
export const mockProjects = [
  {
    id: 'proj-1',
    name: 'Zone Platform',
    description: 'Main platform development',
    status: 'active',
    github_repo_url: null,
    source_id: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-15T10:00:00Z',
  },
  {
    id: 'proj-2',
    name: 'Mobile App',
    description: 'iOS and Android applications',
    status: 'active',
    github_repo_url: null,
    source_id: null,
    created_at: '2024-01-05T00:00:00Z',
    updated_at: '2024-01-14T15:00:00Z',
  },
];

// Tasks - matching Task interface with ALL required fields
export const mockTasks = [
  {
    id: 'task-1',
    project_ids: ['proj-1'],
    workspace_id: 'ws-1',
    title: 'Implement authentication',
    description: 'Add OAuth2 support for the platform',
    acceptance_criteria: 'Users can login with Google and GitHub',
    status: 'in_progress',
    priority: 1,
    model_name: 'llama3.2:latest',
    dependencies: [],
    created_at: '2024-01-10T00:00:00Z',
    updated_at: '2024-01-15T08:00:00Z',
    started_at: '2024-01-12T09:00:00Z',
    completed_at: null,
    is_agentic: true,
    github_repo_url: null,
    source_id: null,
    source_ids: [],
    queued_at: '2024-01-12T08:30:00Z',
    worker_id: 'worker-1',
    pr_url: null,
    branch_name: 'feature/auth',
    pr_status: null,
    pr_created_at: null,
  },
  {
    id: 'task-2',
    project_ids: ['proj-1'],
    workspace_id: 'ws-1',
    title: 'Setup CI/CD pipeline',
    description: 'Configure GitHub Actions for automated testing',
    acceptance_criteria: 'All tests run on PR',
    status: 'complete',
    priority: 2,
    model_name: 'mistral:7b',
    dependencies: [],
    created_at: '2024-01-08T00:00:00Z',
    updated_at: '2024-01-12T16:00:00Z',
    started_at: '2024-01-09T10:00:00Z',
    completed_at: '2024-01-12T16:00:00Z',
    is_agentic: false,
    github_repo_url: null,
    source_id: null,
    source_ids: [],
    queued_at: '2024-01-09T09:00:00Z',
    worker_id: null,
    pr_url: 'https://github.com/example/repo/pull/42',
    branch_name: 'feature/ci-cd',
    pr_status: 'merged',
    pr_created_at: '2024-01-11T14:00:00Z',
  },
];

// Sources - matching Source interface with ALL required fields
export const mockSources = [
  {
    id: 'src-1',
    name: 'Documentation',
    source_type: 'web',
    category: 'web',
    config: { url: 'https://docs.example.com' },
    description: 'Project documentation website',
    url: 'https://docs.example.com',
    is_active: true,
    last_verified_at: '2024-01-15T06:00:00Z',
    last_error: null,
    created_at: '2024-01-10T00:00:00Z',
    updated_at: '2024-01-15T06:00:00Z',
  },
  {
    id: 'src-2',
    name: 'Main Repository',
    source_type: 'github',
    category: 'file',
    config: { owner: 'example', repo: 'main-repo', branch: 'main' },
    description: 'Primary codebase',
    url: 'https://github.com/example/main-repo',
    is_active: true,
    last_verified_at: '2024-01-14T12:00:00Z',
    last_error: null,
    created_at: '2024-01-05T00:00:00Z',
    updated_at: '2024-01-14T12:00:00Z',
  },
];

// Sessions - matching expected session fields
const mockSessions = [
  {
    id: 'sess-1',
    user_id: 'user-1',
    ip_address: '192.168.1.1',
    user_agent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)',
    device_info: 'Chrome on macOS',
    location: 'San Francisco, CA',
    created_at: '2024-01-15T08:00:00Z',
    last_active_at: '2024-01-15T12:00:00Z',
    expires_at: '2024-01-22T08:00:00Z',
    is_current: true,
  },
  {
    id: 'sess-2',
    user_id: 'user-1',
    ip_address: '192.168.1.2',
    user_agent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)',
    device_info: 'Safari on iPhone',
    location: 'San Francisco, CA',
    created_at: '2024-01-14T06:00:00Z',
    last_active_at: '2024-01-14T18:00:00Z',
    expires_at: '2024-01-21T06:00:00Z',
    is_current: false,
  },
];

// Knowledge entries - matching KnowledgeEntry interface
export const mockKnowledge = [
  {
    id: 'kb-1',
    workspace_id: 'ws-1',
    title: 'Getting Started Guide',
    content: 'Welcome to Zone! This guide will help you get started...',
    fetched_content:
      'Welcome to Zone! This guide will help you get started with the platform.',
    type: 'text',
    url: null,
    tags: ['guide', 'getting-started'],
    last_refreshed_at: '2024-01-15T10:00:00Z',
    created_at: '2024-01-10T00:00:00Z',
    updated_at: '2024-01-15T10:00:00Z',
  },
  {
    id: 'kb-2',
    workspace_id: 'ws-1',
    title: 'API Reference',
    content: 'Complete API documentation for Zone...',
    fetched_content:
      'Complete API documentation for Zone platform including all endpoints.',
    type: 'text',
    url: null,
    tags: ['api', 'documentation'],
    last_refreshed_at: '2024-01-14T15:00:00Z',
    created_at: '2024-01-08T00:00:00Z',
    updated_at: '2024-01-14T15:00:00Z',
  },
];

// Setup admin auth with all permissions
export async function setupAdminAuth(page: Page) {
  const fullAdminPermissions = [
    ...adminPermissions,
    'organizations:read',
    'organizations:update',
    'organizations:delete',
    'workspaces:read',
    'workspaces:update',
    'workspaces:delete',
  ];

  const token = createMockJwt({
    sub: mockAdminUser.id,
    email: mockAdminUser.email,
    roles: ['admin', 'user'],
    permissions: fullAdminPermissions,
    exp: Math.floor(Date.now() / 1000) + 3600,
  });

  // Set auth tokens in localStorage
  await page.evaluate(
    ({ token, user }) => {
      localStorage.setItem('manager_access_token', token);
      localStorage.setItem('manager_refresh_token', 'mock-refresh-token');
      localStorage.setItem('manager_user', JSON.stringify(user));
    },
    { token, user: mockAdminUser }
  );
}

export async function setupCommonRoutes(
  page: Page,
  populated = false
): Promise<void> {
  await routeApi(page, /\/api\//, async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    let body: unknown;
    if (path.endsWith('/theme')) {
      await route.fulfill({ status: 404, json: { error: 'Theme not found' } });
      return;
    }
    if (path.endsWith('/members'))
      body = { members: populated ? mockMembers : [] };
    else if (path === '/api/organizations')
      body = { organizations: [mockOrganization] };
    else if (/\/organizations\/[^/]+\/workspaces$/.test(path))
      body = { workspaces: [mockWorkspace] };
    else if (/\/organizations\/[^/]+$/.test(path))
      body = { organization: mockOrganization };
    else if (/\/workspaces\/[^/]+$/.test(path))
      body = { workspace: mockWorkspace };
    else if (path === '/api/models')
      body = { models: populated ? mockModels : [], next_cursor: null };
    else if (path === '/api/chats')
      body = { chats: populated ? mockChats : [] };
    else if (path === '/api/projects')
      body = { projects: populated ? mockProjects : [] };
    else if (path.endsWith('/sync')) body = { configs: [] };
    else if (path.endsWith('/tasks'))
      body = { tasks: populated ? mockTasks : [] };
    else if (path.endsWith('/runs')) body = { runs: [] };
    else if (path.endsWith('/sources/types')) body = { types: [] };
    else if (path.endsWith('/sources'))
      body = { sources: populated ? mockSources : [] };
    else if (path === '/api/knowledge')
      body = { entries: populated ? mockKnowledge : [] };
    else if (path === '/api/auth/sessions')
      body = { sessions: populated ? mockSessions : [] };
    else if (path === '/api/context/search') body = { results: [], total: 0 };
    else {
      await route.fallback();
      return;
    }
    await route.fulfill({ status: 200, json: body });
  });
}
