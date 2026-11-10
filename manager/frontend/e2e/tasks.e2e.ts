import type { BrowserContext, Locator, Page, Route } from '@playwright/test';
import { expect, test } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { chooseSelect } from './helpers/select';
import { blockServiceWorker, routeApi, routeApiContext } from './test-utils';

const tasksRoutePattern = /\/api\/workspaces\/[^/]+\/tasks/;
const taskRoutePattern = /\/api\/tasks\/([^/?]+)(?:\?|$)/;
const sourcesRoutePattern = /\/api\/workspaces\/[^/]+\/sources/;

const workspaceId = '00000000-0000-0000-0000-000000000001';

const NOW = new Date('2026-11-10T12:00:00.000Z');
const UPDATED = '2026-11-10T10:00:00.000Z';
const UPDATED_RELATIVE = 'about 2 hours ago';

const STAGE_LABELS = [
  'Planned',
  'In progress',
  'Blocked',
  'Paused',
  'In review',
  'Completed',
  'Cancelled',
];

const REFUSED = 'Task has an active run or is no longer available';

const PHONE = { width: 375, height: 812 };

const PULL_REQUEST = 'https://github.com/test/test/pull/7';

interface TaskOptions {
  status?: string;
  description?: string;
  priority?: number | null;
  is_agentic?: boolean;
  source_id?: string | null;
  model_name?: string | null;
  pr_url?: string | null;
  pr_status?: string | null;
}

const generateMockProject = (id: string, name: string) => ({
  id,
  name,
  description: `${name} description`,
  status: 'active',
  source_id: null,
  github_repo_url: null,
  created_at: UPDATED,
  updated_at: UPDATED,
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
  created_at: UPDATED,
  updated_at: UPDATED,
});

const generateMockTask = (
  id: string,
  projectId: string,
  title: string,
  options: TaskOptions = {}
) => ({
  id,
  workspace_id: workspaceId,
  project_ids: [projectId],
  title,
  description: options.description ?? 'Task description',
  status: options.status ?? 'created',
  priority: options.priority === undefined ? 1 : options.priority,
  is_agentic: options.is_agentic ?? false,
  source_id: options.source_id ?? null,
  source_ids: options.source_id ? [options.source_id] : [],
  model_name: options.model_name ?? null,
  acceptance_criteria: null,
  dependencies: [],
  github_repo_url: null,
  started_at: null,
  completed_at: null,
  queued_at: null,
  worker_id: null,
  pr_url: options.pr_url ?? null,
  branch_name: null,
  pr_status: options.pr_status ?? null,
  pr_created_at: null,
  created_at: UPDATED,
  updated_at: UPDATED,
});

type MockTask = ReturnType<typeof generateMockTask>;

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
  generateMockTask('task-2', 'proj-1', 'Queue nightly build', { status: 'queued', priority: 2 }),
  generateMockTask('task-3', 'proj-1', 'Fix navigation bug', {
    status: 'in_progress',
    priority: 2,
  }),
  generateMockTask('task-4', 'proj-2', 'Wait on API keys', { status: 'blocked', priority: null }),
  generateMockTask('task-5', 'proj-2', 'Migrate billing', { status: 'paused' }),
  generateMockTask('task-6', 'proj-2', 'Add API endpoint', {
    status: 'review',
    priority: 3,
    is_agentic: true,
    pr_url: PULL_REQUEST,
    pr_status: 'open',
  }),
  generateMockTask('task-7', 'proj-1', 'Ship onboarding', { status: 'complete' }),
  generateMockTask('task-8', 'proj-2', 'Drop legacy export', { status: 'cancelled' }),
];

interface Mutation {
  method: string;
  id: string;
  body: unknown;
}

interface TaskServer {
  tasks: MockTask[];
  mutations: Mutation[];
  listed: URL[];
  refuse: boolean;
  hold: Promise<void> | null;
}

function fulfillJson(route: Route, body: unknown, status = 200): Promise<void> {
  return route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
}

async function serveTasks(context: BrowserContext, page: Page): Promise<TaskServer> {
  const server: TaskServer = {
    tasks: mockTasks.map((task) => ({ ...task })),
    mutations: [],
    listed: [],
    refuse: false,
    hold: null,
  };

  await context.unroute(tasksRoutePattern);
  await page.unroute(tasksRoutePattern);
  await routeApiContext(context, tasksRoutePattern, (route) => {
    if (route.request().method() !== 'GET') return route.continue();
    const url = new URL(route.request().url());
    server.listed.push(url);
    const project = url.searchParams.get('project_id');
    const tasks = project
      ? server.tasks.filter((task) => task.project_ids.includes(project))
      : server.tasks;
    return fulfillJson(route, { tasks });
  });

  await routeApiContext(context, taskRoutePattern, async (route) => {
    const request = route.request();
    const id = decodeURIComponent(taskRoutePattern.exec(request.url())?.[1] ?? '');
    const index = server.tasks.findIndex((task) => task.id === id);
    const method = request.method();
    if (method === 'GET') {
      return index === -1
        ? fulfillJson(route, { error: 'Task not found' }, 404)
        : fulfillJson(route, { task: server.tasks[index] });
    }
    server.mutations.push({ method, id, body: request.postDataJSON() });
    if (method === 'PUT') {
      await server.hold;
      if (server.refuse || index === -1) return fulfillJson(route, { error: REFUSED }, 409);
      server.tasks[index] = { ...server.tasks[index], ...request.postDataJSON() };
      return fulfillJson(route, { task: server.tasks[index] });
    }
    if (method === 'DELETE') {
      server.tasks.splice(index, 1);
      return route.fulfill({ status: 204 });
    }
    return route.continue();
  });

  await page.goto('/');
  await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
  await page.goto('/tasks');
  await expect(page).toHaveURL('/tasks');
  await expect(page.getByRole('region', { name: 'Tasks', exact: true })).toBeVisible({
    timeout: 10000,
  });
  return server;
}

function column(page: Page, label: string): Locator {
  return page.locator('section.board-column').filter({
    has: page.getByRole('heading', { name: label, exact: true }),
  });
}

function card(scope: Page | Locator, title: string): Locator {
  const page = 'page' in scope ? scope.page() : scope;
  return scope.locator('li.task-card').filter({
    has: page.getByRole('button', { name: title, exact: true }),
  });
}

function moves(server: TaskServer): Mutation[] {
  return server.mutations.filter((mutation) => mutation.method === 'PUT');
}

async function openMenu(page: Page, title: string): Promise<void> {
  const trigger = page.getByRole('button', { name: `Actions for ${title}`, exact: true });
  await trigger.hover();
  await trigger.click();
}

async function chooseFromMenu(page: Page, title: string, item: string): Promise<void> {
  await openMenu(page, title);
  await page.getByRole('menuitem', { name: item, exact: true }).click();
}

async function center(locator: Locator): Promise<{ x: number; y: number }> {
  const box = await locator.boundingBox();
  if (!box) throw new Error('The element has no layout box');
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

async function pointerDrag(page: Page, title: string, target: Locator): Promise<void> {
  const handle = page.getByRole('button', { name: `Move ${title}`, exact: true });
  await handle.hover();
  const from = await center(handle);
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + 12, from.y + 4, { steps: 4 });
  await expect(page.locator('.task-card-overlay')).toBeVisible();
  const to = await center(target);
  await page.mouse.move(to.x, to.y, { steps: 12 });
  await page.mouse.up();
  await expect(page.locator('.task-card-overlay')).toHaveCount(0);
}

function announcer(page: Page): Locator {
  return page.locator('.board [aria-live="assertive"]');
}

async function pressAndAnnounce(page: Page, key: string, announcement: string): Promise<void> {
  await page.keyboard.press(key);
  await expect(announcer(page)).toHaveText(announcement);
}

interface Point {
  x: number;
  y: number;
}

interface Finger {
  down: (point: Point) => Promise<void>;
  move: (point: Point) => Promise<void>;
  slide: (from: Point, to: Point) => Promise<void>;
  up: () => Promise<void>;
}

const SLIDE_STEP = 4;

async function finger(page: Page): Promise<Finger> {
  const session = await page.context().newCDPSession(page);
  const touch = (type: 'touchStart' | 'touchMove' | 'touchEnd', points: Point[]) =>
    session.send('Input.dispatchTouchEvent', { type, touchPoints: points });
  const move = async (point: Point) => {
    await touch('touchMove', [point]);
  };
  return {
    down: async (point) => {
      await touch('touchStart', [point]);
    },
    move,
    slide: async (from, to) => {
      const steps = Math.ceil(Math.hypot(to.x - from.x, to.y - from.y) / SLIDE_STEP);
      for (let step = 1; step <= steps; step += 1) {
        await move({
          x: from.x + ((to.x - from.x) * step) / steps,
          y: from.y + ((to.y - from.y) * step) / steps,
        });
      }
    },
    up: async () => {
      await touch('touchEnd', []);
    },
  };
}

async function shrinkToPhone(page: Page): Promise<void> {
  await page.setViewportSize(PHONE);
  await expect(page.locator('.main-content')).toHaveCSS('margin-left', '0px');
}

async function recordAnnouncements(page: Page): Promise<() => Promise<string[]>> {
  await announcer(page).evaluate((region) => {
    const announced: string[] = [];
    Object.assign(window, { announced });
    new MutationObserver(() => announced.push(region.textContent ?? '')).observe(region, {
      childList: true,
      characterData: true,
      subtree: true,
    });
  });
  return () => page.evaluate(() => (window as unknown as { announced: string[] }).announced);
}

async function pickUp(page: Page, title: string, stage: string): Promise<Locator> {
  const grip = page.getByRole('button', { name: `Move ${title}`, exact: true });
  await grip.focus();
  await pressAndAnnounce(page, 'Space', `Picked up ${title}, in ${stage}.`);
  await expect(page.locator('.task-card-overlay')).toBeVisible();
  return grip;
}

test.describe('Tasks Page', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    await page.clock.setFixedTime(NOW);
    await mockCommonEndpoints(page);

    const orgMock = {
      organizations: [
        {
          id: '00000000-0000-0000-0000-000000000001',
          name: 'Default Org',
          slug: 'default',
          description: null,
          is_active: true,
          created_at: UPDATED,
          updated_at: UPDATED,
        },
      ],
    };

    const workspaceMock = {
      workspaces: [
        {
          id: '00000000-0000-0000-0000-000000000001',
          organization_id: '00000000-0000-0000-0000-000000000001',
          name: 'Default Workspace',
          slug: 'default',
          description: null,
          is_active: true,
          created_at: UPDATED,
          updated_at: UPDATED,
        },
      ],
    };

    await routeApi(page, '**/api/organizations?*', (route) => fulfillJson(route, orgMock));
    await routeApi(page, '**/api/organizations', (route) => fulfillJson(route, orgMock));
    await routeApi(page, '**/api/organizations/*/workspaces?*', (route) =>
      fulfillJson(route, workspaceMock)
    );
    await routeApi(page, '**/api/organizations/*/workspaces', (route) =>
      fulfillJson(route, workspaceMock)
    );

    let reportProjectsLoaded: () => void = () => {};
    const projectsLoaded = new Promise<void>((resolve) => {
      reportProjectsLoaded = resolve;
    });

    await routeApi(page, '**/api/projects*', async (route) => {
      if (route.request().method() === 'GET') {
        await fulfillJson(route, { success: true, projects: mockProjects });
        reportProjectsLoaded();
      } else {
        await route.continue();
      }
    });

    await routeApi(page, sourcesRoutePattern, (route) => {
      if (route.request().method() === 'GET') {
        return fulfillJson(route, { sources: mockSources });
      }
    });

    await routeApi(page, tasksRoutePattern, (route) => {
      if (route.request().method() === 'GET') {
        return fulfillJson(route, { tasks: [] });
      }
      return route.continue();
    });

    await page.goto('/');
    await setupAuth(page);
    await page.reload();

    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

    await page.click('a[href="/tasks"]');
    await projectsLoaded;
    await expect(page).toHaveURL('/tasks');
    await expect(page.locator('.tasks-page')).toBeVisible({ timeout: 10000 });
    await expect(page.getByRole('button', { name: /New task/ })).toBeEnabled();
  });

  test.describe('Page Header', () => {
    test('displays page title and subtitle', async ({ page }) => {
      await expect(page.getByRole('heading', { name: 'Tasks', exact: true })).toBeVisible();
      await expect(page.getByText('Autonomous agent workflows')).toBeVisible();
    });

    test('shows new task button', async ({ page }) => {
      const newTaskButton = page.getByRole('button', { name: /New task/ });
      await expect(newTaskButton).toBeVisible({ timeout: 10000 });
      await expect(newTaskButton).toContainText('New task');
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

  test.describe('Board', () => {
    test('shows seven columns in order, each counting its tasks', async ({ context, page }) => {
      await serveTasks(context, page);

      await expect(page.locator('section.board-column h2')).toHaveText(STAGE_LABELS);
      await expect(page.locator('.board-column-count')).toHaveText([
        '2',
        '1',
        '1',
        '1',
        '1',
        '1',
        '1',
      ]);
      await expect(page.locator('li.task-card')).toHaveCount(mockTasks.length);
      await expect(page.locator('p.board-column-empty')).toHaveCount(0);

      const planned = column(page, 'Planned');
      await expect(card(planned, 'Implement login page')).toBeVisible();
      const queued = card(planned, 'Queue nightly build');
      await expect(queued.getByText('Queued', { exact: true })).toBeVisible();
      await expect(card(planned, 'Implement login page').getByText('Queued')).toHaveCount(0);

      await expect(card(column(page, 'Paused'), 'Migrate billing')).toBeVisible();
      await expect(card(column(page, 'Cancelled'), 'Drop legacy export')).toBeVisible();
      await expect(card(column(page, 'Completed'), 'Ship onboarding')).toBeVisible();
      await expect(card(column(page, 'Blocked'), 'Wait on API keys')).toBeVisible();
    });

    test('a card shows its project, priority, pull request and age', async ({
      context,
      page,
    }) => {
      await serveTasks(context, page);

      const login = card(page, 'Implement login page');
      await expect(login.locator('.task-card-project')).toHaveText('Frontend App');
      await expect(login.getByRole('img', { name: 'Priority 1', exact: true })).toBeVisible();
      await expect(login.getByText('Agentic', { exact: true })).toBeVisible();
      await expect(login.locator('time.task-card-time')).toHaveText(UPDATED_RELATIVE);
      await expect(login.locator('time.task-card-time')).toHaveAttribute('datetime', UPDATED);

      const review = card(column(page, 'In review'), 'Add API endpoint');
      await expect(review.getByText('PR open', { exact: true })).toBeVisible();
      const link = review.getByRole('link', { name: 'View PR' });
      await expect(link).toHaveAttribute('href', PULL_REQUEST);
      await expect(link).toHaveAttribute('target', '_blank');
      await expect(link).toHaveAttribute('rel', 'noopener noreferrer');

      const unprioritised = card(page, 'Wait on API keys');
      await expect(unprioritised.getByRole('img', { name: /^Priority/ })).toHaveCount(0);
    });

    test('filters by project only, and shows empty columns as such', async ({
      context,
      page,
    }) => {
      const server = await serveTasks(context, page);

      const filters = page.locator('.tasks-filters').getByRole('combobox');
      await expect(filters).toHaveCount(1);
      await expect(page.getByRole('combobox', { name: 'Filter by status' })).toHaveCount(0);

      await chooseSelect(page.getByRole('combobox', { name: 'Filter by project' }), 'Frontend App');

      await expect(page.locator('li.task-card')).toHaveCount(4);
      await expect(page.locator('.board-column-count')).toHaveText([
        '2',
        '1',
        '0',
        '0',
        '0',
        '1',
        '0',
      ]);
      for (const label of ['Blocked', 'Paused', 'In review', 'Cancelled']) {
        await expect(column(page, label).locator('p.board-column-empty')).toHaveText('No tasks');
      }
      await expect(column(page, 'Planned').locator('p.board-column-empty')).toHaveCount(0);

      expect(server.listed.some((url) => url.searchParams.get('project_id') === 'proj-1')).toBe(
        true
      );
      expect(server.listed.every((url) => !url.searchParams.has('status'))).toBe(true);
    });

    test('dragging a card by its grip moves it to another column', async ({ context, page }) => {
      const server = await serveTasks(context, page);

      await pointerDrag(page, 'Fix navigation bug', column(page, 'Blocked'));

      await expect.poll(() => moves(server)).toEqual([
        { method: 'PUT', id: 'task-3', body: { status: 'blocked' } },
      ]);
      await expect(card(column(page, 'Blocked'), 'Fix navigation bug')).toBeVisible();
      await expect(column(page, 'In progress').locator('p.board-column-empty')).toHaveText(
        'No tasks'
      );
      await expect(column(page, 'Blocked').locator('.board-column-count')).toHaveText('2');
    });

    test('dropping a card back on its own column sends nothing', async ({ context, page }) => {
      const server = await serveTasks(context, page);

      await pointerDrag(page, 'Fix navigation bug', column(page, 'In progress'));

      await expect(card(column(page, 'In progress'), 'Fix navigation bug')).toBeVisible();
      expect(moves(server)).toEqual([]);
    });

    test('a card moves with the keyboard from its grip', async ({ context, page }) => {
      const server = await serveTasks(context, page);

      const grip = await pickUp(page, 'Implement login page', 'Planned');
      await pressAndAnnounce(page, 'ArrowRight', 'Implement login page is over In progress.');
      await pressAndAnnounce(page, 'Space', 'Implement login page moved to In progress.');

      await expect.poll(() => moves(server)).toEqual([
        { method: 'PUT', id: 'task-1', body: { status: 'in_progress' } },
      ]);
      await expect(card(column(page, 'In progress'), 'Implement login page')).toBeVisible();
      await expect(grip).toBeFocused();
    });

    test('quick arrow presses each carry a card one column further', async ({ context, page }) => {
      const server = await serveTasks(context, page);

      await pickUp(page, 'Implement login page', 'Planned');
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowRight');
      await expect(announcer(page)).toHaveText('Implement login page is over Paused.');
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowRight');
      await expect(announcer(page)).toHaveText('Implement login page is over Cancelled.');
      await page.keyboard.press('ArrowLeft');
      await page.keyboard.press('ArrowLeft');
      await expect(announcer(page)).toHaveText('Implement login page is over In review.');
      await pressAndAnnounce(page, 'Space', 'Implement login page moved to In review.');

      await expect.poll(() => moves(server)).toEqual([
        { method: 'PUT', id: 'task-1', body: { status: 'review' } },
      ]);
      await expect(card(column(page, 'In review'), 'Implement login page')).toBeInViewport();
    });

    test('a refused keyboard move keeps focus on the card it puts back', async ({
      context,
      page,
    }) => {
      const server = await serveTasks(context, page);
      let release: () => void = () => {};
      server.refuse = true;
      server.hold = new Promise<void>((resolve) => {
        release = resolve;
      });

      const grip = await pickUp(page, 'Implement login page', 'Planned');
      await pressAndAnnounce(page, 'ArrowRight', 'Implement login page is over In progress.');
      await pressAndAnnounce(page, 'Space', 'Implement login page moved to In progress.');
      await expect(card(column(page, 'In progress'), 'Implement login page')).toBeVisible();
      await expect(grip).toBeFocused();
      release();

      await expect(page.getByText("Couldn't move Implement login page")).toBeVisible();
      await expect(card(column(page, 'Planned'), 'Implement login page')).toBeVisible();
      await expect(grip).toBeFocused();
    });

    test('the up and down arrows scroll nothing while a card is carried', async ({
      context,
      page,
    }) => {
      await serveTasks(context, page);
      await page.setViewportSize({ width: 1280, height: 360 });
      const grip = page.getByRole('button', { name: 'Move Implement login page', exact: true });
      const offsets = () =>
        grip.evaluate((handle) => {
          const scrolled: number[] = [document.scrollingElement?.scrollTop ?? 0];
          for (let node = handle.parentElement; node; node = node.parentElement) {
            scrolled.push(node.scrollTop);
          }
          return scrolled;
        });
      await page.evaluate(() => {
        const claimed: boolean[] = [];
        Object.assign(window, { claimed });
        window.addEventListener('keydown', (event) => {
          if (event.code === 'ArrowUp' || event.code === 'ArrowDown') {
            claimed.push(event.defaultPrevented);
          }
        });
      });

      const before = await offsets();
      await pickUp(page, 'Implement login page', 'Planned');
      await page.keyboard.press('ArrowDown');
      await page.keyboard.press('ArrowDown');
      await page.keyboard.press('ArrowUp');
      await page.keyboard.press('Escape');
      await expect(page.locator('.task-card-overlay')).toHaveCount(0);

      expect(
        await page.evaluate(() => (window as unknown as { claimed: boolean[] }).claimed)
      ).toEqual([true, true, true]);
      expect(await offsets()).toEqual(before);
    });

    test('escape puts a keyboard-carried card back without a request', async ({
      context,
      page,
    }) => {
      const server = await serveTasks(context, page);

      await pickUp(page, 'Implement login page', 'Planned');
      await pressAndAnnounce(page, 'ArrowRight', 'Implement login page is over In progress.');
      await pressAndAnnounce(
        page,
        'Escape',
        'Moving Implement login page cancelled. It stays in Planned.'
      );
      await expect(page.locator('.task-card-overlay')).toHaveCount(0);
      await expect(card(column(page, 'Planned'), 'Implement login page')).toBeVisible();
      expect(moves(server)).toEqual([]);
    });

    test('a refused move puts the card back and says why', async ({ context, page }) => {
      const server = await serveTasks(context, page);
      let release: () => void = () => {};
      server.refuse = true;
      server.hold = new Promise<void>((resolve) => {
        release = resolve;
      });

      await pointerDrag(page, 'Fix navigation bug', column(page, 'Blocked'));

      await expect(card(column(page, 'Blocked'), 'Fix navigation bug')).toBeVisible();
      release();

      await expect(page.getByText("Couldn't move Fix navigation bug")).toBeVisible();
      await expect(page.getByText(REFUSED)).toBeVisible();
      await expect(card(column(page, 'In progress'), 'Fix navigation bug')).toBeVisible();
      await expect(card(column(page, 'Blocked'), 'Fix navigation bug')).toHaveCount(0);
      expect(moves(server)).toEqual([
        { method: 'PUT', id: 'task-3', body: { status: 'blocked' } },
      ]);
    });

    test('clicking a card opens its editor', async ({ context, page }) => {
      await serveTasks(context, page);

      const login = card(page, 'Implement login page');
      const project = await center(login.locator('.task-card-project'));
      await page.mouse.click(project.x, project.y);

      await expect(page).toHaveURL('/tasks?task=task-1');
      await expect(page.locator('.task-details')).toBeVisible();
      await expect(page.getByRole('region', { name: 'Tasks', exact: true })).toHaveCount(0);
    });

    test('the title button opens the editor', async ({ context, page }) => {
      await serveTasks(context, page);

      await page.getByRole('button', { name: 'Ship onboarding', exact: true }).click();

      await expect(page).toHaveURL('/tasks?task=task-7');
      await expect(page.locator('.task-details')).toBeVisible();
    });

    test('the delete dialog cancels without deleting', async ({ context, page }) => {
      const server = await serveTasks(context, page);

      await chooseFromMenu(page, 'Drop legacy export', 'Delete');
      const dialog = page.getByRole('dialog', { name: 'Delete "Drop legacy export"?' });
      await expect(dialog).toBeVisible();
      await dialog.getByRole('button', { name: 'Cancel' }).click();

      await expect(dialog).toHaveCount(0);
      await expect(card(column(page, 'Cancelled'), 'Drop legacy export')).toBeVisible();
      expect(server.mutations).toEqual([]);
      await expect(page).toHaveURL('/tasks');
    });

    test('the delete dialog deletes the task on confirm', async ({ context, page }) => {
      const server = await serveTasks(context, page);

      await chooseFromMenu(page, 'Drop legacy export', 'Delete');
      const dialog = page.getByRole('dialog', { name: 'Delete "Drop legacy export"?' });
      await dialog.getByRole('button', { name: 'Delete task' }).click();

      await expect(dialog).toHaveCount(0);
      await expect(card(page, 'Drop legacy export')).toHaveCount(0);
      await expect(column(page, 'Cancelled').locator('p.board-column-empty')).toHaveText(
        'No tasks'
      );
      expect(server.mutations).toEqual([{ method: 'DELETE', id: 'task-8', body: null }]);
      await expect(page).toHaveURL('/tasks');
      await expect(column(page, 'Cancelled')).toBeFocused();
    });

    test.describe('on a touch screen', () => {
      test.use({ hasTouch: true });

      test('a grip can be hit anywhere in a 44px square around it', async ({ context, page }) => {
        await serveTasks(context, page);
        await page.setViewportSize(PHONE);
        const grip = page.getByRole('button', { name: 'Move Implement login page', exact: true });
        await expect(grip).toBeVisible();

        const target = await grip.evaluate((handle) => {
          const area = getComputedStyle(handle, '::after');
          const box = handle.getBoundingClientRect();
          const left = box.left + Number.parseFloat(area.left);
          const top = box.top + Number.parseFloat(area.top);
          const width = Number.parseFloat(area.width);
          const height = Number.parseFloat(area.height);
          const corners = [
            [left + 1, top + 1],
            [left + width - 1, top + 1],
            [left + 1, top + height - 1],
            [left + width - 1, top + height - 1],
          ];
          return {
            touch: matchMedia('(hover: none)').matches,
            grip: [box.width, box.height],
            width,
            height,
            hits: corners.map(
              ([x, y]) => document.elementFromPoint(x, y)?.closest('.task-card-handle') === handle
            ),
          };
        });

        expect(target).toEqual({
          touch: true,
          grip: [12, 20],
          width: 44,
          height: 44,
          hits: [true, true, true, true],
        });
      });

      test('swiping across a card scrolls the board without picking the card up', async ({
        context,
        page,
      }) => {
        const server = await serveTasks(context, page);
        await shrinkToPhone(page);
        const board = page.getByRole('region', { name: 'Tasks', exact: true });
        const announced = await recordAnnouncements(page);
        const from = await center(card(page, 'Implement login page').locator('.task-card-project'));
        const touch = await finger(page);

        await touch.down(from);
        await touch.slide(from, { x: from.x - 160, y: from.y });
        await expect.poll(() => board.evaluate((element) => element.scrollLeft)).toBeGreaterThan(0);
        await touch.up();

        expect(await announced()).toEqual([]);
        await expect(page.locator('.task-card-overlay')).toHaveCount(0);
        await expect(page).toHaveURL('/tasks');
        expect(moves(server)).toEqual([]);
      });

      test('a card dragged by its grip to the edge stops two columns over', async ({
        context,
        page,
      }) => {
        const server = await serveTasks(context, page);
        await shrinkToPhone(page);
        const bounds = await page.getByRole('region', { name: 'Tasks', exact: true }).boundingBox();
        if (!bounds) throw new Error('The board has no layout box');
        const from = await center(
          page.getByRole('button', { name: 'Move Implement login page', exact: true })
        );
        const edge = { x: bounds.x + bounds.width - 12, y: from.y };
        const rest = { x: bounds.x + bounds.width * 0.75, y: from.y };
        const touch = await finger(page);

        await touch.down(from);
        await touch.slide(from, edge);
        await column(page, 'Blocked').evaluate(
          (section, reach) =>
            new Promise<void>((resolve) => {
              const watch = () =>
                section.getBoundingClientRect().left < reach
                  ? resolve()
                  : requestAnimationFrame(watch);
              watch();
            }),
          rest.x - 20
        );
        await touch.move(rest);
        await expect(announcer(page)).toHaveText('Implement login page is over Blocked.');
        await touch.up();

        await expect.poll(() => moves(server)).toEqual([
          { method: 'PUT', id: 'task-1', body: { status: 'blocked' } },
        ]);
        await expect(card(column(page, 'Blocked'), 'Implement login page')).toBeInViewport();
      });
    });

    test.describe('on a phone', () => {
      test('only the board scrolls sideways, one column at a time', async ({ context, page }) => {
        await serveTasks(context, page);
        await page.setViewportSize(PHONE);
        const board = page.getByRole('region', { name: 'Tasks', exact: true });

        const scroll = await board.evaluate((element) => ({
          scroll: element.scrollWidth,
          client: element.clientWidth,
          snap: getComputedStyle(element).scrollSnapType,
        }));
        expect(scroll.scroll).toBeGreaterThan(scroll.client);
        expect(scroll.snap).toBe('x mandatory');
        await expect
          .poll(() =>
            page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)
          )
          .toBe(true);

        await expect(column(page, 'Planned')).toBeInViewport();
        await expect(column(page, 'Cancelled')).not.toBeInViewport();
        await board.evaluate((element) => element.scrollTo({ left: element.scrollWidth }));
        await expect(column(page, 'Cancelled')).toBeInViewport();
        await expect
          .poll(() =>
            page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)
          )
          .toBe(true);
      });
    });
  });

  test.describe('Create Task Modal', () => {
    test('opens create modal from header button', async ({ page }) => {
      const newTaskButton = page.getByRole('button', { name: /New task/ });
      await expect(newTaskButton).toBeEnabled({ timeout: 10000 });
      await newTaskButton.click();
      await expect(page.getByRole('dialog', { name: 'New Task' })).toBeVisible();
    });

    test('shows project selection options', async ({ page }) => {
      const newTaskButton = page.getByRole('button', { name: /New task/ });
      await expect(newTaskButton).toBeEnabled({ timeout: 10000 });
      await newTaskButton.click();

      await expect(page.locator('.project-selection-option')).toHaveCount(2);
      await expect(page.locator('.project-selection-option').first()).toContainText('Frontend App');
    });

    test('shows all form fields', async ({ page }) => {
      const newTaskButton = page.getByRole('button', { name: /New task/ });
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
      const newTaskButton = page.getByRole('button', { name: /New task/ });
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

      await context.unroute(tasksRoutePattern);
      await page.unroute(tasksRoutePattern);
      await routeApiContext(context, tasksRoutePattern, (route) => {
        if (route.request().method() === 'POST') {
          return fulfillJson(route, { task: newTask }, 201);
        }
        if (route.request().method() === 'GET') {
          return fulfillJson(route, { tasks: [] });
        }
        return route.continue();
      });

      const newTaskButton = page.getByRole('button', { name: /New task/ });
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
      let finishCreation: () => void = () => {};
      const taskCanCreate = new Promise<void>((resolve) => {
        finishCreation = resolve;
      });

      await routeApi(page, tasksRoutePattern, async (route) => {
        if (route.request().method() === 'POST') {
          await taskCanCreate;
          const task = generateMockTask('new-task', 'proj-1', 'New Task');
          await fulfillJson(route, { task }, 201);
        } else {
          await route.continue();
        }
      });

      const newTaskButton = page.getByRole('button', { name: /New task/ });
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
      finishCreation();
      await expect(dialog).toHaveCount(0);
    });

    test('shows error when creation fails', async ({ page }) => {
      await routeApi(page, tasksRoutePattern, (route) => {
        if (route.request().method() === 'POST') {
          return fulfillJson(route, { error: 'Validation failed' }, 400);
        }
        return route.continue();
      });

      const newTaskButton = page.getByRole('button', { name: /New task/ });
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
      const newTaskButton = page.getByRole('button', { name: /New task/ });
      await expect(newTaskButton).toBeEnabled({ timeout: 10000 });
      await newTaskButton.click();
      await page.getByRole('button', { name: 'Cancel' }).click();

      await expect(page.getByRole('dialog', { name: 'New Task' })).toHaveCount(0);
    });
  });

  test.describe('Task Execution', () => {
    test.beforeEach(async ({ page }) => {
      await routeApi(page, tasksRoutePattern, (route) => fulfillJson(route, { tasks: mockTasks }));
      await routeApi(page, '**/api/tasks/*/runs', (route) => fulfillJson(route, { runs: [] }));
      await page.reload();
    });

    const run = {
      id: 'run-123',
      task_id: 'task-1',
      status: 'running',
      current_phase: 'thinking',
      progress_percent: null,
      error_message: null,
    };

    const runLogin = (page: Page) => chooseFromMenu(page, 'Implement login page', 'Run');

    test('opens an accessible idle dialog and closes with Escape', async ({ page }) => {
      await runLogin(page);
      const dialog = page.getByRole('dialog', { name: 'Implement login page' });
      await expect(dialog).toBeVisible();
      await expect(dialog.getByRole('button', { name: 'Start Execution' })).toBeEnabled();
      await expect(dialog.locator('.execution-logs')).toHaveCount(0);
      await page.keyboard.press('Escape');
      await expect(dialog).not.toBeVisible();
    });

    test('starts a real run and restores it on reopen', async ({ page }, testInfo) => {
      let started = false;
      await routeApi(page, '**/api/tasks/task-1/runs', (route) => {
        if (route.request().method() === 'GET') {
          return fulfillJson(route, { runs: started ? [run] : [] });
        }
        started = true;
        return fulfillJson(route, { run: { ...run, status: 'pending' } });
      });
      await routeApi(page, '**/api/tasks/runs/run-123', (route) => fulfillJson(route, { run }));
      await routeApi(page, '**/api/tasks/runs/run-123/logs', (route) =>
        fulfillJson(route, {
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
        })
      );
      await runLogin(page);
      await page.getByRole('button', { name: 'Start Execution' }).click();
      await expect(page.getByText('Reading project files')).toBeVisible();
      await expect(page.getByText('Running', { exact: true })).toBeVisible();
      await page.screenshot({ path: testInfo.outputPath('task-running.png'), fullPage: true });
      await expect(page.getByRole('button', { name: 'Stop Execution' })).toHaveCount(0);
      await page.getByRole('button', { name: 'Close', exact: true }).click();
      await runLogin(page);
      await expect(page.getByText('Reading project files')).toBeVisible();
    });

    for (const theme of ['dark', 'light']) {
      test(`startup failure remains readable in ${theme}`, async ({ page }, testInfo) => {
        await page.evaluate((value) => {
          document.documentElement.setAttribute('data-theme', value);
          document.documentElement.classList.toggle('dark', value === 'dark');
        }, theme);
        await routeApi(page, '**/api/tasks/task-1/runs', (route) =>
          route.request().method() === 'GET'
            ? fulfillJson(route, { runs: [] })
            : fulfillJson(
                route,
                { error: 'Worker unavailable. Try again when a worker is connected.' },
                503
              )
        );
        await runLogin(page);
        await page.getByRole('button', { name: 'Start Execution' }).click();
        await expect(page.getByText('Could not start task')).toBeVisible();
        await expect(
          page.getByText('Worker unavailable. Try again when a worker is connected.')
        ).toBeVisible();
        await expect(page.locator('.execution-logs')).toHaveCount(0);
        await expect(page.locator('.task-progress-bar-container')).toHaveCount(0);
        await page.screenshot({
          path: testInfo.outputPath(`task-error-${theme}.png`),
          fullPage: true,
        });
        await page.setViewportSize({ width: 390, height: 844 });
        await expect
          .poll(() => page.evaluate(() => document.documentElement.scrollWidth))
          .toBe(390);
        await expect(page.getByRole('button', { name: 'Try Again' })).toBeInViewport();
        await expect
          .poll(() =>
            page.getByRole('dialog').evaluate((dialog) => {
              const bounds = dialog.getBoundingClientRect();
              return dialog.contains(document.elementFromPoint(bounds.left + 20, bounds.top + 40));
            })
          )
          .toBe(true);
        await page.screenshot({
          path: testInfo.outputPath(`task-error-${theme}-mobile.png`),
          fullPage: false,
        });
        await page.keyboard.press('Escape');
        await expect(page.getByRole('dialog')).toHaveCount(0);
        await expect
          .poll(() => page.evaluate(() => document.documentElement.scrollWidth))
          .toBe(390);
      });
    }
  });

  test.describe('Error Handling', () => {
    test('shows error when loading tasks fails', async ({ context, page }) => {
      await context.unroute(tasksRoutePattern);
      await page.unroute(tasksRoutePattern);
      await routeApiContext(context, tasksRoutePattern, (route) =>
        fulfillJson(route, { error: 'Server error' }, 500)
      );

      await page.goto('/');
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.goto('/tasks');
      await expect(page).toHaveURL('/tasks');

      await expect(page.getByText('Server error')).toBeVisible({ timeout: 15000 });
    });
  });

  test.describe('Loading State', () => {
    test('shows skeleton cards while loading', async ({ context, page }) => {
      let finishLoading: () => void = () => {};
      const tasksCanLoad = new Promise<void>((resolve) => {
        finishLoading = resolve;
      });
      await context.unroute(tasksRoutePattern);
      await page.unroute(tasksRoutePattern);
      await routeApiContext(context, tasksRoutePattern, async (route) => {
        await tasksCanLoad;
        await fulfillJson(route, { tasks: mockTasks });
      });

      await page.goto('/');
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.goto('/tasks');

      await expect(page.locator('.skeleton-card').first()).toBeVisible();
      finishLoading();
      await expect(page.locator('li.task-card')).toHaveCount(mockTasks.length);
      await expect(page.locator('.skeleton-card')).toHaveCount(0);
    });
  });

  test.describe('Disabled State', () => {
    test('new task button is disabled when no projects exist', async ({ context, page }) => {
      await context.unroute('**/api/projects*');
      await page.unroute('**/api/projects*');
      await routeApiContext(context, '**/api/projects*', (route) =>
        fulfillJson(route, { success: true, projects: [] })
      );

      await page.goto('/');
      await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
      await page.goto('/tasks');
      await expect(page).toHaveURL('/tasks');

      await expect(page.getByRole('button', { name: /New task/ })).toBeDisabled();
    });
  });
});
