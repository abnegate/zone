import { expect, test } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { blockServiceWorker, routeApi, routeApiContext } from './test-utils';

const tasksRoutePattern = /\/api\/workspaces\/[^/]+\/tasks/;
const sourcesRoutePattern = /\/api\/workspaces\/[^/]+\/sources/;

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

const workspaceId = '00000000-0000-0000-0000-000000000001';

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
  workspace_id: workspaceId,
  project_ids: [projectId],
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
  pr_url: null,
  branch_name: null,
  pr_status: null,
  pr_created_at: null,
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
  test.beforeEach(async ({ context, page }) => {
    // Block service worker
    await blockServiceWorker(context);

    // Set up API mocks
    await mockCommonEndpoints(page);

    // Organization mock data
    const orgMock = {
      organizations: [
        {
          id: '00000000-0000-0000-0000-000000000001',
          name: 'Default Org',
          slug: 'default',
          description: null,
          is_active: true,
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        },
      ],
    };

    // Workspace mock data
    const workspaceMock = {
      workspaces: [
        {
          id: '00000000-0000-0000-0000-000000000001',
          organization_id: '00000000-0000-0000-0000-000000000001',
          name: 'Default Workspace',
          slug: 'default',
          description: null,
          is_active: true,
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        },
      ],
    };

    // Mock organizations (with and without query params)
    await routeApi(page, '**/api/organizations?*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(orgMock),
      });
    });
    await routeApi(page, '**/api/organizations', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(orgMock),
      });
    });

    // Mock workspaces (with and without query params)
    await routeApi(page, '**/api/organizations/*/workspaces?*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(workspaceMock),
      });
    });
    await routeApi(page, '**/api/organizations/*/workspaces', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(workspaceMock),
      });
    });

    // Mock projects endpoint
    await routeApi(page, '**/api/projects*', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, projects: mockProjects }),
        });
      }
    });

    // Mock sources endpoint
    await routeApi(page, sourcesRoutePattern, (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ sources: mockSources }),
        });
      }
    });

    // Default tasks mock - empty
    await routeApi(page, tasksRoutePattern, (route) => {
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
    await expect(page.locator('.tasks-page')).toBeVisible({ timeout: 10000 });
  });

  test.describe('Page Header', () => {
    test('displays page title and subtitle', async ({ page }) => {
      await expect(page.getByRole('heading', { name: 'Tasks', exact: true })).toBeVisible();
      await expect(page.getByText('Autonomous agent workflows')).toBeVisible();
    });

    test('shows new task button', async ({ page }) => {
      const newTaskButton = page.getByRole('button', { name: /New Task/ });
      await expect(newTaskButton).toBeVisible({ timeout: 10000 });
      await expect(newTaskButton).toContainText('New Task');
    });
  });

  test.describe('Empty State', () => {
    test('shows empty state when no tasks exist', async ({ page }) => {
      await expect(page.getByRole('heading', { name: 'No tasks yet' })).toBeVisible();
      await expect(
        page.getByText('Create your first task to automate your workflow')
      ).toBeVisible();
      await expect(page.getByRole('button', { name: 'Create Task' })).toBeVisible();
    });
  });

  test.describe('Task List', () => {
    test.beforeEach(async ({ context, page }) => {
      // Unroute any existing context-level and page-level routes for tasks
      await context.unroute(tasksRoutePattern);
      await page.unroute(tasksRoutePattern);

      // Set up the tasks route at context level (takes precedence over page routes)
      await routeApiContext(context, tasksRoutePattern, (route) => {
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

      // Navigate to trigger fresh data fetch
      await page.goto('/');
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.goto('/tasks');
      await expect(page).toHaveURL('/tasks');
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

    test('displays status badge text', async ({ page }) => {
      const taskCards = page.locator('.task-card');
      await expect(taskCards.nth(0).locator('.task-badges')).toContainText('created');
      await expect(taskCards.nth(1).locator('.task-badges')).toContainText('in progress');
      await expect(taskCards.nth(2).locator('.task-badges')).toContainText('complete');
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

    test('filters by project', async ({ context, page }) => {
      await context.unroute(tasksRoutePattern);
      await page.unroute(tasksRoutePattern);
      await routeApiContext(context, tasksRoutePattern, (route) => {
        const url = new URL(route.request().url());
        const projectId = url.searchParams.get('project_id');

        if (projectId === 'proj-1') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              tasks: mockTasks.filter((t) => t.project_ids.includes('proj-1')),
            }),
          });
        } else {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ tasks: mockTasks }),
          });
        }
      });

      await page.goto('/');
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.goto('/tasks');
      await expect(page).toHaveURL('/tasks');
      await expect(page.locator('.task-card')).toHaveCount(3);

      await page.selectOption('.filters select:first-of-type', 'proj-1');

      await expect(page.locator('.task-card')).toHaveCount(2);
    });

    test('filters by status', async ({ context, page }) => {
      await context.unroute(tasksRoutePattern);
      await page.unroute(tasksRoutePattern);
      await routeApiContext(context, tasksRoutePattern, (route) => {
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

      await page.goto('/');
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.goto('/tasks');
      await expect(page).toHaveURL('/tasks');
      await expect(page.locator('.task-card')).toHaveCount(3);

      await page.selectOption('.filters select:last-of-type', 'complete');

      await expect(page.locator('.task-card')).toHaveCount(1);
    });
  });

  test.describe('Create Task Modal', () => {
    test('opens create modal from header button', async ({ page }) => {
      const newTaskButton = page.getByRole('button', { name: /New Task/ });
      await expect(newTaskButton).toBeEnabled({ timeout: 10000 });
      await newTaskButton.click();
      await expect(page.getByRole('dialog', { name: 'New Task' })).toBeVisible();
    });

    test('shows project selection options', async ({ page }) => {
      const newTaskButton = page.getByRole('button', { name: /New Task/ });
      await expect(newTaskButton).toBeEnabled({ timeout: 10000 });
      await newTaskButton.click();

      await expect(page.locator('.project-selection-option')).toHaveCount(2);
      await expect(page.locator('.project-selection-option').first()).toContainText('Frontend App');
    });

    test('shows all form fields', async ({ page }) => {
      const newTaskButton = page.getByRole('button', { name: /New Task/ });
      await expect(newTaskButton).toBeEnabled({ timeout: 10000 });
      await newTaskButton.click();

      await page.locator('.project-selection-option').first().click();
      await page.getByRole('button', { name: 'Next' }).click();
      await expect(page.locator('#task-title')).toBeVisible();
      await expect(page.locator('#task-description')).toBeVisible();
      await expect(page.locator('#task-criteria')).toBeVisible();

      await page.fill('#task-title', 'Task title');
      await page.fill('#task-description', 'Task description');
      await page.getByRole('button', { name: 'Next' }).click();
      await expect(page.locator('.toggle-title')).toContainText('Enable Agentic Mode');
    });

    test('shows source dropdown when agentic mode enabled', async ({ page }) => {
      const newTaskButton = page.getByRole('button', { name: /New Task/ });
      await expect(newTaskButton).toBeEnabled({ timeout: 10000 });
      await newTaskButton.click();

      await page.locator('.project-selection-option').first().click();
      await page.getByRole('button', { name: 'Next' }).click();
      await page.fill('#task-title', 'Task title');
      await page.fill('#task-description', 'Task description');
      await page.getByRole('button', { name: 'Next' }).click();

      await page.locator('.toggle-label').click();
      await expect(page.locator('#task-source')).toBeVisible();
    });

    test('creates task successfully', async ({ context, page }) => {
      const newTask = generateMockTask('new-task', 'proj-1', 'New Task', {
        description: 'New task description',
      });

      // Clear existing routes and add POST handler
      await context.unroute(tasksRoutePattern);
      await page.unroute(tasksRoutePattern);
      await routeApiContext(context, tasksRoutePattern, (route) => {
        if (route.request().method() === 'POST') {
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({ task: newTask }),
          });
        } else if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ tasks: [] }),
          });
        } else {
          route.continue();
        }
      });

      const newTaskButton = page.getByRole('button', { name: /New Task/ });
      await expect(newTaskButton).toBeEnabled({ timeout: 10000 });
      await newTaskButton.click();
      await page.locator('.project-selection-option').first().click();
      await page.getByRole('button', { name: 'Next' }).click();
      await page.fill('#task-title', 'New Task');
      await page.fill('#task-description', 'New task description');
      await page.getByRole('button', { name: 'Next' }).click();
      const dialog = page.getByRole('dialog', { name: 'New Task' });
      await dialog.getByRole('button', { name: 'Create Task' }).click();

      await expect(dialog).toHaveCount(0);
    });

    test('shows loading state during creation', async ({ page }) => {
      await routeApi(page, tasksRoutePattern, async (route) => {
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
      const newTaskButton = page.getByRole('button', { name: /New Task/ });
      await expect(newTaskButton).toBeEnabled({ timeout: 10000 });
      await newTaskButton.click();
      await page.locator('.project-selection-option').first().click();
      await page.getByRole('button', { name: 'Next' }).click();
      await page.fill('#task-title', 'New Task');
      await page.fill('#task-description', 'Description');
      await page.getByRole('button', { name: 'Next' }).click();
      const dialog = page.getByRole('dialog', { name: 'New Task' });
      await dialog.getByRole('button', { name: 'Create Task' }).click();

      await expect(dialog.getByRole('button', { name: 'Creating...' })).toBeVisible();
    });

    test('shows error when creation fails', async ({ page }) => {
      await routeApi(page, tasksRoutePattern, (route) => {
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
      const newTaskButton = page.getByRole('button', { name: /New Task/ });
      await expect(newTaskButton).toBeEnabled({ timeout: 10000 });
      await newTaskButton.click();
      await page.locator('.project-selection-option').first().click();
      await page.getByRole('button', { name: 'Next' }).click();
      await page.fill('#task-title', 'New Task');
      await page.fill('#task-description', 'Description');
      await page.getByRole('button', { name: 'Next' }).click();
      const dialog = page.getByRole('dialog', { name: 'New Task' });
      await dialog.getByRole('button', { name: 'Create Task' }).click();

      await expect(page.locator('.form-error')).toBeVisible();
    });

    test('closes modal on cancel', async ({ page }) => {
      const newTaskButton = page.getByRole('button', { name: /New Task/ });
      await expect(newTaskButton).toBeEnabled({ timeout: 10000 });
      await newTaskButton.click();
      await page.getByRole('button', { name: 'Cancel' }).click();

      await expect(page.getByRole('dialog', { name: 'New Task' })).toHaveCount(0);
    });
  });

  test.describe('Task Execution', () => {
    const run = {
      id: 'run-123',
      task_id: 'task-1',
      status: 'running',
      current_phase: 'thinking',
      progress_percent: null,
      error_message: null,
    };
    test.beforeEach(async ({ page }) => {
      await routeApi(page, '**/api/tasks/task-1/runs', (route) =>
        route.fulfill({ json: { runs: [] } })
      );
    });

    test('opens an accessible idle dialog and closes with Escape', async ({ page }) => {
      await page.getByRole('button', { name: 'Execute', exact: true }).first().click();
      const dialog = page.getByRole('dialog', { name: 'Implement login page' });
      await expect(dialog).toBeVisible();
      await expect(dialog.getByRole('button', { name: 'Start Execution' })).toBeEnabled();
      await expect(dialog.locator('.execution-logs')).toHaveCount(0);
      await page.keyboard.press('Escape');
      await expect(dialog).not.toBeVisible();
    });

    test('starts a real run and restores it on reopen', async ({ page }) => {
      let started = false;
      await routeApi(page, '**/api/tasks/task-1/runs', (route) =>
        route.fulfill({ json: { runs: started ? [run] : [] } })
      );
      await routeApi(page, '**/api/tasks/task-1/run', (route) => {
        started = true;
        return route.fulfill({ json: { run: { ...run, status: 'pending' } } });
      });
      await routeApi(page, '**/api/task-runs/run-123', (route) => route.fulfill({ json: { run } }));
      await routeApi(page, '**/api/task-runs/run-123/logs', (route) =>
        route.fulfill({
          json: {
            logs: [
              {
                id: 'log-1',
                phase: 'thinking',
                agent_type: 'worker',
                log_level: 'info',
                message: 'Reading project files',
                created_at: '2026-09-05T00:00:00Z',
              },
            ],
          },
        })
      );
      await page.getByRole('button', { name: 'Execute', exact: true }).first().click();
      await page.getByRole('button', { name: 'Start Execution' }).click();
      await expect(page.getByText('Reading project files')).toBeVisible();
      await expect(page.getByText('Running', { exact: true })).toBeVisible();
      await expect(page.getByRole('button', { name: 'Stop Execution' })).toHaveCount(0);
      await page.getByRole('button', { name: 'Close', exact: true }).click();
      await page.getByRole('button', { name: 'Execute', exact: true }).first().click();
      await expect(page.getByText('Reading project files')).toBeVisible();
    });

    for (const theme of ['dark', 'light']) {
      test(`startup failure remains readable in ${theme}`, async ({ page }, testInfo) => {
        await page.evaluate((value) => {
          document.documentElement.setAttribute('data-theme', value);
          document.documentElement.classList.toggle('dark', value === 'dark');
        }, theme);
        await routeApi(page, '**/api/tasks/task-1/run', (route) =>
          route.fulfill({
            status: 503,
            json: { error: 'Worker unavailable. Try again when a worker is connected.' },
          })
        );
        await page.getByRole('button', { name: 'Execute', exact: true }).first().click();
        await page.getByRole('button', { name: 'Start Execution' }).click();
        await expect(page.getByText('Could not start task')).toBeVisible();
        await expect(page.locator('.execution-logs')).toHaveCount(0);
        await expect(page.locator('.task-progress-bar-container')).toHaveCount(0);
        await page.screenshot({
          path: testInfo.outputPath(`task-error-${theme}.png`),
          fullPage: true,
        });
        await page.setViewportSize({ width: 390, height: 844 });
        await expect(page.getByRole('button', { name: 'Try Again' })).toBeInViewport();
        await page.screenshot({
          path: testInfo.outputPath(`task-error-${theme}-mobile.png`),
          fullPage: true,
        });
      });
    }
  });

  test.describe('Delete Task', () => {
    test('delete button shows confirmation dialog', async ({ context, page }) => {
      await context.unroute(tasksRoutePattern);
      await page.unroute(tasksRoutePattern);

      await routeApiContext(context, tasksRoutePattern, (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ tasks: mockTasks }),
          });
        } else if (route.request().method() === 'DELETE') {
          route.fulfill({ status: 204 });
        } else {
          route.continue();
        }
      });

      await page.goto('/');
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.goto('/tasks');
      await expect(page).toHaveURL('/tasks');
      await expect(page.locator('.task-card')).toHaveCount(3);

      await page.evaluate(() => {
        (window as unknown as { originalConfirm: typeof window.confirm }).originalConfirm =
          window.confirm;
        window.confirm = (msg?: string) => {
          (window as unknown as { confirmMessage: string }).confirmMessage = msg || '';
          return false;
        };
      });

      const deleteBtn = page.locator('.task-card:first-child button:has-text("Delete")');
      await deleteBtn.click();

      // Verify confirm was called
      const confirmMessage = await page.evaluate(
        () => (window as unknown as { confirmMessage: string }).confirmMessage
      );
      expect(confirmMessage).toContain('delete');

      // Tasks should still be there since confirm returned false
      await expect(page.locator('.task-card')).toHaveCount(3);
    });

    test('delete is cancelled when confirmation rejected', async ({ context, page }) => {
      await context.unroute(tasksRoutePattern);
      await page.unroute(tasksRoutePattern);

      await routeApiContext(context, tasksRoutePattern, (route) => {
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

      await page.goto('/');
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.goto('/tasks');
      await expect(page).toHaveURL('/tasks');
      await expect(page.locator('.task-card')).toHaveCount(3);

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
    test('shows error when loading tasks fails', async ({ context, page }) => {
      await context.unroute(tasksRoutePattern);
      await page.unroute(tasksRoutePattern);
      await routeApiContext(context, tasksRoutePattern, (route) => {
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Server error' }),
        });
      });

      await page.goto('/');
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.goto('/tasks');
      await expect(page).toHaveURL('/tasks');

      await expect(page.getByText('Server error')).toBeVisible({ timeout: 15000 });
    });
  });

  test.describe('Loading State', () => {
    test('shows skeleton cards while loading', async ({ context, page }) => {
      await context.unroute(tasksRoutePattern);
      await page.unroute(tasksRoutePattern);
      await routeApiContext(context, tasksRoutePattern, async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 500));
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ tasks: mockTasks }),
        });
      });

      await page.goto('/');
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.goto('/tasks');

      await expect(page.locator('.skeleton-card').first()).toBeVisible();
    });
  });

  test.describe('Disabled State', () => {
    test('new task button is disabled when no projects exist', async ({ context, page }) => {
      await context.unroute('**/api/projects*');
      await page.unroute('**/api/projects*');
      await routeApiContext(context, '**/api/projects*', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, projects: [] }),
        });
      });

      await page.goto('/');
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.goto('/tasks');
      await expect(page).toHaveURL('/tasks');

      await expect(page.getByRole('button', { name: /New Task/ })).toBeDisabled();
    });
  });
});
