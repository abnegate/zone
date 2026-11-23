import { afterAll, afterEach, describe, expect, it, spyOn } from 'bun:test';
import { ApiError } from '../../../api/ApiError';
import { session } from '../../../api/session';
import { fleetApi } from './api';
import { FleetRefusal } from './FleetRefusal';
import {
  ACTIVE_POLICY,
  ACTIVE_RUNNERS,
  CORDONED_RUNNER,
  CURRENT_RUNNER,
  NO_RUNNER_LEFT,
  ORGANIZATION_ID,
  ORGANIZATION_LABELS,
  WORKSPACE_ID,
  WORKSPACE_LABELS,
} from './fixtures';
import { organizationPlacement, workspacePlacement } from './placement';
import { FleetCode, RunnerState } from './schemas';

const fetchSpy = spyOn(globalThis, 'fetch');

const ORGANIZATION_LABELS_URL = `/api/organizations/${ORGANIZATION_ID}/runner-labels`;

const WORKSPACE_LABELS_URL = `/api/organizations/${ORGANIZATION_ID}/workspaces/${WORKSPACE_ID}/runner-labels`;

function respond(body: unknown, status = 200) {
  return fetchSpy.mockImplementation(async () => Response.json(body, { status }));
}

function sent(request: ReturnType<typeof respond>) {
  const [url, init] = request.mock.calls[0];
  return { url, method: init?.method, body: init?.body ?? undefined };
}

async function failure(action: Promise<unknown>): Promise<unknown> {
  try {
    await action;
  } catch (error) {
    return error;
  }
  throw new Error('expected the request to fail');
}

afterEach(() => {
  fetchSpy.mockReset();
  session.setAccessToken(null);
});

afterAll(() => {
  fetchSpy.mockRestore();
});

describe('fleetApi.policy', () => {
  it('reads the fleet policy with the session token', async () => {
    session.setAccessToken('token-1');
    const request = respond(ACTIVE_POLICY);

    expect(await fleetApi.policy()).toEqual(ACTIVE_POLICY);
    expect(sent(request)).toEqual({ url: '/api/fleet-policy', method: 'GET', body: undefined });
    expect(new Headers(request.mock.calls[0][1]?.headers).get('Authorization')).toBe(
      'Bearer token-1'
    );
  });

  it('sends a minimum, and null to clear it', async () => {
    const request = respond(ACTIVE_POLICY);

    await fleetApi.updatePolicy({ minimum_version: '1.4.0' });
    await fleetApi.updatePolicy({ minimum_version: null, confirm: true });

    expect(request.mock.calls.map(([, init]) => [init?.method, init?.body])).toEqual([
      ['PATCH', JSON.stringify({ minimum_version: '1.4.0' })],
      ['PATCH', JSON.stringify({ minimum_version: null, confirm: true })],
    ]);
  });

  it('turns the lockout refusal into a fleet refusal that asks for confirmation', async () => {
    respond(NO_RUNNER_LEFT, 409);

    const error = await failure(fleetApi.updatePolicy({ minimum_version: '9.0.0' }));

    expect(error).toBeInstanceOf(FleetRefusal);
    expect((error as FleetRefusal).code).toBe(FleetCode.NoRunnerLeft);
    expect((error as FleetRefusal).leavesNoRunner).toBe(true);
    expect((error as FleetRefusal).status).toBe(409);
  });

  it('keeps the server message of an invalid version', async () => {
    respond({ error: '"1.4" is not a valid Zone version.', code: FleetCode.VersionInvalid }, 400);

    const error = await failure(fleetApi.updatePolicy({ minimum_version: '1.4' }));

    expect(error).toBeInstanceOf(FleetRefusal);
    expect((error as FleetRefusal).message).toBe('"1.4" is not a valid Zone version.');
    expect((error as FleetRefusal).leavesNoRunner).toBe(false);
  });

  it('throws a plain ApiError for a refusal without a fleet code', async () => {
    respond({ error: 'Admin access required' }, 403);

    const error = await failure(fleetApi.policy());

    expect(error).toBeInstanceOf(ApiError);
    expect(error).not.toBeInstanceOf(FleetRefusal);
    expect((error as ApiError).message).toBe(
      'Failed to load the fleet policy: Admin access required'
    );
  });
});

describe('fleetApi.runners', () => {
  it('lists the runners', async () => {
    const request = respond(ACTIVE_RUNNERS);

    expect(await fleetApi.runners()).toEqual(ACTIVE_RUNNERS);
    expect(sent(request)).toEqual({ url: '/api/runners', method: 'GET', body: undefined });
  });

  it('rejects a runner in a state the console does not know', async () => {
    respond({ ...ACTIVE_RUNNERS, runners: [{ ...CURRENT_RUNNER, state: 'draining' }] });

    expect(await failure(fleetApi.runners())).toBeInstanceOf(Error);
  });

  it('changes a runner state under an escaped id', async () => {
    const request = respond(CORDONED_RUNNER);

    expect(await fleetApi.updateRunner('a/b', { state: RunnerState.Cordoned })).toEqual(
      CORDONED_RUNNER
    );
    expect(sent(request)).toEqual({
      url: '/api/runners/a%2Fb',
      method: 'PATCH',
      body: JSON.stringify({ state: RunnerState.Cordoned }),
    });
  });

  it('removes a runner and reads no body', async () => {
    const request = fetchSpy.mockImplementation(async () => new Response(null, { status: 204 }));

    await fleetApi.removeRunner(CURRENT_RUNNER.id);

    expect(sent(request)).toEqual({
      url: `/api/runners/${CURRENT_RUNNER.id}`,
      method: 'DELETE',
      body: undefined,
    });
  });

  it('refuses to remove a connected runner with its code', async () => {
    respond({ error: 'This runner is connected', code: FleetCode.RunnerOnline }, 409);

    const error = await failure(fleetApi.removeRunner(CURRENT_RUNNER.id));

    expect((error as FleetRefusal).code).toBe(FleetCode.RunnerOnline);
  });
});

describe('fleetApi.labels', () => {
  it('reads an organization requirement as its own labels', async () => {
    const request = respond(ORGANIZATION_LABELS);

    expect(await fleetApi.labels(organizationPlacement(ORGANIZATION_ID))).toEqual({
      entitled: true,
      enforced: true,
      edition: ORGANIZATION_LABELS.edition,
      status: ORGANIZATION_LABELS.status,
      editable: true,
      inherited: [],
      own: ['region=eu'],
      effective: ['region=eu'],
      available: ORGANIZATION_LABELS.available,
      matching: 2,
      updated_at: ORGANIZATION_LABELS.updated_at,
    });
    expect(sent(request).url).toBe(ORGANIZATION_LABELS_URL);
  });

  it('reads a workspace requirement with what it inherits', async () => {
    const request = respond(WORKSPACE_LABELS);

    const placement = await fleetApi.labels(workspacePlacement(ORGANIZATION_ID, WORKSPACE_ID));

    expect(placement.inherited).toEqual(['region=eu']);
    expect(placement.own).toEqual(['gpu']);
    expect(placement.effective).toEqual(['gpu', 'region=eu']);
    expect(sent(request).url).toBe(WORKSPACE_LABELS_URL);
  });

  it('replaces the labels of a scope', async () => {
    const request = respond(WORKSPACE_LABELS);

    await fleetApi.updateLabels(workspacePlacement(ORGANIZATION_ID, WORKSPACE_ID), {
      labels: ['gpu'],
    });

    expect(sent(request)).toEqual({
      url: WORKSPACE_LABELS_URL,
      method: 'PATCH',
      body: JSON.stringify({ labels: ['gpu'] }),
    });
  });

  it('turns an invalid label into a fleet refusal', async () => {
    respond({ error: '"GPU" is not a valid runner label.', code: FleetCode.LabelInvalid }, 400);

    const error = await failure(
      fleetApi.updateLabels(organizationPlacement(ORGANIZATION_ID), { labels: ['GPU'] })
    );

    expect((error as FleetRefusal).code).toBe(FleetCode.LabelInvalid);
  });
});
