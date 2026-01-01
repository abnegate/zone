import { test, expect } from '@playwright/test';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';

// Mock data generators
const generateMockProject = (id: string, name: string) => ({
  id,
  name,
  description: `${name} description`,
  status: 'active',
  source_id: null,
  github_repo_url: null,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
});

const generateMockSource = (id: string, name: string, type: string) => ({
  id,
  name,
  source_type: type,
  category: type === 'github' || type === 'gitlab' || type === 'filesystem' ? 'file' : 'web',
  config: { owner: 'test', repo: 'test' },
  url: `https://example.com/${name}`,
  description: null,
  is_active: true,
  last_verified_at: null,
  last_error: null,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
});

const generateMockTask = (
  id: string,
  projectId: string,
  title: string,
  options: {
    status?: string;
    description?: string;
    priority?: number;
    is_agentic?: boolean;
    source_id?: string | null;
    model_name?: string | null;
  } = {}
) => ({
  id,
  project_id: projectId,
  title,
  description: options.description || 'Task description',
  status: options.status || 'created',
  priority: options.priority || 1,
  is_agentic: options.is_agentic ?? false,
  source_id: options.source_id || null,
  source_ids: options.source_id ? [options.source_id] : [],
  model_name: options.model_name || null,
  acceptance_criteria: null,
  dependencies: [],
  github_repo_url: null,
  started_at: null,
  completed_at: null,
  queued_at: null,
  worker_id: null,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
});

const mockProjects = [
  generateMockProject('proj-1', 'Frontend App'),
  generateMockProject('proj-2', 'Backend API'),
];

const mockSources = [
  generateMockSource('src-1', 'frontend-repo', 'github'),
  generateMockSource('src-2', 'backend-repo', 'gitlab'),
];

const mockTasks = [
  generateMockTask('task-1', 'proj-1', 'Implement login page', {
    status: 'created',
    priority: 1,
    is_agentic: true,
    source_id: 'src-1',
  }),
  generateMockTask('task-2', 'proj-1', 'Fix navigation bug', {
    status: 'in_progress',
    priority: 2,
  }),
  generateMockTask('task-3', 'proj-2', 'Add API endpoint', {
    status: 'complete',
    priority: 3,
    is_agentic: true,
  }),
];

test.describe('Tasks Page', () => {
  test.beforeEach(async ({ page }) => {
    // Set up API mocks
    await mockCommonEndpoints(page);

    // Mock projects endpoint
    await page.route('**/api/projects*', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, projects: mockProjects }),
        });
      }
    });

    // Mock sources endpoint
    await page.route('**/api/sources*', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ sources: mockSources }),
        });
      }
    });

    // Default tasks mock - empty
    await page.route('**/api/tasks*', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ tasks: [] }),
        });
      } else {
        route.continue();
      }
    });

    // Navigate and set up auth
    await page.goto('/');
    await setupAuth(page);
    await page.reload();

    // Wait for app to load
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

    // Navigate to tasks page
    await page.click('a[href="/tasks"]');
    await expect(page).toHaveURL('/tasks');
  });

  test.describe('Page Header', () => {
    test('displays page title and subtitle', async ({ page }) => {
      await expect(page.locator('.page-header h1')).toContainText('Tasks');
      await expect(page.locator('.page-header .subtitle')).toContainText('agent');
    });

    test('shows new task button', async ({ page }) => {
      await expect(page.locator('.page-header .btn-primary')).toContainText('New Task');
    });
  });

  test.describe('Empty State', () => {
    test('shows empty state when no tasks exist', async ({ page }) => {
      await expect(page.locator('.empty-state')).toBeVisible();
      await expect(page.locator('.empty-state')).toContainText('No tasks found');
    });
  });

  test.describe('Task List', () => {
    test.beforeEach(async ({ page }) => {
      // Set up routes with mock tasks data
      await page.unroute('**/api/tasks*');
      await page.route('**/api/tasks*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ tasks: mockTasks }),
          });
        } else {
          route.continue();
        }
      });

      // Navigate away and back to trigger fresh data fetch
      await page.click('a[href="/"]');
      await page.click('a[href="/tasks"]');
      await expect(page.locator('.task-card').first()).toBeVisible({ timeout: 10000 });
    });

    test('displays list of task cards', async ({ page }) => {
      await expect(page.locator('.task-card')).toHaveCount(3);
    });

    test('displays task title and description', async ({ page }) => {
      await expect(page.locator('.task-card h3').first()).toContainText('Implement login page');
      await expect(page.locator('.task-description').first()).toContainText('Task description');
    });

    test('displays project name for each task', async ({ page }) => {
      await expect(page.locator('.task-project').first()).toContainText('Frontend App');
    });

    test('displays status badge with correct style', async ({ page }) => {
      await expect(page.locator('.task-status-badge.badge-gray')).toContainText('created');
      await expect(page.locator('.task-status-badge.badge-yellow')).toContainText('in progress');
      await expect(page.locator('.task-status-badge.badge-green')).toContainText('complete');
    });

    test('shows agentic badge for agentic tasks', async ({ page }) => {
      await expect(page.locator('.task-agentic-badge')).toHaveCount(2);
    });

    test('displays priority in task meta', async ({ page }) => {
      await expect(page.locator('.task-priority').first()).toContainText('Priority: 1');
    });
  });

  test.describe('Filters', () => {
    test('displays project and status filter dropdowns', async ({ page }) => {
      await expect(page.locator('.filters select')).toHaveCount(2);
    });

    test('filters by project', async ({ page }) => {
      await page.unroute('**/api/tasks*');
      await page.route('**/api/tasks*', (route) => {
        const url = new URL(route.request().url());
        const projectId = url.searchParams.get('project_id');

        if (projectId === 'proj-1') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ tasks: mockTasks.filter((t) => t.project_id === 'proj-1') }),
          });
        } else {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ tasks: mockTasks }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/tasks"]');

      await page.selectOption('.filters select:first-of-type', 'proj-1');

      await expect(page.locator('.task-card')).toHaveCount(2);
    });

    test('filters by status', async ({ page }) => {
      await page.unroute('**/api/tasks*');
      await page.route('**/api/tasks*', (route) => {
        const url = new URL(route.request().url());
        const status = url.searchParams.get('status');

        if (status === 'complete') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ tasks: mockTasks.filter((t) => t.status === 'complete') }),
          });
        } else {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ tasks: mockTasks }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/tasks"]');

      await page.selectOption('.filters select:last-of-type', 'complete');

      await expect(page.locator('.task-card')).toHaveCount(1);
    });
  });

  test.describe('Create Task Modal', () => {
    test('opens create modal from header button', async ({ page }) => {
      await page.click('.page-header .btn-primary');
      await expect(page.locator('.modal-content h2')).toContainText('Create New Task');
    });

    test('shows project dropdown with available projects', async ({ page }) => {
      await page.click('.page-header .btn-primary');

      const projectSelect = page.locator('#project');
      await expect(projectSelect.locator('option')).toHaveCount(2);
      await expect(projectSelect.locator('option').first()).toContainText('Frontend App');
    });

    test('shows all form fields', async ({ page }) => {
      await page.click('.page-header .btn-primary');

      await expect(page.locator('#project')).toBeVisible();
      await expect(page.locator('#title')).toBeVisible();
      await expect(page.locator('#description')).toBeVisible();
      await expect(page.locator('#criteria')).toBeVisible();
      await expect(page.locator('#priority')).toBeVisible();
      await expect(page.locator('#isAgentic')).toBeVisible();
    });

    test('shows source dropdown when agentic mode enabled', async ({ page }) => {
      await page.click('.page-header .btn-primary');

      // Enable agentic mode
      await page.check('#isAgentic');

      await expect(page.locator('#sourceId')).toBeVisible();
    });

    test('creates task successfully', async ({ page }) => {
      const newTask = generateMockTask('new-task', 'proj-1', 'New Task', {
        description: 'New task description',
      });

      await page.route('**/api/tasks', (route) => {
        if (route.request().method() === 'POST') {
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({ task: newTask }),
          });
        }
      });

      await page.click('.page-header .btn-primary');
      await page.fill('#title', 'New Task');
      await page.fill('#description', 'New task description');
      await page.click('.form-actions .btn-primary');

      await expect(page.locator('.modal-content')).not.toBeVisible({ timeout: 5000 });
    });

    test('shows loading state during creation', async ({ page }) => {
      await page.route('**/api/tasks', async (route) => {
        if (route.request().method() === 'POST') {
          await new Promise((resolve) => setTimeout(resolve, 500));
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({
              task: generateMockTask('new-task', 'proj-1', 'New Task'),
            }),
          });
        } else {
          route.continue();
        }
      });

      // Wait for button to be enabled (projects loaded)
      await expect(page.locator('.page-header .btn-primary')).toBeEnabled({ timeout: 10000 });
      await page.click('.page-header .btn-primary');
      await page.fill('#title', 'New Task');
      await page.fill('#description', 'Description');
      await page.click('.form-actions .btn-primary');

      await expect(page.locator('.form-actions .btn-primary')).toContainText('Creating...');
    });

    test('shows error when creation fails', async ({ page }) => {
      await page.route('**/api/tasks', (route) => {
        if (route.request().method() === 'POST') {
          route.fulfill({
            status: 400,
            contentType: 'application/json',
            body: JSON.stringify({ error: 'Validation failed' }),
          });
        } else {
          route.continue();
        }
      });

      // Wait for button to be enabled (projects loaded)
      await expect(page.locator('.page-header .btn-primary')).toBeEnabled({ timeout: 10000 });
      await page.click('.page-header .btn-primary');
      await page.fill('#title', 'New Task');
      await page.fill('#description', 'Description');
      await page.click('.form-actions .btn-primary');

      await expect(page.locator('.form-error')).toBeVisible();
    });

    test('closes modal on cancel', async ({ page }) => {
      await page.click('.page-header .btn-primary');
      await page.click('.form-actions .btn-secondary');

      await expect(page.locator('.modal-content')).not.toBeVisible();
    });
  });

  test.describe('Task Execution', () => {
    test.beforeEach(async ({ page }) => {
      await page.unroute('**/api/tasks*');
      await page.route('**/api/tasks*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ tasks: mockTasks }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/tasks"]');
      await expect(page.locator('.task-card')).toHaveCount(3);
    });

    test('opens execution modal when Execute clicked', async ({ page }) => {
      await page.click('.task-card:first-child button:has-text("Execute")');

      await expect(page.locator('.task-execution-modal')).toBeVisible();
      await expect(page.locator('.task-execution-header h2')).toContainText('Implement login page');
    });

    test('shows Start Execution button in idle state', async ({ page }) => {
      await page.click('.task-card:first-child button:has-text("Execute")');

      await expect(page.locator('.execution-controls button:has-text("Start Execution")')).toBeVisible();
    });

    test('closes execution modal with close button', async ({ page }) => {
      await page.click('.task-card:first-child button:has-text("Execute")');
      await expect(page.locator('.task-execution-modal')).toBeVisible();

      await page.click('.task-execution-header .close-btn');

      await expect(page.locator('.task-execution-modal')).not.toBeVisible();
    });

    test('starts execution and shows progress', async ({ page }) => {
      await page.route('**/api/tasks/task-1/start', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ run_id: 'run-123' }),
        });
      });

      // Mock WebSocket - Playwright doesn't support WebSocket mocking directly,
      // so we test that the UI updates correctly when start is called
      await page.click('.task-card:first-child button:has-text("Execute")');
      await page.click('.execution-controls button:has-text("Start Execution")');

      // After starting, should show Stop Execution button
      await expect(page.locator('.execution-controls button:has-text("Stop Execution")')).toBeVisible();
    });

    test('shows progress bar after starting', async ({ page }) => {
      await page.route('**/api/tasks/task-1/start', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ run_id: 'run-123' }),
        });
      });

      await page.click('.task-card:first-child button:has-text("Execute")');
      await page.click('.execution-controls button:has-text("Start Execution")');

      await expect(page.locator('.task-progress-bar-container')).toBeVisible();
    });

    test('shows execution phases', async ({ page }) => {
      await page.route('**/api/tasks/task-1/start', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ run_id: 'run-123' }),
        });
      });

      await page.click('.task-card:first-child button:has-text("Execute")');
      await page.click('.execution-controls button:has-text("Start Execution")');

      await expect(page.locator('.execution-phases')).toBeVisible();
      await expect(page.locator('.phase-item')).toHaveCount(7);
    });

    test('shows logs section after starting', async ({ page }) => {
      await page.route('**/api/tasks/task-1/start', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ run_id: 'run-123' }),
        });
      });

      await page.click('.task-card:first-child button:has-text("Execute")');
      await page.click('.execution-controls button:has-text("Start Execution")');

      await expect(page.locator('.execution-logs h3')).toContainText('Execution Logs');
    });
  });

  test.describe('Delete Task', () => {
    test.beforeEach(async ({ page }) => {
      await page.unroute('**/api/tasks*');
      await page.unroute('**/api/tasks/**');

      // Route for task list (GET /api/tasks)
      await page.route('**/api/tasks', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ tasks: mockTasks }),
          });
        } else {
          route.continue();
        }
      });

      // Route for individual task operations (DELETE /api/tasks/:id)
      await page.route('**/api/tasks/*', (route) => {
        if (route.request().method() === 'DELETE') {
          route.fulfill({ status: 204 });
        } else {
          route.continue();
        }
      });

      await page.reload();
      await page.click('a[href="/tasks"]');
    });

    test('delete button removes task after confirmation', async ({ page }) => {
      // Mock window.confirm to return true
      await page.evaluate(() => {
        window.confirm = () => true;
      });

      const deleteBtn = page.locator('.task-card:first-child button:has-text("Delete")');
      await deleteBtn.click();

      await expect(page.locator('.task-card')).toHaveCount(2);
    });

    test('delete is cancelled when confirmation rejected', async ({ page }) => {
      // Mock window.confirm to return false
      await page.evaluate(() => {
        window.confirm = () => false;
      });

      const deleteBtn = page.locator('.task-card:first-child button:has-text("Delete")');
      await deleteBtn.click();

      // Task should still be there
      await expect(page.locator('.task-card')).toHaveCount(3);
    });
  });

  test.describe('Error Handling', () => {
    test('shows error when loading tasks fails', async ({ page }) => {
      await page.unroute('**/api/tasks*');
      await page.route('**/api/tasks*', (route) => {
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Server error' }),
        });
      });

      await page.reload();
      await page.click('a[href="/tasks"]');

      await expect(page.locator('.error-banner')).toBeVisible();
    });
  });

  test.describe('Loading State', () => {
    test('shows skeleton cards while loading', async ({ page }) => {
      await page.unroute('**/api/tasks*');
      await page.route('**/api/tasks*', async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 500));
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ tasks: mockTasks }),
        });
      });

      await page.reload();
      await page.click('a[href="/tasks"]');

      await expect(page.locator('.skeleton-card').first()).toBeVisible();
    });
  });

  test.describe('Disabled State', () => {
    test('new task button is disabled when no projects exist', async ({ page }) => {
      await page.unroute('**/api/projects*');
      await page.route('**/api/projects*', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, projects: [] }),
        });
      });

      await page.reload();
      await page.click('a[href="/tasks"]');

      await expect(page.locator('.page-header .btn-primary')).toBeDisabled();
    });
  });
});
