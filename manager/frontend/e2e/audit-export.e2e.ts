import { readFile } from 'node:fs/promises';
import path from 'node:path';
import type { Locator, Page, Route } from '@playwright/test';
import { exportFilename } from '../src/features/enterprise/audit/filename';
import {
  CAPPED_DESTINATIONS,
  CAPPED_RETENTION,
  CAPPED_WEBHOOK,
  COMMUNITY_DESTINATIONS,
  COMMUNITY_RETENTION,
  COMMUNITY_SAVED_RETENTION,
  DELIVERED_TEST,
  ENTERPRISE_DESTINATIONS,
  ENTERPRISE_EMPTY_DESTINATIONS,
  EXPORT_QUERY,
  FAILED_TEST,
  FAILING_WEBHOOK,
  FOREVER_RETENTION,
  HEALTHY_S3,
  LAPSED_DESTINATIONS,
  LAPSED_RETENTION,
  LAPSED_TEAM_DESTINATIONS,
  LAPSED_TEAM_RETENTION,
  NOW,
  PAUSED_SYSLOG,
  SERVER_DESTINATIONS,
  SERVER_RETENTION,
  SERVER_SYSLOG,
  TEAM_DESTINATIONS,
  TEAM_RETENTION,
  UNVERIFIED_DESTINATIONS,
  UNVERIFIED_RETENTION,
  WAITING_RETENTION,
} from '../src/features/enterprise/audit/fixtures';
import {
  ADD_DESTINATION_LABEL,
  AUDIT_LOG_LINK,
  DELETE_CONFIRM_LABEL,
  DELETE_LABEL,
  DELETE_TITLE,
  DOWNLOAD_LABEL,
  DOWNLOAD_READ_ONLY,
  DOWNLOAD_UNVERIFIED,
  DOWNLOADED_LABEL,
  EDIT_DESTINATION_TITLE,
  EDIT_LABEL,
  EDITION_NOTICE,
  END_DATE_LABEL,
  FORMAT_LABEL,
  GENERATE_SECRET_LABEL,
  HOLDERS_LABEL,
  KEEP_FOREVER_LABEL,
  KEPT_FOREVER,
  LAPSED_NOTICE,
  LAPSED_TEAM_NOTICE,
  NAME_LABEL,
  NEW_DESTINATION_TITLE,
  NO_DESTINATIONS,
  NO_DESTINATIONS_READ_ONLY,
  ORGANIZATION_DESCRIPTION,
  PASTE_KEY_LINK_LABEL,
  READ_ONLY_EDITION,
  READ_ONLY_UNLICENSED,
  READ_ONLY_UNVERIFIED,
  RENEW_LINK_LABEL,
  RETENTION_DAYS_LABEL,
  RETENTION_SAVED,
  RETENTION_STILL_APPLIED,
  SAVE_DESTINATION_LABEL,
  SAVE_RETENTION_LABEL,
  SECRET_SAVED_PLACEHOLDER,
  SECTION_TITLE,
  SERVER_DESCRIPTION,
  START_DATE_LABEL,
  UNENCRYPTED_HTTP_WARNING,
  UNVERIFIED_NOTICE,
  UPGRADE_NOTICE,
  URL_LABEL,
} from '../src/features/enterprise/audit/labels';
import {
  backlogText,
  countText,
  deleteMessage,
  enabledLabel,
  formatLabel,
  healthLabel,
  retentionSummary,
  retentionText,
  secretLabel,
  testLabel,
  testResultMessage,
  waitMessage,
} from '../src/features/enterprise/audit/messages';
import {
  AuditKind,
  DEFAULT_ADDRESSING,
  DEFAULT_FACILITY,
  DEFAULT_SYSLOG_HOSTNAME,
  DeliveryHealth,
  ExportFormat,
  ExportQuerySchema,
  RetentionWait,
  SecretName,
} from '../src/features/enterprise/audit/schemas';
import { organizationScope, SERVER_SCOPE } from '../src/features/enterprise/audit/scope';
import type {
  AuditScope,
  CreateDestinationRequest,
  DeliveryHealth as DeliveryHealthValue,
  DeliveryResponse,
  DestinationResponse,
  DestinationsResponse,
  ExportQuery,
  HolderResponse,
  RetentionResponse,
  RetentionWait as RetentionWaitValue,
  SecretName as SecretNameValue,
  Settings,
  SettingsRequest,
  TestResponse,
  UpdateDestinationRequest,
  UpdateRetentionRequest,
} from '../src/features/enterprise/audit/types';
import { expect, test } from './fixtures';
import { routeWorkspaceRole, setupAdminAuth, setupCommonRoutes } from './layout-fixtures';
import { blockServiceWorker, routeApi } from './test-utils';

const ORGANIZATION_PATH = '/settings/organization/audit-export';
const SERVER_PATH = '/settings/server/audit-export';
const AUDIT_LOG_PATH = '/settings/organization/audit';

const ORGANIZATION_API =
  /\/api\/organizations\/[^/]+\/audit-(?:destinations|retention|exports)(?:[/?]|$)/;
const SERVER_API = /\/api\/audit-(?:destinations|retention|exports)(?:[/?]|$)/;
const AUDIT_PATH =
  /^\/api(?:\/organizations\/([^/]+))?\/audit-(destinations|retention|exports)(?:\/([^/]+))?(\/tests)?$/;
const AUDIT_LOGS_API = /\/api\/organizations\/[^/]+\/audit-logs(?:\?|$)/;

const NEW_DESTINATION_ID = '6e7f8a9b-0c1d-4e2f-9a3b-4c5d6e7f8a9b';
const WEBHOOK_NAME = 'Splunk';
const RENAMED_WEBHOOK = 'Splunk Cloud';
const PLAIN_URL = 'http://splunk:8088/services/collector';
const SECURE_URL = 'https://splunk.acme.com/services/collector';
const RETENTION_DAYS = 30;
const SWEPT_EARLIER = 70;
const DAY_MILLISECONDS = 86_400_000;
const EXPORT_BODY = '{"id":"5d1e","action":"sign_in"}\n';
const NDJSON_TYPE = 'application/x-ndjson';
const CSV_TYPE = 'text/csv';
const CSV_QUERY: ExportQuery = { ...EXPORT_QUERY, format: ExportFormat.Csv };

const DESKTOP = { width: 1280, height: 800 };
const PHONE = { width: 375, height: 812 };
const WIDTHS = [DESKTOP, PHONE];
const SCHEMES = ['light', 'dark'] as const;
type Scheme = (typeof SCHEMES)[number];

const SCREENSHOTS = process.env.AUDIT_SCREENSHOTS;

const Resource = {
  Destinations: 'destinations',
  Retention: 'retention',
  Exports: 'exports',
} as const;

const WAITS: Readonly<Partial<Record<DeliveryHealthValue, RetentionWaitValue>>> = {
  [DeliveryHealth.Failing]: RetentionWait.Failing,
  [DeliveryHealth.Paused]: RetentionWait.Paused,
};

interface ExportRequest {
  organizationId: string | null;
  query: ExportQuery;
}

interface AuditServer {
  destinations: DestinationsResponse;
  retention: RetentionResponse;
  created: CreateDestinationRequest[];
  updates: UpdateDestinationRequest[];
  deletions: string[];
  tests: string[];
  retentions: UpdateRetentionRequest[];
  exports: ExportRequest[];
}

interface AuditSetup {
  destinations: DestinationsResponse;
  retention: RetentionResponse;
  results?: Readonly<Record<string, TestResponse>>;
}

interface OpenOptions {
  scheme?: Scheme;
}

interface SavedDownload {
  filename: string;
  contents: string;
}

function stored(settings: SettingsRequest): Settings {
  switch (settings.kind) {
    case AuditKind.Webhook:
      return { kind: AuditKind.Webhook, url: settings.url };
    case AuditKind.S3:
      return {
        kind: AuditKind.S3,
        bucket: settings.bucket,
        region: settings.region,
        endpoint: settings.endpoint ?? null,
        prefix: settings.prefix ?? '',
        addressing: settings.addressing ?? DEFAULT_ADDRESSING,
      };
    case AuditKind.Syslog:
      return {
        kind: AuditKind.Syslog,
        host: settings.host,
        port: settings.port,
        protocol: settings.protocol,
        facility: settings.facility ?? DEFAULT_FACILITY,
        hostname: settings.hostname ?? DEFAULT_SYSLOG_HOSTNAME,
      };
  }
}

function secretNames(
  saved: readonly SecretNameValue[],
  secrets: Readonly<Partial<Record<SecretNameValue, string>>> = {}
): SecretNameValue[] {
  const entries = Object.entries(secrets) as [SecretNameValue, string][];
  const removed = new Set(entries.filter(([, value]) => value === '').map(([name]) => name));
  const added = entries.filter(([, value]) => value !== '').map(([name]) => name);
  return [...new Set([...saved, ...added])].filter((name) => !removed.has(name));
}

function freshDelivery(enabled: boolean): DeliveryResponse {
  return {
    health: enabled ? DeliveryHealth.Healthy : DeliveryHealth.Disabled,
    backlog: 0,
    backlog_capped: false,
    oldest_pending_at: null,
    attempts: 0,
    next_attempt_at: enabled ? NOW : null,
    last_success_at: null,
    last_failure_at: null,
    last_error_code: null,
    last_error: null,
    delivered_count: 0,
  };
}

function createdDestination(request: CreateDestinationRequest): DestinationResponse {
  const enabled = request.enabled ?? true;
  return {
    id: NEW_DESTINATION_ID,
    name: request.name,
    kind: request.settings.kind,
    enabled,
    settings: stored(request.settings),
    secrets: secretNames([], request.secrets),
    delivery: freshDelivery(enabled),
    created_at: NOW,
    updated_at: NOW,
  };
}

function updatedDelivery(delivery: DeliveryResponse, enabled: boolean): DeliveryResponse {
  if (!enabled) {
    return { ...delivery, health: DeliveryHealth.Disabled, next_attempt_at: null };
  }
  return delivery.health === DeliveryHealth.Disabled
    ? { ...delivery, health: DeliveryHealth.Healthy, next_attempt_at: NOW }
    : delivery;
}

function updatedDestination(
  destination: DestinationResponse,
  request: UpdateDestinationRequest
): DestinationResponse {
  const enabled = request.enabled ?? destination.enabled;
  return {
    ...destination,
    name: request.name ?? destination.name,
    enabled,
    settings: request.settings ? stored(request.settings) : destination.settings,
    secrets: secretNames(destination.secrets, request.secrets),
    delivery: updatedDelivery(destination.delivery, enabled),
    updated_at: NOW,
  };
}

function holdersOf(destinations: readonly DestinationResponse[]): HolderResponse[] {
  return destinations
    .filter((destination) => destination.enabled && destination.delivery.backlog > 0)
    .map((destination) => ({
      destination_id: destination.id,
      name: destination.name,
      wait: WAITS[destination.delivery.health] ?? RetentionWait.Backlog,
    }));
}

function retained(server: AuditServer, days: number | null): RetentionResponse {
  if (days === null) {
    return {
      ...server.retention,
      days: null,
      cutoff: null,
      expired: 0,
      expired_capped: false,
      held: 0,
      held_capped: false,
      holders: [],
      updated_at: NOW,
    };
  }
  const holders = holdersOf(server.destinations.destinations);
  const held = server.destinations.destinations
    .filter((destination) => holders.some((holder) => holder.destination_id === destination.id))
    .reduce((total, destination) => total + destination.delivery.backlog, 0);
  return {
    ...server.retention,
    days,
    cutoff: new Date(Date.parse(NOW) - days * DAY_MILLISECONDS).toISOString(),
    expired: held + SWEPT_EARLIER,
    held,
    holders,
    updated_at: NOW,
  };
}

function replaceDestinations(server: AuditServer, destinations: DestinationResponse[]): void {
  server.destinations = { ...server.destinations, destinations };
}

function serveDestination(
  route: Route,
  server: AuditServer,
  destinationId: string,
  results: Readonly<Record<string, TestResponse>>,
  tests: boolean
) {
  const request = route.request();
  const existing = server.destinations.destinations.find(
    (destination) => destination.id === destinationId
  );
  if (!existing) {
    return route.fulfill({
      status: 404,
      json: { error: 'Destination not found', code: 'audit_destination_not_found' },
    });
  }
  if (tests) {
    server.tests.push(destinationId);
    return route.fulfill({ status: 200, json: results[destinationId] ?? DELIVERED_TEST });
  }
  if (request.method() === 'DELETE') {
    server.deletions.push(destinationId);
    replaceDestinations(
      server,
      server.destinations.destinations.filter((destination) => destination.id !== destinationId)
    );
    return route.fulfill({ status: 204, body: '' });
  }
  if (request.method() === 'PATCH') {
    const update = request.postDataJSON() as UpdateDestinationRequest;
    server.updates.push(update);
    const updated = updatedDestination(existing, update);
    replaceDestinations(
      server,
      server.destinations.destinations.map((destination) =>
        destination.id === destinationId ? updated : destination
      )
    );
    return route.fulfill({ status: 200, json: updated });
  }
  return route.fulfill({ status: 200, json: existing });
}

function serveExport(route: Route, server: AuditServer, organizationId: string | null) {
  const url = new URL(route.request().url());
  const query = ExportQuerySchema.parse(Object.fromEntries(url.searchParams));
  server.exports.push({ organizationId, query });
  const scope: AuditScope = organizationId ? organizationScope(organizationId) : SERVER_SCOPE;
  return route.fulfill({
    status: 200,
    headers: {
      'Content-Type': query.format === ExportFormat.Csv ? CSV_TYPE : NDJSON_TYPE,
      'Content-Disposition': `attachment; filename="${exportFilename(scope, query)}"`,
    },
    body: EXPORT_BODY,
  });
}

async function serveAudit(page: Page, api: RegExp, setup: AuditSetup): Promise<AuditServer> {
  const server: AuditServer = {
    destinations: setup.destinations,
    retention: setup.retention,
    created: [],
    updates: [],
    deletions: [],
    tests: [],
    retentions: [],
    exports: [],
  };
  const results = setup.results ?? {};

  await routeApi(page, api, (route) => {
    const request = route.request();
    const match = new URL(request.url()).pathname.match(AUDIT_PATH);
    if (!match) {
      return route.fallback();
    }
    const [, organizationId, resource, destinationId, tests] = match;
    if (resource === Resource.Exports) {
      return serveExport(route, server, organizationId ?? null);
    }
    if (resource === Resource.Retention) {
      if (request.method() === 'PUT') {
        const update = request.postDataJSON() as UpdateRetentionRequest;
        server.retentions.push(update);
        server.retention = retained(server, update.days);
      }
      return route.fulfill({ status: 200, json: server.retention });
    }
    if (destinationId) {
      return serveDestination(
        route,
        server,
        decodeURIComponent(destinationId),
        results,
        tests !== undefined
      );
    }
    if (request.method() === 'POST') {
      const creation = request.postDataJSON() as CreateDestinationRequest;
      server.created.push(creation);
      const destination = createdDestination(creation);
      replaceDestinations(server, [...server.destinations.destinations, destination]);
      return route.fulfill({ status: 201, json: destination });
    }
    return route.fulfill({ status: 200, json: server.destinations });
  });

  return server;
}

async function signIn(page: Page, { scheme = 'light' }: OpenOptions = {}): Promise<void> {
  await blockServiceWorker(page.context());
  await page.clock.setFixedTime(NOW);
  await page.emulateMedia({ colorScheme: scheme });
  await setupCommonRoutes(page, false);
  await routeWorkspaceRole(page, 'admin');
  await page.goto('/login', { waitUntil: 'domcontentloaded' });
  await setupAdminAuth(page, [], new Date(NOW));
}

async function openSection(
  page: Page,
  sectionPath: string,
  api: RegExp,
  setup: AuditSetup,
  options: OpenOptions = {}
): Promise<AuditServer> {
  await signIn(page, options);
  const server = await serveAudit(page, api, setup);
  await page.goto(sectionPath, { waitUntil: 'domcontentloaded' });
  await expect(page.getByRole('heading', { name: SECTION_TITLE, level: 2 })).toBeVisible();
  return server;
}

function openOrganization(
  page: Page,
  setup: AuditSetup,
  options?: OpenOptions
): Promise<AuditServer> {
  return openSection(page, ORGANIZATION_PATH, ORGANIZATION_API, setup, options);
}

function openServer(page: Page, setup: AuditSetup, options?: OpenOptions): Promise<AuditServer> {
  return openSection(page, SERVER_PATH, SERVER_API, setup, options);
}

function row(page: Page, name: string): Locator {
  return page.getByRole('listitem', { name, exact: true });
}

function retentionCard(page: Page): Locator {
  return page.locator('.audit-retention');
}

async function expectNoSidewaysScroll(page: Page): Promise<void> {
  const overflow = await page.evaluate(() =>
    Math.max(
      document.documentElement.scrollWidth - document.documentElement.clientWidth,
      ...Array.from(
        document.querySelectorAll('.audit-section, [role="dialog"]'),
        (element) => element.scrollWidth - element.clientWidth
      )
    )
  );
  expect(overflow, 'the page never scrolls sideways').toBeLessThanOrEqual(0);
}

function screenshotPath(name: string): string {
  const file = `${name}.png`;
  return SCREENSHOTS ? path.join(SCREENSHOTS, file) : test.info().outputPath(file);
}

function contentHeight(page: Page): Promise<number> {
  return page.evaluate(() =>
    Math.ceil(
      Math.max(
        ...Array.from(
          document.querySelectorAll('*'),
          (element) => element.getBoundingClientRect().top + element.scrollHeight
        )
      )
    )
  );
}

async function shoot(page: Page, name: string): Promise<void> {
  await expectNoSidewaysScroll(page);
  const viewport = page.viewportSize() ?? DESKTOP;
  let height = viewport.height;
  for (let measured = await contentHeight(page); measured > height; ) {
    height = measured;
    await page.setViewportSize({ width: viewport.width, height });
    measured = await contentHeight(page);
  }
  await expectNoSidewaysScroll(page);
  await page.screenshot({ path: screenshotPath(name), animations: 'disabled' });
  await page.setViewportSize(viewport);
}

async function download(page: Page, query: ExportQuery): Promise<SavedDownload> {
  await page.getByLabel(START_DATE_LABEL, { exact: true }).fill(query.start_date);
  await page.getByLabel(END_DATE_LABEL, { exact: true }).fill(query.end_date);
  if (query.format !== ExportFormat.Ndjson) {
    await page.getByRole('combobox', { name: FORMAT_LABEL }).click();
    await page.getByRole('option', { name: formatLabel(query.format) }).click();
  }
  const pending = page.waitForEvent('download');
  await page.getByRole('button', { name: DOWNLOAD_LABEL, exact: true }).click();
  const file = await pending;
  const target = test.info().outputPath(file.suggestedFilename());
  await file.saveAs(target);
  return { filename: file.suggestedFilename(), contents: await readFile(target, 'utf8') };
}

test.use({ timezoneId: 'UTC', locale: 'en-US' });

test.describe('Organization audit export', () => {
  test('adds a webhook, keeps its secret on edit, disables and deletes it', async ({ page }) => {
    const server = await openOrganization(page, {
      destinations: ENTERPRISE_EMPTY_DESTINATIONS,
      retention: FOREVER_RETENTION,
    });
    await expect(page.getByText(NO_DESTINATIONS)).toBeVisible();

    await page.getByRole('button', { name: ADD_DESTINATION_LABEL }).click();
    const form = page.getByRole('dialog', { name: NEW_DESTINATION_TITLE });
    await form.getByRole('textbox', { name: NAME_LABEL }).fill(WEBHOOK_NAME);
    await form.getByRole('textbox', { name: URL_LABEL }).fill(PLAIN_URL);
    await expect(form.getByRole('status')).toHaveText(UNENCRYPTED_HTTP_WARNING);
    await form.getByRole('textbox', { name: URL_LABEL }).fill(SECURE_URL);
    await expect(form.getByText(UNENCRYPTED_HTTP_WARNING)).toHaveCount(0);
    await form.getByRole('button', { name: GENERATE_SECRET_LABEL }).click();
    const secret = form.getByRole('textbox', { name: secretLabel(SecretName.SigningSecret) });
    await expect(secret).not.toHaveValue('');
    const signingSecret = await secret.inputValue();
    await form.getByRole('button', { name: ADD_DESTINATION_LABEL }).click();

    await expect(form).toBeHidden();
    expect(server.created).toEqual([
      {
        name: WEBHOOK_NAME,
        enabled: true,
        settings: { kind: AuditKind.Webhook, url: SECURE_URL },
        secrets: { signing_secret: signingSecret },
      },
    ]);
    const added = row(page, WEBHOOK_NAME);
    await expect(added.getByText(healthLabel(DeliveryHealth.Healthy))).toBeVisible();
    await expect(added.getByText(SECURE_URL)).toBeVisible();

    await added.getByRole('button', { name: `${EDIT_LABEL} ${WEBHOOK_NAME}` }).click();
    const edit = page.getByRole('dialog', { name: EDIT_DESTINATION_TITLE });
    const savedSecret = edit.getByRole('textbox', { name: secretLabel(SecretName.SigningSecret) });
    await expect(savedSecret).toHaveValue('');
    await expect(savedSecret).toHaveAttribute('placeholder', SECRET_SAVED_PLACEHOLDER);
    await edit.getByRole('textbox', { name: NAME_LABEL }).fill(RENAMED_WEBHOOK);
    await edit.getByRole('button', { name: SAVE_DESTINATION_LABEL }).click();

    await expect(edit).toBeHidden();
    expect(server.updates).toEqual([{ name: RENAMED_WEBHOOK }]);
    expect(server.destinations.destinations.map((destination) => destination.secrets)).toEqual([
      [SecretName.SigningSecret],
    ]);

    const renamed = row(page, RENAMED_WEBHOOK);
    const enabled = renamed.getByRole('switch', { name: enabledLabel(RENAMED_WEBHOOK) });
    await expect(enabled).toHaveAttribute('aria-checked', 'true');
    await enabled.click();
    await expect(enabled).toHaveAttribute('aria-checked', 'false');
    await expect(renamed.getByText(healthLabel(DeliveryHealth.Disabled))).toBeVisible();
    expect(server.updates.at(-1)).toEqual({ enabled: false });

    await renamed.getByRole('button', { name: `${DELETE_LABEL} ${RENAMED_WEBHOOK}` }).click();
    const confirm = page.getByRole('dialog', { name: DELETE_TITLE });
    await expect(confirm.getByText(deleteMessage(RENAMED_WEBHOOK))).toBeVisible();
    await confirm.getByRole('button', { name: DELETE_CONFIRM_LABEL }).click();

    await expect(confirm).toBeHidden();
    expect(server.deletions).toEqual([NEW_DESTINATION_ID]);
    await expect(row(page, RENAMED_WEBHOOK)).toHaveCount(0);
    await expect(page.getByText(NO_DESTINATIONS)).toBeVisible();
  });

  test('sends test events and says whether each one arrived', async ({ page }) => {
    const server = await openOrganization(page, {
      destinations: ENTERPRISE_DESTINATIONS,
      retention: WAITING_RETENTION,
      results: { [FAILING_WEBHOOK.id]: FAILED_TEST },
    });

    const archive = row(page, HEALTHY_S3.name);
    await archive.getByRole('button', { name: testLabel(HEALTHY_S3.name) }).click();
    await expect(archive.getByRole('status')).toHaveText(testResultMessage(DELIVERED_TEST));

    const failing = row(page, FAILING_WEBHOOK.name);
    await failing.getByRole('button', { name: testLabel(FAILING_WEBHOOK.name) }).click();
    await expect(failing.getByRole('status')).toHaveText(testResultMessage(FAILED_TEST));
    await expect(archive.getByRole('status')).toHaveCount(0);

    expect(server.tests).toEqual([HEALTHY_S3.id, FAILING_WEBHOOK.id]);
  });

  test('sets a retention period, explains what it waits for, then keeps events forever', async ({
    page,
  }) => {
    const server = await openOrganization(page, {
      destinations: ENTERPRISE_DESTINATIONS,
      retention: FOREVER_RETENTION,
    });
    const retention = retentionCard(page);
    await expect(retention.getByText(KEPT_FOREVER)).toBeVisible();

    const forever = retention.getByRole('checkbox', { name: KEEP_FOREVER_LABEL });
    await expect(forever).toBeChecked();
    await forever.click();
    await retention
      .getByRole('textbox', { name: RETENTION_DAYS_LABEL })
      .fill(String(RETENTION_DAYS));
    await retention.getByRole('button', { name: SAVE_RETENTION_LABEL }).click();

    await expect(retention.getByRole('status')).toHaveText(RETENTION_SAVED);
    expect(server.retentions).toEqual([{ days: RETENTION_DAYS }]);
    await expect(retention.getByText(retentionText(RETENTION_DAYS))).toBeVisible();
    await expect(retention.getByRole('heading', { name: HOLDERS_LABEL })).toBeVisible();
    const holder = retention.getByRole('listitem');
    await expect(holder).toHaveCount(1);
    await expect(holder).toContainText(FAILING_WEBHOOK.name);
    await expect(holder).toContainText(waitMessage(RetentionWait.Failing));

    await forever.click();
    await retention.getByRole('button', { name: SAVE_RETENTION_LABEL }).click();

    await expect.poll(() => server.retentions).toEqual([{ days: RETENTION_DAYS }, { days: null }]);
    await expect(retention.getByText(KEPT_FOREVER)).toBeVisible();
    await expect(retention.getByRole('heading', { name: HOLDERS_LABEL })).toHaveCount(0);
  });

  test('downloads a date range under the filename the server gives', async ({ page }) => {
    const server = await openOrganization(page, {
      destinations: ENTERPRISE_DESTINATIONS,
      retention: WAITING_RETENTION,
    });

    const saved = await download(page, EXPORT_QUERY);

    expect(server.exports).toHaveLength(1);
    const [request] = server.exports;
    expect(request?.query).toEqual(EXPORT_QUERY);
    const organizationId = request?.organizationId ?? null;
    expect(organizationId).not.toBeNull();
    const filename = exportFilename(organizationScope(organizationId ?? ''), EXPORT_QUERY);
    expect(saved).toEqual({ filename, contents: EXPORT_BODY });
    await expect(page.getByRole('status').filter({ hasText: DOWNLOADED_LABEL })).toHaveText(
      `${DOWNLOADED_LABEL} ${filename}`
    );
  });

  test('offers the upgrade on Community and changes nothing', async ({ page }) => {
    await openOrganization(page, {
      destinations: COMMUNITY_DESTINATIONS,
      retention: COMMUNITY_RETENTION,
    });

    await expect(page.getByRole('note').filter({ hasText: UPGRADE_NOTICE })).toBeVisible();
    await expect(page.getByText(NO_DESTINATIONS_READ_ONLY)).toBeVisible();
    await expect(page.getByRole('button', { name: ADD_DESTINATION_LABEL })).toHaveCount(0);
    await expect(page.getByRole('button', { name: SAVE_RETENTION_LABEL })).toHaveCount(0);
    await expect(page.getByRole('button', { name: DOWNLOAD_LABEL, exact: true })).toBeDisabled();
    await expect(retentionCard(page).getByText(READ_ONLY_UNLICENSED)).toBeVisible();
    await expect(page.getByText(DOWNLOAD_READ_ONLY)).toBeVisible();
  });

  test('says a retention period saved before the license was removed resumes later', async ({
    page,
  }) => {
    await openOrganization(page, {
      destinations: COMMUNITY_DESTINATIONS,
      retention: COMMUNITY_SAVED_RETENTION,
    });

    const retention = retentionCard(page);
    await expect(
      retention.getByText(retentionSummary(COMMUNITY_SAVED_RETENTION.days, false))
    ).toBeVisible();
    await expect(retention.getByText(KEPT_FOREVER, { exact: true })).toHaveCount(0);
    await expect(retention.getByText(READ_ONLY_UNLICENSED)).toBeVisible();
  });

  test('pauses streaming on a lapsed license and says why events are held', async ({ page }) => {
    await openOrganization(page, {
      destinations: LAPSED_DESTINATIONS,
      retention: LAPSED_RETENTION,
    });

    await expect(page.getByRole('status').filter({ hasText: LAPSED_NOTICE })).toBeVisible();
    const paused = row(page, PAUSED_SYSLOG.name);
    await expect(paused.getByText(healthLabel(DeliveryHealth.Paused))).toBeVisible();
    await expect(paused.getByRole('button', { name: testLabel(PAUSED_SYSLOG.name) })).toHaveCount(
      0
    );
    const holders = retentionCard(page).getByRole('listitem');
    await expect(holders).toHaveCount(LAPSED_DESTINATIONS.destinations.length);
    for (const destination of LAPSED_DESTINATIONS.destinations) {
      await expect(holders.filter({ hasText: destination.name })).toContainText(
        waitMessage(RetentionWait.Paused)
      );
    }
    await expect(page.getByRole('button', { name: ADD_DESTINATION_LABEL })).toHaveCount(0);
    await expect(page.getByRole('button', { name: SAVE_RETENTION_LABEL })).toHaveCount(0);
    await expect(page.getByRole('button', { name: DOWNLOAD_LABEL, exact: true })).toBeDisabled();
  });

  test('warns a lapsed Team license that downloads stopped', async ({ page }) => {
    await openOrganization(page, {
      destinations: LAPSED_TEAM_DESTINATIONS,
      retention: LAPSED_TEAM_RETENTION,
    });

    await expect(page.getByRole('status').filter({ hasText: LAPSED_TEAM_NOTICE })).toBeVisible();
    await expect(page.getByText(RETENTION_STILL_APPLIED, { exact: false })).toHaveCount(0);
    await expect(page.getByText(EDITION_NOTICE)).toHaveCount(0);
    await expect(page.getByRole('button', { name: DOWNLOAD_LABEL, exact: true })).toBeDisabled();
    await expect(page.getByText(DOWNLOAD_READ_ONLY)).toBeVisible();
    await expect(retentionCard(page).getByText(READ_ONLY_EDITION)).toBeVisible();
  });

  test('asks for the license key again when the stored one cannot be verified', async ({
    page,
  }) => {
    await openOrganization(page, {
      destinations: UNVERIFIED_DESTINATIONS,
      retention: UNVERIFIED_RETENTION,
    });

    const warning = page.getByRole('status').filter({ hasText: UNVERIFIED_NOTICE });
    await expect(warning.getByRole('link', { name: PASTE_KEY_LINK_LABEL })).toBeVisible();
    await expect(page.getByRole('link', { name: RENEW_LINK_LABEL })).toHaveCount(0);
    await expect(page.getByRole('button', { name: DOWNLOAD_LABEL, exact: true })).toBeDisabled();
    await expect(page.getByText(DOWNLOAD_UNVERIFIED)).toBeVisible();
    await expect(page.getByText(DOWNLOAD_READ_ONLY)).toHaveCount(0);
    await expect(retentionCard(page).getByText(READ_ONLY_UNVERIFIED)).toBeVisible();
  });

  test('marks counts the server stopped at as lower bounds', async ({ page }) => {
    await openOrganization(page, {
      destinations: CAPPED_DESTINATIONS,
      retention: CAPPED_RETENTION,
    });

    const backlog = CAPPED_WEBHOOK.delivery;
    await expect(
      row(page, CAPPED_WEBHOOK.name).getByText(backlogText(backlog.backlog, backlog.backlog_capped))
    ).toBeVisible();
    const held = countText(CAPPED_RETENTION.held, CAPPED_RETENTION.held_capped);
    await expect(retentionCard(page).getByText(held, { exact: true })).toHaveCount(2);
  });

  test('lets Team download but keeps streaming behind the upgrade', async ({ page }) => {
    const server = await openOrganization(page, {
      destinations: TEAM_DESTINATIONS,
      retention: TEAM_RETENTION,
    });

    await expect(page.getByRole('note').filter({ hasText: EDITION_NOTICE })).toBeVisible();
    await expect(page.getByRole('button', { name: ADD_DESTINATION_LABEL })).toHaveCount(0);
    await expect(page.getByRole('button', { name: SAVE_RETENTION_LABEL })).toHaveCount(0);
    await expect(retentionCard(page).getByText(READ_ONLY_EDITION)).toBeVisible();

    const saved = await download(page, EXPORT_QUERY);
    expect(server.exports.map((request) => request.query)).toEqual([EXPORT_QUERY]);
    expect(saved.contents).toBe(EXPORT_BODY);
  });

  test('is linked from the audit log', async ({ page }) => {
    await signIn(page);
    await serveAudit(page, ORGANIZATION_API, {
      destinations: ENTERPRISE_DESTINATIONS,
      retention: WAITING_RETENTION,
    });
    await routeApi(page, AUDIT_LOGS_API, (route) =>
      route.fulfill({ status: 200, json: { logs: [], total: 0 } })
    );
    await page.goto(AUDIT_LOG_PATH, { waitUntil: 'domcontentloaded' });

    await page.getByRole('link', { name: AUDIT_LOG_LINK }).click();

    await expect(page).toHaveURL(new RegExp(`${ORGANIZATION_PATH}$`));
    await expect(page.getByRole('heading', { name: SECTION_TITLE, level: 2 })).toBeVisible();
    await expect(page.getByText(ORGANIZATION_DESCRIPTION)).toBeVisible();
  });
});

test.describe('Server audit export', () => {
  test('streams server-wide events and downloads them as CSV', async ({ page }) => {
    const server = await openServer(page, {
      destinations: SERVER_DESTINATIONS,
      retention: SERVER_RETENTION,
    });

    await expect(page.getByText(SERVER_DESCRIPTION)).toBeVisible();
    await expect(row(page, SERVER_SYSLOG.name)).toBeVisible();
    await expect(retentionCard(page).getByText(retentionText(SERVER_RETENTION.days))).toBeVisible();

    const saved = await download(page, CSV_QUERY);

    expect(server.exports).toEqual([{ organizationId: null, query: CSV_QUERY }]);
    expect(saved).toEqual({
      filename: exportFilename(SERVER_SCOPE, CSV_QUERY),
      contents: EXPORT_BODY,
    });
  });
});

test.describe('Audit export layout', () => {
  for (const viewport of WIDTHS) {
    for (const scheme of SCHEMES) {
      const suffix = `${scheme}-${viewport.width}`;

      test(`lays the organization section out at ${viewport.width}px in ${scheme}`, async ({
        page,
      }) => {
        await page.setViewportSize(viewport);
        await openOrganization(
          page,
          { destinations: ENTERPRISE_DESTINATIONS, retention: WAITING_RETENTION },
          { scheme }
        );
        await expect(row(page, FAILING_WEBHOOK.name)).toBeVisible();
        await shoot(page, `organization-${suffix}`);

        await page.getByRole('button', { name: ADD_DESTINATION_LABEL }).click();
        const form = page.getByRole('dialog', { name: NEW_DESTINATION_TITLE });
        await form.getByRole('textbox', { name: NAME_LABEL }).fill(WEBHOOK_NAME);
        await form.getByRole('textbox', { name: URL_LABEL }).fill(PLAIN_URL);
        await expect(form.getByRole('status')).toHaveText(UNENCRYPTED_HTTP_WARNING);
        await shoot(page, `destination-form-${suffix}`);
      });

      test(`lays the server section out at ${viewport.width}px in ${scheme}`, async ({ page }) => {
        await page.setViewportSize(viewport);
        await openServer(
          page,
          { destinations: SERVER_DESTINATIONS, retention: SERVER_RETENTION },
          { scheme }
        );
        await expect(row(page, SERVER_SYSLOG.name)).toBeVisible();
        await shoot(page, `server-${suffix}`);
      });
    }
  }

  const states: readonly (readonly [string, AuditSetup])[] = [
    ['community', { destinations: COMMUNITY_DESTINATIONS, retention: COMMUNITY_RETENTION }],
    [
      'community-saved',
      { destinations: COMMUNITY_DESTINATIONS, retention: COMMUNITY_SAVED_RETENTION },
    ],
    ['lapsed', { destinations: LAPSED_DESTINATIONS, retention: LAPSED_RETENTION }],
    ['lapsed-team', { destinations: LAPSED_TEAM_DESTINATIONS, retention: LAPSED_TEAM_RETENTION }],
    ['team', { destinations: TEAM_DESTINATIONS, retention: TEAM_RETENTION }],
    ['unverified', { destinations: UNVERIFIED_DESTINATIONS, retention: UNVERIFIED_RETENTION }],
    ['capped', { destinations: CAPPED_DESTINATIONS, retention: CAPPED_RETENTION }],
  ];

  for (const viewport of WIDTHS) {
    for (const scheme of SCHEMES) {
      test(`lays the license states out at ${viewport.width}px in ${scheme}`, async ({ page }) => {
        await page.setViewportSize(viewport);
        await signIn(page, { scheme });
        for (const [name, setup] of states) {
          await page.unroute(ORGANIZATION_API);
          await serveAudit(page, ORGANIZATION_API, setup);
          await page.goto(ORGANIZATION_PATH, { waitUntil: 'domcontentloaded' });
          await expect(page.getByRole('heading', { name: SECTION_TITLE, level: 2 })).toBeVisible();
          await shoot(page, `${name}-${scheme}-${viewport.width}`);
        }
      });
    }
  }
});
