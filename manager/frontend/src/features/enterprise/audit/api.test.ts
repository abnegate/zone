import { afterAll, afterEach, describe, expect, it, spyOn } from 'bun:test';
import { ApiError } from '../../../api/ApiError';
import { session } from '../../../api/session';
import { AuditRefusal } from './AuditRefusal';
import { auditApi } from './api';
import {
  ENTERPRISE_DESTINATIONS,
  EXPORT_FILENAME,
  EXPORT_QUERY,
  FAILED_TEST,
  FAILING_WEBHOOK,
  ORGANIZATION_ID,
  ORGANIZATION_SCOPE,
  SERVER_DESTINATIONS,
  SERVER_EXPORT_FILENAME,
  WAITING_RETENTION,
} from './fixtures';
import { AuditCode, AuditKind, ExportFormat } from './schemas';
import { organizationScope, SERVER_SCOPE } from './scope';

const fetchSpy = spyOn(globalThis, 'fetch');

const ORGANIZATION_URL = `/api/organizations/${ORGANIZATION_ID}`;

function respond(body: unknown, status = 200) {
  return fetchSpy.mockImplementation(async () => Response.json(body, { status }));
}

function respondWith(response: () => Response) {
  return fetchSpy.mockImplementation(async () => response());
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

describe('auditApi.destinations', () => {
  it('reads the organization destinations with the session token', async () => {
    session.setAccessToken('token-1');
    const request = respond(ENTERPRISE_DESTINATIONS);

    expect(await auditApi.destinations(ORGANIZATION_SCOPE)).toEqual(ENTERPRISE_DESTINATIONS);
    expect(sent(request)).toEqual({
      url: `${ORGANIZATION_URL}/audit-destinations`,
      method: 'GET',
      body: undefined,
    });
    expect(new Headers(request.mock.calls[0][1]?.headers).get('Authorization')).toBe(
      'Bearer token-1'
    );
  });

  it('reads the server destinations from the server routes', async () => {
    const request = respond(SERVER_DESTINATIONS);

    expect(await auditApi.destinations(SERVER_SCOPE)).toEqual(SERVER_DESTINATIONS);
    expect(sent(request).url).toBe('/api/audit-destinations');
  });

  it('escapes the organization id', async () => {
    const request = respond(ENTERPRISE_DESTINATIONS);

    await auditApi.destinations(organizationScope('a/b'));

    expect(sent(request).url).toBe('/api/organizations/a%2Fb/audit-destinations');
  });

  it('rejects a response outside the contract', async () => {
    respond({ ...ENTERPRISE_DESTINATIONS, standing: {} });

    expect(await failure(auditApi.destinations(ORGANIZATION_SCOPE))).toBeInstanceOf(Error);
  });

  it('throws a plain ApiError for a failure without an audit code', async () => {
    respond({ error: 'Organization not found' }, 404);

    const error = await failure(auditApi.destinations(ORGANIZATION_SCOPE));

    expect(error).toBeInstanceOf(ApiError);
    expect(error).not.toBeInstanceOf(AuditRefusal);
    expect((error as ApiError).message).toBe(
      'Failed to load the audit destinations: Organization not found'
    );
  });
});

describe('auditApi destination changes', () => {
  it('reads one destination', async () => {
    const request = respond(FAILING_WEBHOOK);

    expect(await auditApi.destination(SERVER_SCOPE, FAILING_WEBHOOK.id)).toEqual(FAILING_WEBHOOK);
    expect(sent(request).url).toBe(`/api/audit-destinations/${FAILING_WEBHOOK.id}`);
  });

  it('posts a new destination with its secrets', async () => {
    const request = respond(FAILING_WEBHOOK, 201);
    const creation = {
      name: 'Splunk HEC',
      settings: { kind: AuditKind.Webhook, url: 'https://siem.acme.com/zone/audit' },
      secrets: { signing_secret: 'whsec-1' },
    } as const;

    expect(await auditApi.createDestination(ORGANIZATION_SCOPE, creation)).toEqual(FAILING_WEBHOOK);
    const { url, method, body } = sent(request);
    expect({ url, method }).toEqual({
      url: `${ORGANIZATION_URL}/audit-destinations`,
      method: 'POST',
    });
    expect(JSON.parse(String(body))).toEqual(creation);
  });

  it('refuses to send a request outside the contract', async () => {
    const request = respond(FAILING_WEBHOOK);

    await failure(
      auditApi.createDestination(ORGANIZATION_SCOPE, {
        name: '',
        settings: { kind: AuditKind.Webhook, url: 'https://siem.acme.com' },
      })
    );

    expect(request).not.toHaveBeenCalled();
  });

  it('patches only the keys it is given', async () => {
    const request = respond(FAILING_WEBHOOK);

    await auditApi.updateDestination(ORGANIZATION_SCOPE, FAILING_WEBHOOK.id, { enabled: false });

    expect(sent(request)).toEqual({
      url: `${ORGANIZATION_URL}/audit-destinations/${FAILING_WEBHOOK.id}`,
      method: 'PATCH',
      body: JSON.stringify({ enabled: false }),
    });
  });

  it('deletes a destination without reading a body', async () => {
    const request = respondWith(() => new Response(null, { status: 204 }));

    await auditApi.deleteDestination(SERVER_SCOPE, FAILING_WEBHOOK.id);

    expect(sent(request)).toEqual({
      url: `/api/audit-destinations/${FAILING_WEBHOOK.id}`,
      method: 'DELETE',
      body: undefined,
    });
  });

  it('posts a test and reads the delivery result', async () => {
    const request = respond(FAILED_TEST);

    expect(await auditApi.testDestination(ORGANIZATION_SCOPE, FAILING_WEBHOOK.id)).toEqual(
      FAILED_TEST
    );
    expect(sent(request)).toEqual({
      url: `${ORGANIZATION_URL}/audit-destinations/${FAILING_WEBHOOK.id}/tests`,
      method: 'POST',
      body: undefined,
    });
  });

  it('turns a coded refusal into an AuditRefusal that keeps its status', async () => {
    respond({ error: 'A license is required', code: AuditCode.LicenseRequired }, 403);

    const error = await failure(
      auditApi.updateDestination(ORGANIZATION_SCOPE, FAILING_WEBHOOK.id, { name: 'SIEM' })
    );

    expect(error).toBeInstanceOf(AuditRefusal);
    expect((error as AuditRefusal).code).toBe(AuditCode.LicenseRequired);
    expect((error as AuditRefusal).status).toBe(403);
  });

  it('turns a duplicate into an AuditRefusal', async () => {
    respond({ error: 'Duplicate', code: AuditCode.DestinationDuplicate }, 409);

    const error = await failure(
      auditApi.createDestination(SERVER_SCOPE, {
        name: 'SIEM',
        settings: { kind: AuditKind.Webhook, url: 'https://siem.acme.com' },
      })
    );

    expect((error as AuditRefusal).code).toBe(AuditCode.DestinationDuplicate);
    expect((error as AuditRefusal).status).toBe(409);
  });
});

describe('auditApi retention', () => {
  it('reads the retention of a scope', async () => {
    const request = respond(WAITING_RETENTION);

    expect(await auditApi.retention(ORGANIZATION_SCOPE)).toEqual(WAITING_RETENTION);
    expect(sent(request).url).toBe(`${ORGANIZATION_URL}/audit-retention`);
  });

  it('puts the days, and null to keep events forever', async () => {
    const request = respond(WAITING_RETENTION);

    await auditApi.setRetention(SERVER_SCOPE, { days: null });

    expect(sent(request)).toEqual({
      url: '/api/audit-retention',
      method: 'PUT',
      body: JSON.stringify({ days: null }),
    });
  });
});

describe('auditApi.download', () => {
  function file(disposition: string | null): () => Response {
    return () =>
      new Response('{"id":"1"}\n', {
        headers: {
          'Content-Type': 'application/x-ndjson',
          ...(disposition === null ? {} : { 'Content-Disposition': disposition }),
        },
      });
  }

  it('asks for the format and range and names the file as the server does', async () => {
    const request = respondWith(file(`attachment; filename="${EXPORT_FILENAME}"`));

    const download = await auditApi.download(ORGANIZATION_SCOPE, EXPORT_QUERY);

    expect(sent(request)).toEqual({
      url: `${ORGANIZATION_URL}/audit-exports?format=ndjson&start_date=2026-11-01&end_date=2026-11-15`,
      method: 'GET',
      body: undefined,
    });
    expect(download.filename).toBe(EXPORT_FILENAME);
    expect(await download.blob.text()).toBe('{"id":"1"}\n');
  });

  it('downloads the server log from the server route', async () => {
    const request = respondWith(file(null));

    const download = await auditApi.download(SERVER_SCOPE, {
      ...EXPORT_QUERY,
      format: ExportFormat.Csv,
    });

    expect(String(sent(request).url)).toStartWith('/api/audit-exports?format=csv&');
    expect(download.filename).toBe(SERVER_EXPORT_FILENAME.replace('.ndjson', '.csv'));
  });

  it('refuses an unlicensed export with its code', async () => {
    respond({ error: 'License required', code: AuditCode.LicenseRequired }, 403);

    const error = await failure(auditApi.download(ORGANIZATION_SCOPE, EXPORT_QUERY));

    expect((error as AuditRefusal).code).toBe(AuditCode.LicenseRequired);
  });

  it('reports a refused audit row as a plain failure without a body', async () => {
    respondWith(() => new Response(null, { status: 500 }));

    const error = await failure(auditApi.download(ORGANIZATION_SCOPE, EXPORT_QUERY));

    expect(error).not.toBeInstanceOf(AuditRefusal);
    expect((error as ApiError).message).toBe('Failed to download the audit log: 500');
  });
});
