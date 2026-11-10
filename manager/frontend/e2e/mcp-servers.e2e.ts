import path from 'node:path';
import type { Locator, Page } from '@playwright/test';
import { expect, test } from './fixtures';
import { setupAuth } from './helpers/auth';
import { chooseSelect, expectSelectOptions } from './helpers/select';
import { blockServiceWorker, routeApi } from './test-utils';

type OrgRole = 'owner' | 'admin' | 'member';
type WorkspaceRole = 'owner' | 'admin' | 'member' | 'viewer';
type Status = 'connected' | 'failed' | 'disabled' | 'connecting';

interface SecretField {
  name: string;
  has_value: boolean;
}

interface McpServer {
  id: string | null;
  name: string;
  source: 'organization' | 'instance';
  transport: 'stdio' | 'http' | 'sse';
  command: string | null;
  arguments: string[];
  url: string | null;
  tools: string[] | null;
  enabled: boolean;
  environment: SecretField[];
  headers: SecretField[];
  status: Status;
  error: string | null;
  available_tools: string[];
  created_at: string | null;
  updated_at: string | null;
}

interface Scenario {
  role?: OrgRole;
  workspaceRole?: WorkspaceRole;
  commands?: boolean;
  servers?: McpServer[];
  overrides?: Record<string, boolean>;
}

interface Captured {
  method: string;
  path: string;
  body: unknown;
}

interface Mock {
  captured: Captured[];
  secrets: Map<string, string>;
}

type Body = Record<string, unknown>;

const organizationId = '00000000-0000-0000-0000-000000000001';
const workspaceId = '00000000-0000-0000-0000-000000000001';
const userId = 'user-1';
const serversPath = `/api/organizations/${organizationId}/mcp-servers`;
const workspacePath = `/api/organizations/${organizationId}/workspaces/${workspaceId}/mcp-servers`;
const organizationPage = '/settings/organization/mcp';
const workspacePage = '/settings/workspace/mcp';
const stamp = '2026-11-01T00:00:00Z';
const docsId = '11111111-1111-4111-8111-000000000001';
const githubId = '11111111-1111-4111-8111-000000000002';
const searchId = '11111111-1111-4111-8111-000000000003';
const notesId = '11111111-1111-4111-8111-000000000004';
const wikiId = '22222222-2222-4222-8222-000000000001';
const fixtureSecret = 'Bearer fixture-docs-secret';
const commandsOff = 'Command (stdio) servers are turned off on this instance.';
const readOnly = 'Only organization admins can change MCP servers.';
const workspaceReadOnly = 'Only workspace admins can change MCP servers.';
const directory = process.env.MCP_SCREENSHOTS;

function server(changes: Partial<McpServer> & Pick<McpServer, 'name'>): McpServer {
  return {
    id: null,
    source: 'organization',
    transport: 'http',
    command: null,
    arguments: [],
    url: null,
    tools: null,
    enabled: true,
    environment: [],
    headers: [],
    status: 'connected',
    error: null,
    available_tools: [],
    created_at: stamp,
    updated_at: stamp,
    ...changes,
  };
}

function fixtures(): McpServer[] {
  return [
    server({
      id: docsId,
      name: 'docs',
      url: 'https://docs.example/mcp',
      tools: ['search_pages', 'fetch_page'],
      headers: [{ name: 'Authorization', has_value: true }],
      available_tools: ['search_pages', 'fetch_page', 'list_spaces'],
    }),
    server({
      id: githubId,
      name: 'github',
      transport: 'stdio',
      command: 'npx',
      arguments: ['-y', '@modelcontextprotocol/server-github'],
      environment: [{ name: 'GITHUB_TOKEN', has_value: true }],
      status: 'failed',
      error: 'spawn npx ENOENT',
    }),
    server({
      id: searchId,
      name: 'search',
      url: 'https://search.example/mcp',
      status: 'connecting',
    }),
    server({
      id: notesId,
      name: 'notes',
      url: 'https://notes.example/mcp',
      enabled: false,
      status: 'disabled',
    }),
    server({
      name: 'magents',
      source: 'instance',
      transport: 'stdio',
      command: 'magents',
      arguments: ['mcp'],
      available_tools: ['send_message', 'list_sessions'],
      created_at: null,
      updated_at: null,
    }),
  ];
}

function secretMap(map: unknown): Record<string, string | null> {
  return map && typeof map === 'object' ? (map as Record<string, string | null>) : {};
}

function storeSecrets(
  secrets: Map<string, string>,
  id: string,
  kind: string,
  previous: SecretField[],
  map: Record<string, string | null>
): SecretField[] {
  return Object.entries(map).map(([name, value]) => {
    if (value !== null) {
      secrets.set(`${id}:${kind}:${name}`, value);
      return { name, has_value: true };
    }
    return { name, has_value: previous.find((field) => field.name === name)?.has_value ?? false };
  });
}

async function mockApi(page: Page, scenario: Scenario = {}): Promise<Mock> {
  const role = scenario.role ?? 'admin';
  const workspaceRole = scenario.workspaceRole ?? 'admin';
  const commands = scenario.commands ?? true;
  const servers = scenario.servers ?? fixtures();
  const overrides = new Map(Object.entries(scenario.overrides ?? {}));
  const captured: Captured[] = [];
  const secrets = new Map<string, string>([[`${docsId}:headers:Authorization`, fixtureSecret]]);
  const pending = new Map<string, number>();
  let created = 0;

  const connect = (target: McpServer) => {
    target.status = 'connecting';
    target.error = null;
    target.available_tools = [];
    if (target.id) pending.set(target.id, 2);
  };

  const advance = () => {
    for (const [id, remaining] of pending) {
      const target = servers.find((entry) => entry.id === id);
      if (!target) {
        pending.delete(id);
      } else if (remaining <= 1) {
        pending.delete(id);
        target.status = 'connected';
        target.available_tools = ['delegate'];
      } else {
        pending.set(id, remaining - 1);
      }
    }
  };

  const workspaceList = () => ({
    servers: servers.map((entry) => {
      const override = entry.id ? overrides.get(entry.id) : undefined;
      const workspaceEnabled = override === undefined ? null : override;
      return {
        ...entry,
        workspace_enabled: workspaceEnabled,
        effective: entry.enabled && (workspaceEnabled ?? true),
      };
    }),
  });

  await routeApi(page, /\/api\//, async (route) => {
    const request = route.request();
    const { pathname } = new URL(request.url());
    const method = request.method();
    const body: unknown = request.postData() ? request.postDataJSON() : null;
    captured.push({ method, path: pathname, body });

    if (pathname === '/api/organizations') {
      await route.fulfill({
        json: {
          organizations: [
            {
              id: organizationId,
              name: 'Acme Corp',
              slug: 'acme-corp',
              description: null,
              is_active: true,
              role,
              created_at: '2024-01-01T00:00:00Z',
              updated_at: '2024-01-01T00:00:00Z',
            },
          ],
        },
      });
      return;
    }
    if (pathname === `/api/organizations/${organizationId}/workspaces`) {
      await route.fulfill({
        json: {
          workspaces: [
            {
              id: workspaceId,
              organization_id: organizationId,
              name: 'Engineering',
              slug: 'engineering',
              description: null,
              is_active: true,
              created_at: '2024-01-01T00:00:00Z',
              updated_at: '2024-01-01T00:00:00Z',
            },
          ],
        },
      });
      return;
    }
    if (pathname === `/api/workspaces/${workspaceId}/members`) {
      await route.fulfill({
        json: {
          members: [
            {
              id: '00000000-0000-0000-0000-0000000000a1',
              user_id: userId,
              workspace_id: workspaceId,
              role: workspaceRole,
              email: 'user@example.com',
              display_name: null,
              joined_at: '2024-01-01T00:00:00Z',
            },
          ],
        },
      });
      return;
    }
    if (pathname === serversPath && method === 'GET') {
      advance();
      await route.fulfill({ json: { commands, servers } });
      return;
    }
    if (pathname === serversPath && method === 'POST') {
      const posted = body as Body;
      created += 1;
      const id = `22222222-2222-4222-8222-${String(created).padStart(12, '0')}`;
      const entry = server({
        id,
        name: String(posted.name),
        transport: posted.transport as McpServer['transport'],
        command: typeof posted.command === 'string' ? posted.command : null,
        arguments: Array.isArray(posted.arguments) ? (posted.arguments as string[]) : [],
        url: typeof posted.url === 'string' ? posted.url : null,
        environment: storeSecrets(secrets, id, 'environment', [], secretMap(posted.environment)),
        headers: storeSecrets(secrets, id, 'headers', [], secretMap(posted.headers)),
      });
      connect(entry);
      servers.push(entry);
      await route.fulfill({ status: 201, json: entry });
      return;
    }
    if (pathname.startsWith(`${serversPath}/`)) {
      const [id, action] = pathname.slice(serversPath.length + 1).split('/');
      const index = servers.findIndex((entry) => entry.id === id);
      if (index === -1) {
        await route.fulfill({ status: 404, json: { error: 'MCP server not found' } });
        return;
      }
      const target = servers[index];
      if (action === 'connection' && method === 'POST') {
        connect(target);
        await route.fulfill({ status: 202, json: target });
        return;
      }
      if (method === 'DELETE') {
        servers.splice(index, 1);
        await route.fulfill({ status: 204 });
        return;
      }
      if (method === 'PUT') {
        const changes = body as Body;
        let reconnect = false;
        for (const key of ['name', 'command', 'arguments', 'url'] as const) {
          if (key in changes) {
            Object.assign(target, { [key]: changes[key] });
            reconnect = true;
          }
        }
        if ('tools' in changes) target.tools = changes.tools as string[] | null;
        for (const kind of ['environment', 'headers'] as const) {
          if (kind in changes) {
            target[kind] = storeSecrets(secrets, id, kind, target[kind], secretMap(changes[kind]));
            reconnect = true;
          }
        }
        if ('enabled' in changes) {
          target.enabled = Boolean(changes.enabled);
          reconnect = true;
        }
        if (!target.enabled) {
          target.status = 'disabled';
          target.error = null;
          target.available_tools = [];
          pending.delete(id);
        } else if (reconnect) {
          connect(target);
        }
        await route.fulfill({ json: target });
        return;
      }
    }
    if (pathname === workspacePath) {
      if (method === 'PUT') {
        const changes = (body as { servers: Record<string, boolean | null> }).servers;
        for (const [id, value] of Object.entries(changes)) {
          if (value === null) overrides.delete(id);
          else overrides.set(id, value);
        }
      }
      await route.fulfill({ json: workspaceList() });
      return;
    }
    if (pathname === '/api/models') {
      await route.fulfill({ json: { models: [] } });
      return;
    }
    await route.fallback();
  });
  return { captured, secrets };
}

function writes(mock: Mock): Captured[] {
  return mock.captured.filter(
    (entry) => entry.method !== 'GET' && entry.path.includes('/mcp-servers')
  );
}

async function open(page: Page, target: string, scenario: Scenario): Promise<Mock> {
  const mock = await mockApi(page, scenario);
  await setupAuth(page);
  await page.goto(target);
  await expect(page.getByRole('heading', { name: 'MCP servers', level: 2 })).toBeVisible({
    timeout: 45000,
  });
  return mock;
}

async function openOrganization(page: Page, scenario: Scenario = {}): Promise<Mock> {
  const mock = await open(page, organizationPage, scenario);
  await expect(page.getByRole('list', { name: 'MCP servers' })).toBeVisible();
  return mock;
}

async function openWorkspace(page: Page, scenario: Scenario = {}): Promise<Mock> {
  const mock = await open(page, workspacePage, scenario);
  await expect(page.getByRole('group', { name: 'docs', exact: true })).toBeVisible();
  return mock;
}

function row(page: Page, name: string): Locator {
  return page
    .getByRole('list', { name: 'MCP servers' })
    .getByRole('listitem', { name, exact: true });
}

async function chooseAction(page: Page, name: string, item: string): Promise<void> {
  await page.getByRole('button', { name: `Actions for ${name}`, exact: true }).click();
  await page.getByRole('menuitem', { name: item, exact: true }).click();
}

async function fillHttpServer(page: Page): Promise<Locator> {
  await page.getByRole('button', { name: 'Add server', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Add MCP server' });
  await expect(dialog).toBeVisible();
  await dialog.getByLabel('Name', { exact: true }).fill('wiki');
  await dialog.getByLabel('URL', { exact: true }).fill('https://wiki.example/mcp');
  await dialog.getByRole('button', { name: 'Add header', exact: true }).click();
  await dialog.getByLabel('Header name 1', { exact: true }).fill('Authorization');
  await dialog.getByLabel('Value for Authorization', { exact: true }).fill('Bearer t');
  return dialog;
}

async function addHttpServer(page: Page): Promise<void> {
  const dialog = await fillHttpServer(page);
  await dialog.getByRole('button', { name: 'Add server', exact: true }).click();
  await expect(dialog).toHaveCount(0);
}

async function expectNoSidewaysScroll(page: Page): Promise<void> {
  const overflow = await page.evaluate(() => {
    const root = document.documentElement;
    return root.scrollWidth - root.clientWidth;
  });
  expect(overflow).toBeLessThanOrEqual(0);
}

async function expectDialogContained(page: Page): Promise<void> {
  const layout = await page.evaluate(() => {
    const dialog = document.querySelector('[role="dialog"]');
    if (!dialog) return null;
    const box = dialog.getBoundingClientRect();
    const root = document.documentElement;
    const escaped = Array.from(dialog.querySelectorAll('*'))
      .filter((element) => {
        const rect = element.getBoundingClientRect();
        return rect.width > 0 && (rect.left < box.left - 0.5 || rect.right > box.right + 0.5);
      })
      .map((element) => `${element.tagName.toLowerCase()}.${element.className}`);
    return {
      escaped,
      inside: box.left >= 0 && box.right <= root.clientWidth,
      scrollsSideways:
        root.scrollWidth > root.clientWidth || dialog.scrollWidth > dialog.clientWidth,
    };
  });
  expect(layout).toEqual({ escaped: [], inside: true, scrollsSideways: false });
}

test.use({ viewport: { width: 1280, height: 900 }, timezoneId: 'UTC', locale: 'en-US' });

test.describe('MCP servers', () => {
  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  test('the list shows every status, the failure and a read-only instance row', async ({
    page,
  }) => {
    await openOrganization(page);

    await expect(row(page, 'docs').getByRole('status')).toHaveText('Connected');
    await expect(row(page, 'docs')).toContainText('2 of 3 tools');
    await expect(row(page, 'github').getByRole('status')).toHaveText('Failed');
    await expect(row(page, 'github')).toContainText('spawn npx ENOENT');
    await expect(row(page, 'search').getByRole('status')).toHaveText('Connecting');
    await expect(row(page, 'search').locator('.spinner')).toBeVisible();
    await expect(row(page, 'notes').getByRole('status')).toHaveText('Disabled');
    await expect(page.getByRole('switch', { name: 'Enable notes' })).not.toBeChecked();
    await expect(page.getByRole('switch', { name: 'Enable docs' })).toBeChecked();

    const instance = row(page, 'magents');
    await expect(instance.getByRole('status')).toHaveText('Connected');
    await expect(instance).toContainText('Instance');
    await expect(instance.getByRole('switch')).toHaveCount(0);
    await expect(instance.getByRole('button', { name: 'Actions for magents' })).toHaveCount(0);
    await expect(page.getByText(commandsOff)).toHaveCount(0);
    await expect(page.getByText(readOnly)).toHaveCount(0);
  });

  test('adding an http server posts exactly the typed fields', async ({ page }) => {
    const mock = await openOrganization(page);

    await addHttpServer(page);

    expect(writes(mock)).toEqual([
      {
        method: 'POST',
        path: serversPath,
        body: {
          name: 'wiki',
          transport: 'http',
          url: 'https://wiki.example/mcp',
          headers: { Authorization: 'Bearer t' },
        },
      },
    ]);
    await expect(row(page, 'wiki')).toBeVisible();
    expect(mock.secrets.get(`${wikiId}:headers:Authorization`)).toBe('Bearer t');
    expect(await page.content()).not.toContain('Bearer t');
  });

  test('a new server goes from connecting to connected without a reload', async ({ page }) => {
    const mock = await openOrganization(page);
    let navigations = 0;
    page.on('framenavigated', (frame) => {
      if (frame === page.mainFrame()) navigations += 1;
    });

    await addHttpServer(page);

    const status = row(page, 'wiki').getByRole('status');
    await expect(status).toHaveText('Connecting');
    await expect(row(page, 'wiki').locator('.spinner')).toBeVisible();
    await expect(status).toHaveText('Connected', { timeout: 15000 });
    await expect(row(page, 'wiki')).toContainText('1 tool');
    await expect(row(page, 'search').getByRole('status')).toHaveText('Connecting');
    expect(navigations).toBe(0);
    const reads = mock.captured.filter(
      (entry) => entry.method === 'GET' && entry.path === serversPath
    );
    expect(reads.length).toBeGreaterThanOrEqual(3);
  });

  const toolCases = [
    { tool: 'list_spaces', checked: true, tools: null },
    { tool: 'search_pages', checked: false, tools: ['fetch_page'] },
  ];

  for (const { tool, checked, tools } of toolCases) {
    test(`${checked ? 'checking' : 'unchecking'} ${tool} puts tools ${JSON.stringify(tools)}`, async ({
      page,
    }) => {
      const mock = await openOrganization(page);
      const docs = row(page, 'docs');
      const expand = docs.getByRole('button', { name: 'Tools for docs', exact: true });
      await expect(expand).toHaveAttribute('aria-expanded', 'false');
      await expand.click();
      await expect(expand).toHaveAttribute('aria-expanded', 'true');

      const box = docs.getByRole('checkbox', { name: tool, exact: true });
      await expect(box).toBeChecked({ checked: !checked });
      await box.click();
      await expect(box).toBeChecked({ checked });

      expect(writes(mock)).toEqual([
        { method: 'PUT', path: `${serversPath}/${docsId}`, body: { tools } },
      ]);
    });
  }

  test('switching a server off puts enabled false', async ({ page }) => {
    const mock = await openOrganization(page);

    const toggle = page.getByRole('switch', { name: 'Enable docs' });
    await row(page, 'docs').locator('label.mcp-server-toggle').click();
    await expect(toggle).not.toBeChecked();

    await expect(row(page, 'docs').getByRole('status')).toHaveText('Disabled');
    expect(writes(mock)).toEqual([
      { method: 'PUT', path: `${serversPath}/${docsId}`, body: { enabled: false } },
    ]);
  });

  test('removing a server confirms first, then deletes it', async ({ page }) => {
    const mock = await openOrganization(page);

    await chooseAction(page, 'notes', 'Remove');
    const dialog = page.getByRole('dialog', { name: 'Remove notes?' });
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(dialog).toHaveCount(0);
    expect(writes(mock)).toEqual([]);

    await chooseAction(page, 'notes', 'Remove');
    await dialog.getByRole('button', { name: 'Remove server', exact: true }).click();

    await expect(dialog).toHaveCount(0);
    await expect(row(page, 'notes')).toHaveCount(0);
    await expect(row(page, 'docs')).toBeVisible();
    expect(writes(mock)).toEqual([
      { method: 'DELETE', path: `${serversPath}/${notesId}`, body: null },
    ]);
  });

  test('importing posts each server in order and skips sse', async ({ page }) => {
    const mock = await openOrganization(page);

    await page.getByRole('button', { name: 'Import JSON', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Import MCP servers' });
    await dialog.getByLabel('mcpServers JSON').fill(
      JSON.stringify({
        mcpServers: {
          files: { command: 'npx', args: ['-y', 'server-files'], env: { ROOT: '/srv' } },
          wiki: { url: 'https://wiki.example/mcp' },
          legacy: { type: 'sse', url: 'https://legacy.example/sse' },
        },
      })
    );

    await expect(
      dialog.getByRole('list', { name: 'Servers to add' }).getByRole('listitem')
    ).toHaveText(['files', 'wiki']);
    await expect(dialog.getByRole('list', { name: 'Skipped' }).getByRole('listitem')).toHaveText([
      'legacy: SSE servers are not supported; use streamable HTTP (http)',
    ]);
    await dialog.getByRole('button', { name: 'Import 2 servers', exact: true }).click();
    await expect(dialog).toHaveCount(0);

    expect(writes(mock)).toEqual([
      {
        method: 'POST',
        path: serversPath,
        body: {
          name: 'files',
          transport: 'stdio',
          command: 'npx',
          arguments: ['-y', 'server-files'],
          environment: { ROOT: '/srv' },
        },
      },
      {
        method: 'POST',
        path: serversPath,
        body: { name: 'wiki', transport: 'http', url: 'https://wiki.example/mcp' },
      },
    ]);
    await expect(page.getByText('Imported 2 servers.')).toBeVisible();
    await expect(row(page, 'files')).toBeVisible();
    await expect(row(page, 'wiki')).toBeVisible();
    await expect(row(page, 'legacy')).toHaveCount(0);
  });

  test('with commands off only streamable http is offered', async ({ page }) => {
    await openOrganization(page, { commands: false });

    await expect(page.getByText(commandsOff)).toBeVisible();
    await page.getByRole('button', { name: 'Add server', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Add MCP server' });
    await expectSelectOptions(dialog.getByRole('combobox', { name: 'Transport' }), [
      'Streamable HTTP',
    ]);
  });

  test('with commands on a stdio server posts its command, arguments and environment', async ({
    page,
  }) => {
    const mock = await openOrganization(page);

    await page.getByRole('button', { name: 'Add server', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Add MCP server' });
    const transport = dialog.getByRole('combobox', { name: 'Transport' });
    await expectSelectOptions(transport, ['Streamable HTTP', 'Command (stdio)']);
    await chooseSelect(transport, 'Command (stdio)');
    await dialog.getByLabel('Name', { exact: true }).fill('files');
    await dialog.getByLabel('Command', { exact: true }).fill('npx');
    await dialog.getByLabel('Arguments', { exact: true }).fill('-y\nserver-files');
    await dialog.getByRole('button', { name: 'Add variable', exact: true }).click();
    await dialog.getByLabel('Variable name 1', { exact: true }).fill('ROOT');
    await dialog.getByLabel('Value for ROOT', { exact: true }).fill('/srv');
    await dialog.getByRole('button', { name: 'Add server', exact: true }).click();
    await expect(dialog).toHaveCount(0);

    expect(writes(mock)).toEqual([
      {
        method: 'POST',
        path: serversPath,
        body: {
          name: 'files',
          transport: 'stdio',
          command: 'npx',
          arguments: ['-y', 'server-files'],
          environment: { ROOT: '/srv' },
        },
      },
    ]);
    await expect(row(page, 'files')).toBeVisible();
  });

  test('editing sends only what changed and never shows a saved secret', async ({ page }) => {
    const mock = await openOrganization(page);
    const dialog = page.getByRole('dialog', { name: 'Edit docs' });
    const value = dialog.getByLabel('Value for Authorization', { exact: true });

    await chooseAction(page, 'docs', 'Edit');
    await expect(value).toHaveValue('');
    await expect(value).toHaveAttribute('placeholder', 'Saved');
    await expect(value).toHaveAttribute('type', 'password');
    await dialog.getByLabel('URL', { exact: true }).fill('https://docs.example/v2/mcp');
    await dialog.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(dialog).toHaveCount(0);

    await chooseAction(page, 'docs', 'Edit');
    await value.fill('Bearer new');
    await dialog.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(dialog).toHaveCount(0);

    expect(writes(mock)).toEqual([
      {
        method: 'PUT',
        path: `${serversPath}/${docsId}`,
        body: { url: 'https://docs.example/v2/mcp' },
      },
      {
        method: 'PUT',
        path: `${serversPath}/${docsId}`,
        body: { headers: { Authorization: 'Bearer new' } },
      },
    ]);
    expect(mock.secrets.get(`${docsId}:headers:Authorization`)).toBe('Bearer new');
    let content = await page.content();
    expect(content).not.toContain('Bearer new');
    expect(content).not.toContain(fixtureSecret);

    await chooseAction(page, 'docs', 'Edit');
    await expect(value).toHaveValue('');
    await expect(value).toHaveAttribute('placeholder', 'Saved');
    content = await page.content();
    expect(content).not.toContain('Bearer new');
    expect(content).not.toContain(fixtureSecret);
  });

  test('an organization member sees the list but cannot change it', async ({ page }) => {
    await openOrganization(page, { role: 'member' });

    await expect(page.getByText(readOnly)).toBeVisible();
    await expect(row(page, 'docs').getByRole('status')).toHaveText('Connected');
    await expect(page.getByRole('button', { name: 'Add server' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Import JSON' })).toHaveCount(0);
    await expect(page.getByRole('switch')).toHaveCount(0);
    await expect(page.getByRole('button', { name: /^Actions for / })).toHaveCount(0);
  });

  test('a workspace admin turns docs off for this workspace', async ({ page }) => {
    const mock = await openWorkspace(page, { overrides: { [githubId]: true } });

    const docs = page.getByRole('radiogroup', { name: 'docs in this workspace' });
    await expect(docs.getByRole('radio', { name: 'Inherit' })).toBeChecked();
    await expect(
      page
        .getByRole('radiogroup', { name: 'github in this workspace' })
        .getByRole('radio', { name: 'On' })
    ).toBeChecked();
    const notes = page.getByRole('group', { name: 'notes', exact: true });
    await expect(notes).toContainText('Off for the organization');
    await expect(notes.getByRole('radio', { name: 'On' })).toBeDisabled();
    const instance = page.getByRole('group', { name: 'magents', exact: true });
    await expect(instance).toContainText('Always on');
    await expect(instance.getByRole('radio')).toHaveCount(0);

    await docs.getByRole('radio', { name: 'Off' }).click();
    await expect(docs.getByRole('radio', { name: 'Off' })).toBeChecked();

    expect(writes(mock)).toEqual([
      { method: 'PUT', path: workspacePath, body: { servers: { [docsId]: false } } },
    ]);
  });

  test('a workspace member sees disabled overrides', async ({ page }) => {
    await openWorkspace(page, { workspaceRole: 'member' });

    await expect(page.getByText(workspaceReadOnly)).toBeVisible();
    const radios = page
      .getByRole('radiogroup', { name: 'docs in this workspace' })
      .getByRole('radio');
    await expect(radios).toHaveCount(3);
    for (const radio of await radios.all()) {
      await expect(radio).toBeDisabled();
    }
  });

  test('nothing scrolls sideways at phone width', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await openOrganization(page);
    await expectNoSidewaysScroll(page);

    await fillHttpServer(page);
    await expectDialogContained(page);
    await expectNoSidewaysScroll(page);
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog')).toHaveCount(0);

    await page.goto(workspacePage);
    await expect(page.getByRole('group', { name: 'docs', exact: true })).toBeVisible();
    await expectNoSidewaysScroll(page);
  });
});

test.describe('screenshots', () => {
  test.skip(!directory, 'set MCP_SCREENSHOTS to an absolute directory');

  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  async function themed(page: Page, theme: string): Promise<void> {
    await page.addInitScript((value) => localStorage.setItem('manager_theme', value), theme);
  }

  async function expectTheme(page: Page, theme: string): Promise<void> {
    await expect(page.locator(`html[data-theme="${theme}"]`)).toHaveCount(1);
  }

  async function still(page: Page, name: string, theme: string): Promise<void> {
    const viewport = page.viewportSize();
    if (viewport) await page.mouse.move(viewport.width - 1, viewport.height - 1);
    await page.screenshot({
      path: path.join(directory ?? '', `${name}-${theme}.png`),
      fullPage: true,
      animations: 'disabled',
    });
  }

  for (const theme of ['light', 'dark']) {
    test(`organization states in ${theme}`, async ({ page }) => {
      await themed(page, theme);
      await openOrganization(page);
      await expectTheme(page, theme);
      await expect(row(page, 'search').locator('.spinner')).toBeVisible();
      await still(page, 'org-list', theme);

      const expand = row(page, 'docs').getByRole('button', { name: 'Tools for docs' });
      await expand.click();
      await expect(row(page, 'docs').getByRole('checkbox')).toHaveCount(3);
      await still(page, 'org-expanded', theme);
      await expand.click();

      let dialog = await fillHttpServer(page);
      await still(page, 'form-http', theme);

      await chooseSelect(dialog.getByRole('combobox', { name: 'Transport' }), 'Command (stdio)');
      await dialog.getByLabel('Name', { exact: true }).fill('files');
      await dialog.getByLabel('Command', { exact: true }).fill('npx');
      await dialog
        .getByLabel('Arguments', { exact: true })
        .fill('-y\n@modelcontextprotocol/server-filesystem\n/srv/files');
      await dialog.getByRole('button', { name: 'Add variable', exact: true }).click();
      await dialog.getByLabel('Variable name 1', { exact: true }).fill('ROOT');
      await dialog.getByLabel('Value for ROOT', { exact: true }).fill('/srv');
      await still(page, 'form-stdio', theme);
      await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
      await expect(dialog).toHaveCount(0);

      await chooseAction(page, 'docs', 'Edit');
      dialog = page.getByRole('dialog', { name: 'Edit docs' });
      await expect(dialog.getByLabel('Value for Authorization')).toHaveAttribute(
        'placeholder',
        'Saved'
      );
      await still(page, 'form-edit', theme);
      await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
      await expect(dialog).toHaveCount(0);

      await chooseAction(page, 'notes', 'Remove');
      dialog = page.getByRole('dialog', { name: 'Remove notes?' });
      await expect(dialog).toBeVisible();
      await still(page, 'remove-confirmation', theme);
      await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
      await expect(dialog).toHaveCount(0);

      await page.getByRole('button', { name: 'Import JSON', exact: true }).click();
      dialog = page.getByRole('dialog', { name: 'Import MCP servers' });
      await dialog.getByLabel('mcpServers JSON').fill(
        JSON.stringify(
          {
            mcpServers: {
              files: { command: 'npx', args: ['-y', 'server-files'], env: { ROOT: '/srv' } },
              wiki: { url: 'https://wiki.example/mcp' },
              legacy: { type: 'sse', url: 'https://legacy.example/sse' },
            },
          },
          null,
          2
        )
      );
      await expect(dialog.getByRole('list', { name: 'Skipped' })).toBeVisible();
      await still(page, 'import-dialog', theme);
    });

    test(`organization without commands in ${theme}`, async ({ page }) => {
      await themed(page, theme);
      await openOrganization(page, { commands: false });
      await expectTheme(page, theme);
      await page.getByRole('button', { name: 'Add server', exact: true }).click();
      const dialog = page.getByRole('dialog', { name: 'Add MCP server' });
      await dialog.getByRole('combobox', { name: 'Transport' }).click();
      await expect(page.getByRole('listbox').getByRole('option')).toHaveText(['Streamable HTTP']);
      await still(page, 'form-no-stdio', theme);
    });

    test(`organization member in ${theme}`, async ({ page }) => {
      await themed(page, theme);
      await openOrganization(page, { role: 'member' });
      await expectTheme(page, theme);
      await expect(page.getByText(readOnly)).toBeVisible();
      await still(page, 'org-member', theme);
    });

    test(`workspace overrides in ${theme}`, async ({ page }) => {
      await themed(page, theme);
      await openWorkspace(page, { overrides: { [githubId]: true, [searchId]: false } });
      await expectTheme(page, theme);
      await still(page, 'workspace-overrides', theme);
    });

    test(`narrow views in ${theme}`, async ({ page }) => {
      await page.setViewportSize({ width: 375, height: 812 });
      await themed(page, theme);
      await openOrganization(page);
      await expectTheme(page, theme);
      await expectNoSidewaysScroll(page);
      await still(page, 'org-list-narrow', theme);

      const dialog = await fillHttpServer(page);
      await expectDialogContained(page);
      await still(page, 'form-http-narrow', theme);
      await page.keyboard.press('Escape');
      await expect(dialog).toHaveCount(0);

      await page.goto(workspacePage);
      await expect(page.getByRole('group', { name: 'docs', exact: true })).toBeVisible();
      await expectNoSidewaysScroll(page);
      await still(page, 'workspace-overrides-narrow', theme);
    });
  }
});
