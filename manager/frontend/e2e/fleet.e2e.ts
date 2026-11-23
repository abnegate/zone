import type { Page } from '@playwright/test';
import {
  ACTIVE_POLICY,
  ACTIVE_RUNNERS,
  COMMUNITY_POLICY,
  COMMUNITY_RUNNERS,
  CORDONED_RUNNER,
  CURRENT_RUNNER,
  NO_RUNNER_LEFT,
  NOW,
  OFFLINE_RUNNER,
  ORGANIZATION_LABELS,
  OUTDATED_RUNNER,
  UNMATCHED_WORKSPACE_LABELS,
  UNVERIFIED_ORGANIZATION_LABELS,
  UNVERIFIED_POLICY,
  UNVERIFIED_RUNNERS,
  UNVERIFIED_WORKSPACE_LABELS,
  WORKSPACE_LABELS,
} from '../src/features/enterprise/fleet/fixtures';
import {
  CHATS_IGNORE_LABELS,
  CURRENT_BADGE,
  MINIMUM_LABEL,
  NO_RUNNER_LEFT_CONFIRM,
  NO_RUNNER_LEFT_TITLE,
  NOT_ENFORCED,
  noMatchWarning,
  ORGANIZATION_REQUIRED_LABEL,
  OUTDATED_BADGE,
  PLACEMENT_TITLE,
  PLACEMENT_UNVERIFIED,
  SAVE_LABELS,
  SAVE_MINIMUM,
  UNVERIFIED_NOTICE,
  UPGRADE_NOTICE,
  WORKSPACE_REQUIRED_LABEL,
} from '../src/features/enterprise/fleet/messages';
import { FleetCode, RunnerState } from '../src/features/enterprise/fleet/schemas';
import type {
  FleetPolicyResponse,
  FleetWorkCode,
  OrganizationRunnerLabelsResponse,
  RunnerResponse,
  RunnerState as RunnerStateValue,
  RunnersResponse,
  UpdateFleetPolicyRequest,
  UpdateRunnerLabelsRequest,
  UpdateRunnerRequest,
  WorkspaceRunnerLabelsResponse,
} from '../src/features/enterprise/fleet/types';
import {
  ACTIVE_ORGANIZATION,
  ACTIVE_WORKSPACE_INHERITED,
  UNVERIFIED_ORGANIZATION,
  UNVERIFIED_WORKSPACE,
} from '../src/features/enterprise/policy/fixtures';
import type {
  OrganizationPolicyResponse,
  WorkspacePolicyResponse,
} from '../src/features/enterprise/policy/types';
import { expect, test } from './fixtures';
import { routeWorkspaceRole, setupAdminAuth, setupCommonRoutes } from './layout-fixtures';
import { blockServiceWorker, routeApi } from './test-utils';

const RUNNERS_PATH = '/settings/server/runners';
const ORGANIZATION_POLICY_PATH = '/settings/organization/agent-policy';
const WORKSPACE_POLICY_PATH = '/settings/workspace/agent-policy';

const FLEET_POLICY_API = /\/api\/fleet-policy$/;
const RUNNERS_API = /\/api\/runners$/;
const RUNNER_API = /\/api\/runners\/([^/]+)$/;
const ORGANIZATION_POLICY_API = /\/api\/organizations\/[^/]+\/agent-policy$/;
const WORKSPACE_POLICY_API = /\/api\/organizations\/[^/]+\/workspaces\/[^/]+\/agent-policy$/;
const ORGANIZATION_LABELS_API = /\/api\/organizations\/[^/]+\/runner-labels$/;
const WORKSPACE_LABELS_API = /\/api\/organizations\/[^/]+\/workspaces\/[^/]+\/runner-labels$/;

const STATE_CODES: Readonly<Record<RunnerStateValue, FleetWorkCode | null>> = {
  [RunnerState.Active]: null,
  [RunnerState.Cordoned]: FleetCode.RunnerCordoned,
  [RunnerState.Revoked]: FleetCode.RunnerRevoked,
};

const DESKTOP = { width: 1280, height: 800 };
const PHONE = { width: 375, height: 812 };
const WIDTHS = [DESKTOP, PHONE];

function seenAsOfNow(runner: RunnerResponse): RunnerResponse {
  const ago = Date.parse(NOW) - Date.parse(runner.last_seen_at);
  return { ...runner, last_seen_at: new Date(Date.now() - ago).toISOString() };
}

interface FleetServer {
  policies: UpdateFleetPolicyRequest[];
  changes: { id: string; request: UpdateRunnerRequest }[];
  removals: string[];
}

interface Lockout {
  refuseUnconfirmed: boolean;
}

async function serveFleet(
  page: Page,
  policy: FleetPolicyResponse,
  listed: RunnersResponse,
  lockout: Lockout = { refuseUnconfirmed: false }
): Promise<FleetServer> {
  const server: FleetServer = { policies: [], changes: [], removals: [] };
  let currentPolicy = policy;
  let runners: RunnerResponse[] = listed.runners.map(seenAsOfNow);

  await routeApi(page, FLEET_POLICY_API, (route) => {
    if (route.request().method() === 'PATCH') {
      const request = route.request().postDataJSON() as UpdateFleetPolicyRequest;
      server.policies.push(request);
      if (lockout.refuseUnconfirmed && !request.confirm) {
        return route.fulfill({ status: 409, json: NO_RUNNER_LEFT });
      }
      currentPolicy = { ...currentPolicy, minimum_version: request.minimum_version };
    }
    return route.fulfill({ status: 200, json: currentPolicy });
  });

  await routeApi(page, RUNNERS_API, (route) =>
    route.fulfill({ status: 200, json: { ...listed, runners } })
  );

  await routeApi(page, RUNNER_API, (route) => {
    const id = RUNNER_API.exec(new URL(route.request().url()).pathname)?.[1] ?? '';
    if (route.request().method() === 'DELETE') {
      server.removals.push(id);
      runners = runners.filter((runner) => runner.id !== id);
      return route.fulfill({ status: 204, body: '' });
    }
    const request = route.request().postDataJSON() as UpdateRunnerRequest;
    server.changes.push({ id, request });
    if (lockout.refuseUnconfirmed && !request.confirm) {
      return route.fulfill({ status: 409, json: NO_RUNNER_LEFT });
    }
    const changed = runners.find((runner) => runner.id === id);
    if (changed === undefined) {
      return route.fulfill({ status: 404, json: { error: 'Runner not found' } });
    }
    const code = STATE_CODES[request.state];
    const updated: RunnerResponse = {
      ...changed,
      state: request.state,
      serving: code === null && changed.connected,
      code,
    };
    runners = runners.map((runner) => (runner.id === id ? updated : runner));
    return route.fulfill({ status: 200, json: updated });
  });

  return server;
}

interface Policies {
  organization: OrganizationPolicyResponse;
  workspace: WorkspacePolicyResponse;
}

const ACTIVE_POLICIES: Policies = {
  organization: ACTIVE_ORGANIZATION,
  workspace: ACTIVE_WORKSPACE_INHERITED,
};

async function serveLabels(
  page: Page,
  organization: OrganizationRunnerLabelsResponse,
  workspace: WorkspaceRunnerLabelsResponse,
  policies: Policies = ACTIVE_POLICIES
): Promise<UpdateRunnerLabelsRequest[]> {
  const saves: UpdateRunnerLabelsRequest[] = [];
  let organizationLabels = organization;
  await routeApi(page, ORGANIZATION_LABELS_API, (route) => {
    if (route.request().method() === 'PATCH') {
      const request = route.request().postDataJSON() as UpdateRunnerLabelsRequest;
      saves.push(request);
      organizationLabels = { ...organizationLabels, labels: request.labels };
    }
    return route.fulfill({ status: 200, json: organizationLabels });
  });
  await routeApi(page, WORKSPACE_LABELS_API, (route) =>
    route.fulfill({ status: 200, json: workspace })
  );
  await routeApi(page, ORGANIZATION_POLICY_API, (route) =>
    route.fulfill({ status: 200, json: policies.organization })
  );
  await routeApi(page, WORKSPACE_POLICY_API, (route) =>
    route.fulfill({ status: 200, json: policies.workspace })
  );
  return saves;
}

async function signIn(page: Page): Promise<void> {
  await blockServiceWorker(page.context());
  await setupCommonRoutes(page, false);
  await routeWorkspaceRole(page, 'admin');
  await page.goto('/login', { waitUntil: 'domcontentloaded' });
  await setupAdminAuth(page);
}

async function openRunners(page: Page): Promise<void> {
  await page.goto(RUNNERS_PATH, { waitUntil: 'domcontentloaded' });
  await expect(page.getByRole('heading', { name: 'Runners', level: 2 })).toBeVisible();
}

async function expectNoSidewaysScroll(page: Page): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth
      )
    )
    .toBeLessThanOrEqual(0);
}

async function shoot(page: Page, view: string): Promise<void> {
  await expectNoSidewaysScroll(page);
  const viewport = page.viewportSize() ?? DESKTOP;
  const height = await page.evaluate(() =>
    Math.max(...Array.from(document.querySelectorAll('*'), (element) => element.scrollHeight))
  );
  await page.setViewportSize({ width: viewport.width, height: Math.max(viewport.height, height) });
  await expectNoSidewaysScroll(page);
  await page.screenshot({
    path: test.info().outputPath(`${view}-${viewport.width}.png`),
    animations: 'disabled',
  });
  await page.setViewportSize(viewport);
}

function row(page: Page, runner: RunnerResponse) {
  return page.getByRole('row', { name: runner.name });
}

test.describe('Runners', () => {
  test('lists the fleet with the serving runner marked and outdated runners flagged', async ({
    page,
  }) => {
    await signIn(page);
    await serveFleet(page, ACTIVE_POLICY, ACTIVE_RUNNERS);
    await openRunners(page);

    await expect(row(page, CURRENT_RUNNER).getByText(CURRENT_BADGE)).toBeVisible();
    await expect(row(page, OUTDATED_RUNNER).getByText(OUTDATED_BADGE)).toBeVisible();
    await expect(row(page, CORDONED_RUNNER).getByText('Cordoned', { exact: true })).toBeVisible();
    await expect(page.getByRole('textbox', { name: MINIMUM_LABEL })).toHaveValue('1.4.0');
    await expect(row(page, CURRENT_RUNNER).getByRole('button', { name: /^Remove/ })).toHaveCount(0);
    await expect(row(page, OFFLINE_RUNNER).getByRole('button', { name: /^Remove/ })).toBeVisible();
  });

  test('cordons a runner after confirming', async ({ page }) => {
    await signIn(page);
    const server = await serveFleet(page, ACTIVE_POLICY, ACTIVE_RUNNERS);
    await openRunners(page);

    await page.getByRole('button', { name: `Cordon ${CURRENT_RUNNER.name}` }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toContainText('keeps serving chats');
    await dialog.getByRole('button', { name: 'Cordon runner' }).click();

    await expect(dialog).toHaveCount(0);
    expect(server.changes).toEqual([
      { id: CURRENT_RUNNER.id, request: { state: RunnerState.Cordoned } },
    ]);
    await expect(row(page, CURRENT_RUNNER).getByText('Cordoned', { exact: true })).toBeVisible();
  });

  test('asks again before a revoke leaves no runner, then sends it confirmed', async ({ page }) => {
    await signIn(page);
    const server = await serveFleet(page, ACTIVE_POLICY, ACTIVE_RUNNERS, {
      refuseUnconfirmed: true,
    });
    await openRunners(page);

    await page.getByRole('button', { name: `Revoke ${CURRENT_RUNNER.name}` }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toContainText('Its live task runs stop and its chats end');
    await dialog.getByRole('button', { name: 'Revoke runner' }).click();
    await expect(page.getByRole('heading', { name: NO_RUNNER_LEFT_TITLE })).toBeVisible();
    await page.getByRole('dialog').getByRole('button', { name: NO_RUNNER_LEFT_CONFIRM }).click();

    await expect(page.getByRole('dialog')).toHaveCount(0);
    expect(server.changes.map(({ request }) => request)).toEqual([
      { state: RunnerState.Revoked },
      { state: RunnerState.Revoked, confirm: true },
    ]);
  });

  test('confirms a minimum version that would leave no runner', async ({ page }) => {
    await signIn(page);
    const server = await serveFleet(page, ACTIVE_POLICY, ACTIVE_RUNNERS, {
      refuseUnconfirmed: true,
    });
    await openRunners(page);

    await page.getByRole('textbox', { name: MINIMUM_LABEL }).fill('9.0.0');
    await page.getByRole('button', { name: SAVE_MINIMUM }).click();
    await page.getByRole('dialog').getByRole('button', { name: NO_RUNNER_LEFT_CONFIRM }).click();

    await expect(page.getByRole('dialog')).toHaveCount(0);
    expect(server.policies).toEqual([
      { minimum_version: '9.0.0' },
      { minimum_version: '9.0.0', confirm: true },
    ]);
    await expect(page.getByRole('textbox', { name: MINIMUM_LABEL })).toHaveValue('9.0.0');
  });

  test('removes an offline runner', async ({ page }) => {
    await signIn(page);
    const server = await serveFleet(page, ACTIVE_POLICY, ACTIVE_RUNNERS);
    await openRunners(page);

    await page.getByRole('button', { name: `Remove ${OFFLINE_RUNNER.name}` }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Remove runner' }).click();

    await expect(row(page, OFFLINE_RUNNER)).toHaveCount(0);
    expect(server.removals).toEqual([OFFLINE_RUNNER.id]);
  });

  test('shows Community the fleet read-only and says nothing is enforced', async ({ page }) => {
    await signIn(page);
    await serveFleet(page, COMMUNITY_POLICY, COMMUNITY_RUNNERS);
    await openRunners(page);

    await expect(page.getByRole('note').filter({ hasText: UPGRADE_NOTICE })).toBeVisible();
    await expect(page.getByText(NOT_ENFORCED)).toBeVisible();
    await expect(page.getByRole('button', { name: /^(Cordon|Revoke|Activate) / })).toHaveCount(0);
    await expect(page.getByRole('textbox', { name: MINIMUM_LABEL })).toHaveCount(0);
  });

  test('says a key that no longer verifies keeps the fleet enforced and read-only', async ({
    page,
  }) => {
    await signIn(page);
    await serveFleet(page, UNVERIFIED_POLICY, UNVERIFIED_RUNNERS);
    await openRunners(page);

    await expect(page.getByRole('status').filter({ hasText: UNVERIFIED_NOTICE })).toBeVisible();
    await expect(page.getByText(UPGRADE_NOTICE)).toHaveCount(0);
    await expect(page.getByText(NOT_ENFORCED)).toHaveCount(0);
    await expect(page.getByRole('button', { name: /^(Cordon|Revoke|Activate) / })).toHaveCount(0);
    await expect(page.getByRole('textbox', { name: MINIMUM_LABEL })).toHaveCount(0);
    await shoot(page, 'runners-unverified');
  });

  for (const viewport of WIDTHS) {
    test(`lays the runners out at ${viewport.width}px`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await signIn(page);
      await serveFleet(page, ACTIVE_POLICY, ACTIVE_RUNNERS);
      await openRunners(page);

      await expect(row(page, CURRENT_RUNNER)).toBeVisible();
      await shoot(page, 'runners');
    });
  }
});

test.describe('Runner placement', () => {
  test('requires a label the organization picks from what runners carry', async ({ page }) => {
    await signIn(page);
    const saves = await serveLabels(page, ORGANIZATION_LABELS, WORKSPACE_LABELS);
    await page.goto(ORGANIZATION_POLICY_PATH, { waitUntil: 'domcontentloaded' });

    const card = page.getByRole('region', { name: PLACEMENT_TITLE });
    await expect(card.getByText(CHATS_IGNORE_LABELS)).toBeVisible();
    await card.getByRole('button', { name: 'Require gpu' }).click();
    await card.getByRole('button', { name: SAVE_LABELS }).click();

    await expect.poll(() => saves).toEqual([{ labels: ['gpu', 'region=eu'] }]);
    await expect(card.getByRole('button', { name: SAVE_LABELS })).toBeDisabled();
  });

  test('shows a workspace what it inherits and warns when no runner matches', async ({ page }) => {
    await signIn(page);
    await serveLabels(page, ORGANIZATION_LABELS, UNMATCHED_WORKSPACE_LABELS);
    await page.goto(WORKSPACE_POLICY_PATH, { waitUntil: 'domcontentloaded' });

    const card = page.getByRole('region', { name: PLACEMENT_TITLE });
    await expect(card.getByRole('list', { name: ORGANIZATION_REQUIRED_LABEL })).toHaveText(
      'region=eu'
    );
    await expect(
      card.getByRole('list', { name: ORGANIZATION_REQUIRED_LABEL }).getByRole('button')
    ).toHaveCount(0);
    await expect(card.getByRole('list', { name: WORKSPACE_REQUIRED_LABEL })).toContainText(
      'region=us'
    );
    await expect(
      card.getByText(noMatchWarning(UNMATCHED_WORKSPACE_LABELS.effective))
    ).toBeVisible();
  });

  test('shows the placement of a key that no longer verifies read-only', async ({ page }) => {
    await signIn(page);
    await serveLabels(page, UNVERIFIED_ORGANIZATION_LABELS, UNVERIFIED_WORKSPACE_LABELS, {
      organization: UNVERIFIED_ORGANIZATION,
      workspace: UNVERIFIED_WORKSPACE,
    });

    await page.goto(ORGANIZATION_POLICY_PATH, { waitUntil: 'domcontentloaded' });
    const organization = page.getByRole('region', { name: PLACEMENT_TITLE });
    await expect(organization.getByText(PLACEMENT_UNVERIFIED)).toBeVisible();
    await expect(organization.getByRole('button', { name: SAVE_LABELS })).toHaveCount(0);
    await expect(organization.getByRole('button', { name: /^Remove / })).toHaveCount(0);
    await organization.scrollIntoViewIfNeeded();
    await shoot(page, 'organization-placement-unverified');

    await page.goto(WORKSPACE_POLICY_PATH, { waitUntil: 'domcontentloaded' });
    const workspace = page.getByRole('region', { name: PLACEMENT_TITLE });
    await expect(workspace.getByText(PLACEMENT_UNVERIFIED)).toBeVisible();
    await expect(workspace.getByRole('list', { name: WORKSPACE_REQUIRED_LABEL })).toHaveText('gpu');
    await workspace.scrollIntoViewIfNeeded();
    await shoot(page, 'workspace-placement-unverified');
  });

  for (const viewport of WIDTHS) {
    test(`lays the placement cards out at ${viewport.width}px`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await signIn(page);
      await serveLabels(page, ORGANIZATION_LABELS, WORKSPACE_LABELS);

      await page.goto(ORGANIZATION_POLICY_PATH, { waitUntil: 'domcontentloaded' });
      const organization = page.getByRole('region', { name: PLACEMENT_TITLE });
      await expect(organization.getByRole('button', { name: 'Require gpu' })).toBeVisible();
      await organization.scrollIntoViewIfNeeded();
      await shoot(page, 'organization-placement');

      await page.goto(WORKSPACE_POLICY_PATH, { waitUntil: 'domcontentloaded' });
      const workspace = page.getByRole('region', { name: PLACEMENT_TITLE });
      await expect(
        workspace.getByRole('list', { name: ORGANIZATION_REQUIRED_LABEL })
      ).toBeVisible();
      await workspace.scrollIntoViewIfNeeded();
      await shoot(page, 'workspace-placement');
    });
  }
});
