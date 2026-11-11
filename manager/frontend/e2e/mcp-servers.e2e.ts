import path from 'node:path';
import type { Locator, Page } from '@playwright/test';
import { z } from 'zod';
import { McpTransportSchema } from '../src/features/settings/mcp/schemas';
import type {
  CreateMcpServerRequest,
  McpSecretField,
  McpServer,
  UpdateMcpServerRequest,
  UpdateWorkspaceMcpServersRequest,
} from '../src/features/settings/mcp/types';
import { expect, test } from './fixtures';
import { setupAuth } from './helpers/auth';
import { chooseSelect, expectSelectOptions } from './helpers/select';
import { blockServiceWorker, routeApi } from './test-utils';

type OrgRole = 'owner' | 'admin' | 'member';
type WorkspaceRole = 'owner' | 'admin' | 'member' | 'viewer';

interface LeakedSecretField extends McpSecretField {
  value?: string;
}

interface WireServer extends Omit<McpServer, 'environment' | 'headers'> {
  environment: LeakedSecretField[];
  headers: LeakedSecretField[];
}

interface Scenario {
  role?: OrgRole;
  workspaceRole?: WorkspaceRole | null;
  commands?: boolean;
  servers?: WireServer[];
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

const SecretMapSchema = z.record(z.string(), z.string());
const SecretChangesSchema = z.record(z.string(), z.string().nullable());
const ToolsSchema = z.array(z.string()).nullable();

const CreateRequestSchema = z.strictObject({
  name: z.string(),
  transport: McpTransportSchema,
  command: z.string().optional(),
  arguments: z.array(z.string()).optional(),
  url: z.string().optional(),
  tools: ToolsSchema.optional(),
  enabled: z.boolean().optional(),
  environment: SecretMapSchema.optional(),
  headers: SecretMapSchema.optional(),
}) satisfies z.ZodType<CreateMcpServerRequest>;

const UpdateRequestSchema = z.strictObject({
  name: z.string().optional(),
  command: z.string().optional(),
  arguments: z.array(z.string()).optional(),
  url: z.string().optional(),
  tools: ToolsSchema.optional(),
  enabled: z.boolean().optional(),
  environment: SecretChangesSchema.optional(),
  headers: SecretChangesSchema.optional(),
}) satisfies z.ZodType<UpdateMcpServerRequest>;

const UpdateWorkspaceRequestSchema = z.strictObject({
  servers: z.record(z.string(), z.boolean().nullable()),
}) satisfies z.ZodType<UpdateWorkspaceMcpServersRequest>;

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
const commandsDisabled = 'Command servers are turned off on this instance (ZONE_MCP_COMMANDS)';
const unconnected = 'did not connect; see the server log';
const transportFixed = '`transport` cannot be changed; remove the server and add it again';
const workspaceMissing = 'Workspace not found';
const reservedHeaders = ['accept', 'content-type', 'host', 'mcp-session-id'];
const metadataAddress = 'Do not use a link-local or cloud metadata address';
const metadataRefused = '`url` must not point at a link-local or cloud metadata address';
const refusedHost = 'refused.example';
const directory = process.env.MCP_SCREENSHOTS;

function server(changes: Partial<WireServer> & Pick<WireServer, 'name'>): WireServer {
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

function fixtures(): WireServer[] {
  return [
    server({
      id: docsId,
      name: 'docs',
      url: 'https://docs.example/mcp',
      tools: ['search_pages', 'fetch_page'],
      headers: [{ name: 'Authorization', has_value: true, value: fixtureSecret }],
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
      error: unconnected,
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
      arguments: [],
      available_tools: ['send_message', 'list_sessions'],
      created_at: null,
      updated_at: null,
    }),
  ];
}

function storeSecrets(
  secrets: Map<string, string>,
  id: string,
  kind: string,
  previous: LeakedSecretField[],
  map: Record<string, string | null> = {}
): LeakedSecretField[] {
  return Object.entries(map).map(([name, value]) => {
    if (value !== null) {
      secrets.set(`${id}:${kind}:${name}`, value);
      return { name, has_value: true };
    }
    return { name, has_value: previous.find((field) => field.name === name)?.has_value ?? false };
  });
}

function has(value: object | undefined): boolean {
  return value !== undefined && Object.keys(value).length > 0;
}

function refusal(
  transport: McpServer['transport'],
  fields: CreateMcpServerRequest | UpdateMcpServerRequest
): string | null {
  const unused =
    transport === 'stdio'
      ? { url: fields.url !== undefined, headers: has(fields.headers) }
      : {
          command: fields.command !== undefined,
          arguments: has(fields.arguments),
          environment: has(fields.environment),
        };
  const field = Object.entries(unused).find(([, present]) => present)?.[0];
  if (field) {
    return `\`${field}\` is not allowed for \`${transport}\` servers`;
  }
  if (fields.url !== undefined && new URL(fields.url).hostname === refusedHost) {
    return metadataRefused;
  }
  const reserved = Object.keys(fields.headers ?? {}).find((name) =>
    reservedHeaders.includes(name.toLowerCase())
  );
  return reserved
    ? `\`headers\` name \`${reserved}\` is set by the connection itself and cannot be configured`
    : null;
}

async function mockApi(page: Page, scenario: Scenario = {}): Promise<Mock> {
  const role = scenario.role ?? 'admin';
  const workspaceRole = scenario.workspaceRole === undefined ? 'admin' : scenario.workspaceRole;
  const commands = scenario.commands ?? true;
  const servers = scenario.servers ?? fixtures();
  for (const entry of servers) {
    if (!commands && entry.source === 'organization' && entry.transport === 'stdio') {
      entry.status = entry.enabled ? 'failed' : 'disabled';
      entry.error = entry.enabled ? commandsDisabled : null;
      entry.available_tools = [];
    }
  }
  const overrides = new Map(Object.entries(scenario.overrides ?? {}));
  const captured: Captured[] = [];
  const secrets = new Map<string, string>([[`${docsId}:headers:Authorization`, fixtureSecret]]);
  const pending = new Map<string, number>();
  let created = 0;

  const connect = (target: WireServer) => {
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
          members: workspaceRole
            ? [
                {
                  id: '00000000-0000-0000-0000-0000000000a1',
                  user_id: userId,
                  workspace_id: workspaceId,
                  role: workspaceRole,
                  email: 'user@example.com',
                  display_name: null,
                  joined_at: '2024-01-01T00:00:00Z',
                },
              ]
            : [],
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
      const posted = CreateRequestSchema.parse(body);
      if (posted.transport === 'stdio' && !commands) {
        await route.fulfill({ status: 403, json: { error: commandsDisabled } });
        return;
      }
      const refused = refusal(posted.transport, posted);
      if (refused) {
        await route.fulfill({ status: 400, json: { error: refused } });
        return;
      }
      created += 1;
      const id = `22222222-2222-4222-8222-${String(created).padStart(12, '0')}`;
      const entry = server({
        id,
        name: posted.name,
        transport: posted.transport,
        command: posted.command ?? null,
        arguments: posted.arguments ?? [],
        url: posted.url ?? null,
        tools: posted.tools ?? null,
        enabled: posted.enabled ?? true,
        environment: storeSecrets(secrets, id, 'environment', [], posted.environment),
        headers: storeSecrets(secrets, id, 'headers', [], posted.headers),
      });
      if (entry.enabled) connect(entry);
      else entry.status = 'disabled';
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
      const locked = target.transport === 'stdio' && !commands;
      if (action === 'connection' && method === 'POST') {
        if (locked) {
          await route.fulfill({ status: 403, json: { error: commandsDisabled } });
          return;
        }
        if (target.enabled) connect(target);
        await route.fulfill({ status: 202, json: target });
        return;
      }
      if (method === 'DELETE') {
        servers.splice(index, 1);
        await route.fulfill({ status: 204 });
        return;
      }
      if (method === 'PUT') {
        if (typeof body === 'object' && body !== null && 'transport' in body) {
          await route.fulfill({ status: 400, json: { error: transportFixed } });
          return;
        }
        const changes = UpdateRequestSchema.parse(body);
        if (locked) {
          await route.fulfill({ status: 403, json: { error: commandsDisabled } });
          return;
        }
        const refused = refusal(target.transport, changes);
        if (refused) {
          await route.fulfill({ status: 400, json: { error: refused } });
          return;
        }
        let reconnect = false;
        for (const key of ['name', 'command', 'arguments', 'url'] as const) {
          if (changes[key] !== undefined) {
            Object.assign(target, { [key]: changes[key] });
            reconnect = true;
          }
        }
        if (changes.tools !== undefined) target.tools = changes.tools;
        for (const kind of ['environment', 'headers'] as const) {
          if (changes[kind] !== undefined) {
            target[kind] = storeSecrets(secrets, id, kind, target[kind], changes[kind]);
            reconnect = true;
          }
        }
        if (changes.enabled !== undefined) {
          target.enabled = changes.enabled;
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
      if (workspaceRole === null) {
        await route.fulfill({ status: 404, json: { error: workspaceMissing } });
        return;
      }
      if (method === 'PUT') {
        const { servers: changes } = UpdateWorkspaceRequestSchema.parse(body);
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
    await expect(row(page, 'github')).toContainText(unconnected);
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

  test('importing keeps a disabled server off and names the keys it leaves out', async ({
    page,
  }) => {
    const mock = await openOrganization(page);

    await page.getByRole('button', { name: 'Import JSON', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Import MCP servers' });
    await dialog.getByLabel('mcpServers JSON').fill(
      JSON.stringify({
        mcpServers: {
          files: { command: 'npx', args: ['-y', 'server-files'], cwd: '/srv', timeout: 30 },
          wiki: {
            url: 'https://wiki.example/mcp',
            headers: { Authorization: 'Bearer t' },
            disabled: true,
          },
        },
      })
    );

    const items = dialog.getByRole('list', { name: 'Servers to add' }).getByRole('listitem');
    await expect(items).toHaveCount(2);
    await expect(items.nth(0)).toContainText('Not imported: cwd, timeout');
    await expect(items.nth(1).getByText('Disabled', { exact: true })).toBeVisible();
    await expect(items.nth(1)).not.toContainText('Not imported');
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
        },
      },
      {
        method: 'POST',
        path: serversPath,
        body: {
          name: 'wiki',
          transport: 'http',
          url: 'https://wiki.example/mcp',
          headers: { Authorization: 'Bearer t' },
          enabled: false,
        },
      },
    ]);
    await expect(row(page, 'wiki').getByRole('status')).toHaveText('Disabled');
    await expect(page.getByRole('switch', { name: 'Enable wiki' })).not.toBeChecked();
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

  test('with commands off a command server cannot be switched or reconnected', async ({ page }) => {
    const mock = await openOrganization(page, { commands: false });

    const github = row(page, 'github');
    await expect(github.getByRole('status')).toHaveText('Failed');
    await expect(github).toContainText(commandsDisabled);
    const toggle = page.getByRole('switch', { name: 'Enable github' });
    await expect(toggle).toBeDisabled();
    await expect(toggle).toHaveAccessibleDescription(commandsOff);
    await expect(github.locator('label.mcp-server-toggle')).toHaveAttribute('title', commandsOff);

    await page.getByRole('button', { name: 'Actions for github', exact: true }).click();
    const reconnect = page.getByRole('menuitem', { name: /^Reconnect/ });
    await expect(reconnect).toBeDisabled();
    await expect(reconnect).toContainText(commandsOff);
    await expect(page.getByRole('menuitem', { name: 'Remove', exact: true })).toBeEnabled();
    await page.keyboard.press('Escape');

    await expect(page.getByRole('switch', { name: 'Enable docs' })).toBeEnabled();
    await chooseAction(page, 'docs', 'Reconnect');
    await expect(row(page, 'docs').getByRole('status')).toHaveText('Connecting');
    expect(writes(mock)).toEqual([
      { method: 'POST', path: `${serversPath}/${docsId}/connection`, body: null },
    ]);
  });

  test('a header the connection sets itself is refused before anything is posted', async ({
    page,
  }) => {
    const mock = await openOrganization(page);

    const dialog = await fillHttpServer(page);
    await dialog.getByLabel('Header name 1', { exact: true }).fill('Content-Type');
    await dialog.getByRole('button', { name: 'Add server', exact: true }).click();

    await expect(dialog.getByText('The connection sets this header itself')).toBeVisible();
    await expect(dialog).toBeVisible();
    expect(writes(mock)).toEqual([]);

    await dialog.getByLabel('Header name 1', { exact: true }).fill('Authorization');
    await dialog.getByRole('button', { name: 'Add server', exact: true }).click();
    await expect(dialog).toHaveCount(0);
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
  });

  test('a link-local or metadata address is refused before anything is posted', async ({
    page,
  }) => {
    const mock = await openOrganization(page);

    const dialog = await fillHttpServer(page);
    const url = dialog.getByLabel('URL', { exact: true });
    for (const address of [
      'http://169.254.10.1/mcp',
      'http://[fe80::1]/mcp',
      'http://[::ffff:169.254.169.254]/mcp',
      'http://2852039166/mcp',
      'http://0251.0376.0251.0376/mcp',
      'http://metadata.google.internal./mcp',
    ]) {
      await url.fill(address);
      await dialog.getByRole('button', { name: 'Add server', exact: true }).click();
      await expect(url, address).toHaveAccessibleDescription(metadataAddress);
    }
    expect(writes(mock)).toEqual([]);
  });

  test('a URL only the server refuses is shown on the URL field', async ({ page }) => {
    const mock = await openOrganization(page);

    const dialog = await fillHttpServer(page);
    const url = dialog.getByLabel('URL', { exact: true });
    await url.fill(`https://${refusedHost}/mcp`);
    await dialog.getByRole('button', { name: 'Add server', exact: true }).click();

    await expect(url).toHaveAccessibleDescription(metadataRefused);
    await expect(url).toHaveAttribute('aria-invalid', 'true');
    await expect(dialog.locator('.modal-error')).toHaveCount(0);
    expect(writes(mock)).toHaveLength(1);

    await url.fill('https://wiki.example/mcp');
    await expect(url).toHaveAttribute('aria-invalid', 'false');
    await dialog.getByRole('button', { name: 'Add server', exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await expect(row(page, 'wiki')).toBeVisible();
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
    expect(await page.content()).not.toContain(fixtureSecret);

    await chooseAction(page, 'docs', 'Edit');
    await expect(value).toHaveValue('');
    await expect(value).toHaveAttribute('placeholder', 'Saved');
    await expect(value).toHaveAttribute('type', 'password');
    expect(await page.content()).not.toContain(fixtureSecret);
    await dialog.getByLabel('URL', { exact: true }).fill('https://docs.example/v2/mcp');
    await dialog.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(dialog).toHaveCount(0);

    await chooseAction(page, 'docs', 'Edit');
    await expect(value).toHaveAttribute('placeholder', 'Saved');
    expect(await page.content()).not.toContain(fixtureSecret);
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
    await expect(instance).toContainText('Set by the instance');
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

  test('a caller with no role in the workspace is told it was not found', async ({ page }) => {
    await open(page, workspacePage, { workspaceRole: null });

    await expect(
      page.getByText(`Failed to load workspace MCP servers: ${workspaceMissing}`)
    ).toBeVisible();
    await expect(page.getByRole('button', { name: 'Retry', exact: true })).toBeVisible();
    await expect(page.getByRole('radiogroup')).toHaveCount(0);
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
              files: {
                command: 'npx',
                args: ['-y', 'server-files'],
                env: { ROOT: '/srv' },
                cwd: '/srv',
              },
              wiki: { url: 'https://wiki.example/mcp', disabled: true },
              legacy: { type: 'sse', url: 'https://legacy.example/sse' },
            },
          },
          null,
          2
        )
      );
      await expect(dialog.getByRole('list', { name: 'Skipped' })).toBeVisible();
      await expect(dialog.getByText('Not imported: cwd')).toBeVisible();
      await still(page, 'import-dialog', theme);
    });

    test(`organization without commands in ${theme}`, async ({ page }) => {
      await themed(page, theme);
      await openOrganization(page, { commands: false });
      await expectTheme(page, theme);
      await page.getByRole('button', { name: 'Actions for github', exact: true }).click();
      await expect(page.getByRole('menuitem', { name: /^Reconnect/ })).toBeDisabled();
      await still(page, 'org-list-no-stdio', theme);
      await page.keyboard.press('Escape');
      await page.getByRole('button', { name: 'Add server', exact: true }).click();
      const dialog = page.getByRole('dialog', { name: 'Add MCP server' });
      await dialog.getByRole('combobox', { name: 'Transport' }).click();
      await expect(page.getByRole('listbox').getByRole('option')).toHaveText(['Streamable HTTP']);
      await still(page, 'form-no-stdio', theme);
    });

    test(`URL refusals in ${theme}`, async ({ page }) => {
      await themed(page, theme);
      await openOrganization(page);
      await expectTheme(page, theme);
      const dialog = await fillHttpServer(page);
      const url = dialog.getByLabel('URL', { exact: true });

      await url.fill('http://169.254.169.254/mcp');
      await dialog.getByRole('button', { name: 'Add server', exact: true }).click();
      await expect(url).toHaveAccessibleDescription(metadataAddress);
      await still(page, 'form-metadata', theme);

      await url.fill(`https://${refusedHost}/mcp`);
      await dialog.getByRole('button', { name: 'Add server', exact: true }).click();
      await expect(url).toHaveAccessibleDescription(metadataRefused);
      await still(page, 'form-server-refusal', theme);
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
