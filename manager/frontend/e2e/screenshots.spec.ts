import { test, expect, Page, BrowserContext } from '@playwright/test';
import {
  blockServiceWorker,
  createMockJwt,
  adminPermissions,
  mockAdminUser,
} from './test-utils';

// =============================================================================
// Mock Data - Complete schemas matching API types
// =============================================================================

const mockOrganization = {
  id: 'org-1',
  name: 'Acme Corp',
  slug: 'acme-corp',
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const mockWorkspace = {
  id: 'ws-1',
  name: 'Engineering',
  slug: 'engineering',
  organization_id: 'org-1',
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const mockMembers = [
  { id: 'user-1', email: 'admin@example.com', display_name: 'Admin User', role: 'admin' },
  { id: 'user-2', email: 'dev@example.com', display_name: 'Developer', role: 'member' },
  { id: 'user-3', email: 'designer@example.com', display_name: 'Designer', role: 'member' },
];

const mockModels = [
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

const mockChats = [
  {
    id: 'chat-1',
    title: 'Code Review Discussion',
    model: 'llama3.2:latest',
    model_name: 'llama3.2:latest',
    archived: false,
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
    created_at: '2024-01-14T09:00:00Z',
    updated_at: '2024-01-14T16:45:00Z',
    message_count: 8,
  },
];

// Projects - matching Project interface
const mockProjects = [
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
const mockTasks = [
  {
    id: 'task-1',
    project_id: 'proj-1',
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
    project_id: 'proj-1',
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
const mockSources = [
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
const mockKnowledge = [
  {
    id: 'kb-1',
    workspace_id: 'ws-1',
    title: 'Getting Started Guide',
    content: 'Welcome to Zone! This guide will help you get started...',
    fetched_content: 'Welcome to Zone! This guide will help you get started with the platform.',
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
    fetched_content: 'Complete API documentation for Zone platform including all endpoints.',
    type: 'text',
    url: null,
    tags: ['api', 'documentation'],
    last_refreshed_at: '2024-01-14T15:00:00Z',
    created_at: '2024-01-08T00:00:00Z',
    updated_at: '2024-01-14T15:00:00Z',
  },
];

// =============================================================================
// Setup Functions
// =============================================================================

// Setup admin auth with all permissions
async function setupAdminAuth(page: Page) {
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

// Helper to check if request is an API call (not a module import)
function isApiRequest(route: Parameters<Parameters<Page['route']>[1]>[0]): boolean {
  const resourceType = route.request().resourceType();
  // Only intercept fetch/xhr requests, not scripts/modules
  return resourceType === 'fetch' || resourceType === 'xhr';
}

// Setup common API routes
async function setupCommonRoutes(page: Page, populated: boolean = false) {
  // Organizations list
  await page.route('**/api/organizations', (route) => {
    if (!isApiRequest(route)) return route.continue();
    if (route.request().method() === 'GET') {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ organizations: [mockOrganization] }),
      });
    } else {
      route.continue();
    }
  });

  // Organization details
  await page.route('**/api/organizations/*', (route) => {
    if (!isApiRequest(route)) return route.continue();
    const url = route.request().url();
    if (url.includes('/workspaces') || url.includes('/members')) {
      return route.continue();
    }
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(mockOrganization),
    });
  });

  // Workspaces for organization
  await page.route('**/api/organizations/*/workspaces', (route) => {
    if (!isApiRequest(route)) return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ workspaces: [mockWorkspace] }),
    });
  });

  // Workspace details
  await page.route('**/api/workspaces/*', (route) => {
    if (!isApiRequest(route)) return route.continue();
    const url = route.request().url();
    if (url.includes('/members')) return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(mockWorkspace),
    });
  });

  // Organization members
  await page.route('**/api/organizations/*/members**', (route) => {
    if (!isApiRequest(route)) return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ members: populated ? mockMembers : [] }),
    });
  });

  // Workspace members
  await page.route('**/api/workspaces/*/members**', (route) => {
    if (!isApiRequest(route)) return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ members: populated ? mockMembers : [] }),
    });
  });

  // Models
  await page.route('**/api/models**', (route) => {
    if (!isApiRequest(route)) return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ models: populated ? mockModels : [] }),
    });
  });

  // Chats
  await page.route('**/api/chats**', (route) => {
    if (!isApiRequest(route)) return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ chats: populated ? mockChats : [] }),
    });
  });

  // Projects
  await page.route('**/api/projects**', (route) => {
    if (!isApiRequest(route)) return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ projects: populated ? mockProjects : [] }),
    });
  });

  // Tasks
  await page.route('**/api/tasks**', (route) => {
    if (!isApiRequest(route)) return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ tasks: populated ? mockTasks : [] }),
    });
  });

  // Sources
  await page.route('**/api/sources**', (route) => {
    if (!isApiRequest(route)) return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ sources: populated ? mockSources : [] }),
    });
  });

  // Knowledge/Wiki
  await page.route('**/api/knowledge**', (route) => {
    if (!isApiRequest(route)) return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ entries: populated ? mockKnowledge : [] }),
    });
  });

  // Sessions
  await page.route('**/api/auth/sessions**', (route) => {
    if (!isApiRequest(route)) return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ sessions: populated ? mockSessions : [] }),
    });
  });

  // Search
  await page.route('**/api/search**', (route) => {
    if (!isApiRequest(route)) return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ results: [] }),
    });
  });
}

// Helper to verify no errors on page
async function verifyNoErrors(page: Page) {
  // Check for validation error text
  const validationError = page.locator('text=/Validation failed/i');
  await expect(validationError).not.toBeVisible({ timeout: 1000 }).catch(() => {});

  // Check for unexpected token errors
  const tokenError = page.locator('text=/Unexpected token/i');
  await expect(tokenError).not.toBeVisible({ timeout: 1000 }).catch(() => {});

  // Check for generic error messages
  const genericError = page.locator('[class*="error"]').filter({ hasText: /failed|error/i });
  const errorCount = await genericError.count();
  if (errorCount > 0) {
    const errorText = await genericError.first().textContent();
    if (errorText && (errorText.includes('Validation failed') || errorText.includes('Unexpected token'))) {
      throw new Error(`Page has error: ${errorText}`);
    }
  }
}

// =============================================================================
// Tests
// =============================================================================

test.describe('Screenshots - Public Pages', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    // Clear localStorage before page loads
    await page.addInitScript(() => {
      localStorage.clear();
    });
    // Mock auth refresh to fail immediately so the page doesn't hang
    await page.route('**/api/auth/refresh', (route) => {
      route.fulfill({
        status: 401,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Unauthorized' }),
      });
    });
  });

  test('Login page', async ({ page }) => {
    await page.goto('/login');
    await page.waitForLoadState('networkidle');

    await expect(page.locator('input[type="email"]')).toBeVisible({ timeout: 10000 });
    await page.screenshot({ path: 'screenshots/login.png', fullPage: true });
  });

  test('Register page', async ({ page }) => {
    await page.goto('/register');
    await page.waitForLoadState('networkidle');

    await expect(page.locator('input[type="email"]')).toBeVisible({ timeout: 10000 });
    await page.screenshot({ path: 'screenshots/register.png', fullPage: true });
  });

  test('Forgot password page', async ({ page }) => {
    await page.goto('/forgot-password');
    await page.waitForLoadState('networkidle');

    await expect(page.locator('input[type="email"]')).toBeVisible({ timeout: 10000 });
    await page.screenshot({ path: 'screenshots/forgot-password.png', fullPage: true });
  });

  test('Unauthorized page', async ({ page }) => {
    await page.goto('/unauthorized');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(500);
    await page.screenshot({ path: 'screenshots/unauthorized.png', fullPage: true });
  });
});

test.describe('Screenshots - Empty States', () => {
  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  test('Models page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/models-empty.png', fullPage: true });
  });

  test('Chats page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/chats');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/chats-empty.png', fullPage: true });
  });

  test('Projects page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/projects');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/projects-empty.png', fullPage: true });
  });

  test('Tasks page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/tasks');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/tasks-empty.png', fullPage: true });
  });

  test('Sources page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/sources');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/sources-empty.png', fullPage: true });
  });

  test('Wiki page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/wiki');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/wiki-empty.png', fullPage: true });
  });

  test('Sessions page (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/sessions');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/sessions-empty.png', fullPage: true });
  });

  test('Organization settings (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/org-settings');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/org-settings-empty.png', fullPage: true });
  });

  test('Workspace settings (empty)', async ({ page }) => {
    await setupCommonRoutes(page, false);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/settings');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/workspace-settings-empty.png', fullPage: true });
  });
});

test.describe('Screenshots - Populated States', () => {
  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  test('Models page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/models-populated.png', fullPage: true });
  });

  test('Chats page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/chats');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/chats-populated.png', fullPage: true });
  });

  test('Projects page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/projects');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/projects-populated.png', fullPage: true });
  });

  test('Tasks page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/tasks');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/tasks-populated.png', fullPage: true });
  });

  test('Sources page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/sources');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/sources-populated.png', fullPage: true });
  });

  test('Wiki page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/wiki');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/wiki-populated.png', fullPage: true });
  });

  test('Sessions page (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/sessions');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/sessions-populated.png', fullPage: true });
  });

  test('Organization settings (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/org-settings');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/org-settings-populated.png', fullPage: true });
  });

  test('Workspace settings (populated)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/settings');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/workspace-settings-populated.png', fullPage: true });
  });

  test('Search page', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/search');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/search.png', fullPage: true });
  });
});

// =============================================================================
// Dark Mode Screenshots
// =============================================================================

test.describe('Screenshots - Dark Mode', () => {
  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  async function enableDarkMode(page: Page) {
    await page.evaluate(() => {
      document.documentElement.setAttribute('data-theme', 'dark');
    });
    await page.waitForTimeout(100);
  }

  test('Login page (dark)', async ({ page }) => {
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await enableDarkMode(page);

    await expect(page.locator('input[type="email"]')).toBeVisible({ timeout: 10000 });
    await page.screenshot({ path: 'screenshots/dark-login.png', fullPage: true });
  });

  test('Models page (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await enableDarkMode(page);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/dark-models.png', fullPage: true });
  });

  test('Projects page (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/projects');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await enableDarkMode(page);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/dark-projects.png', fullPage: true });
  });

  test('Tasks page (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/tasks');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await enableDarkMode(page);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/dark-tasks.png', fullPage: true });
  });

  test('Sources page (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/sources');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await enableDarkMode(page);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/dark-sources.png', fullPage: true });
  });

  test('Wiki page (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/wiki');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await enableDarkMode(page);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/dark-wiki.png', fullPage: true });
  });

  test('Chats page (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/chats');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });
    await enableDarkMode(page);
    await verifyNoErrors(page);
    await page.waitForTimeout(300);
    await page.screenshot({ path: 'screenshots/dark-chats.png', fullPage: true });
  });
});

// =============================================================================
// Modal Screenshots
// =============================================================================

test.describe('Screenshots - Modals', () => {
  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  test('New Project modal', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/projects');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });

    // Click new project button
    const newProjectBtn = page.locator('button').filter({ hasText: /New Project/i });
    if (await newProjectBtn.isVisible()) {
      await newProjectBtn.click();
      await page.waitForTimeout(300);
      await page.screenshot({ path: 'screenshots/modal-new-project.png', fullPage: true });
    }
  });

  test('New Task modal', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/tasks');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });

    // Click new task button
    const newTaskBtn = page.locator('button').filter({ hasText: /New Task/i });
    if (await newTaskBtn.isVisible()) {
      await newTaskBtn.click();
      await page.waitForTimeout(300);
      await page.screenshot({ path: 'screenshots/modal-new-task.png', fullPage: true });
    }
  });

  test('Add Source modal', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/sources');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });

    // Click add source button
    const addSourceBtn = page.locator('button').filter({ hasText: /Add Source/i });
    if (await addSourceBtn.isVisible()) {
      await addSourceBtn.click();
      await page.waitForTimeout(300);
      await page.screenshot({ path: 'screenshots/modal-add-source.png', fullPage: true });
    }
  });

  test('Add Knowledge modal', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/wiki');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });

    // Click add knowledge button
    const addKnowledgeBtn = page.locator('button').filter({ hasText: /Add Knowledge/i });
    if (await addKnowledgeBtn.isVisible()) {
      await addKnowledgeBtn.click();
      await page.waitForTimeout(300);
      await page.screenshot({ path: 'screenshots/modal-add-knowledge.png', fullPage: true });
    }
  });

  test('New Project modal (dark)', async ({ page }) => {
    await setupCommonRoutes(page, true);
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await setupAdminAuth(page);

    await page.goto('/projects');
    await page.waitForLoadState('networkidle');
    await expect(page.getByRole('navigation')).toBeVisible({ timeout: 10000 });

    // Enable dark mode
    await page.evaluate(() => {
      document.documentElement.setAttribute('data-theme', 'dark');
    });
    await page.waitForTimeout(100);

    // Click new project button
    const newProjectBtn = page.locator('button').filter({ hasText: /New Project/i });
    if (await newProjectBtn.isVisible()) {
      await newProjectBtn.click();
      await page.waitForTimeout(300);
      await page.screenshot({ path: 'screenshots/dark-modal-new-project.png', fullPage: true });
    }
  });
});
