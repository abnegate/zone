import { describe, expect, it } from 'bun:test';
import {
  changed,
  createRequest,
  draftErrors,
  draftOf,
  draftSecrets,
  emptyDraft,
  generateSecret,
  updateRequest,
  withProtocol,
} from './draft';
import { FAILING_WEBHOOK, HEALTHY_S3, PAUSED_SYSLOG } from './fixtures';
import {
  CLIENT_PAIR_REQUIRED,
  HOST_REQUIRED,
  NAME_REQUIRED,
  SECRET_REQUIRED,
  URL_REQUIRED,
} from './labels';
import { DestinationField, refusalMessage } from './messages';
import {
  AuditCode,
  AuditKind,
  CreateDestinationRequestSchema,
  SecretName,
  SyslogProtocol,
  UpdateDestinationRequestSchema,
} from './schemas';

const SECRET = 'whsec-0123456789';

describe('createRequest', () => {
  it('builds a webhook with its signing secret', () => {
    const request = createRequest({
      ...emptyDraft(),
      name: ' SIEM ',
      url: ' https://siem.example.com/audit ',
      secrets: { ...emptyDraft().secrets, signing_secret: SECRET },
    });

    expect(request).toEqual({
      name: 'SIEM',
      enabled: true,
      settings: { kind: AuditKind.Webhook, url: 'https://siem.example.com/audit' },
      secrets: { signing_secret: SECRET },
    });
    expect(CreateDestinationRequestSchema.parse(request)).toEqual(request);
  });

  it('sends a blank S3 endpoint as null for AWS', () => {
    const request = createRequest({
      ...emptyDraft(AuditKind.S3),
      name: 'Archive',
      bucket: 'acme-audit',
      region: 'eu-west-1',
      secrets: { ...emptyDraft().secrets, access_key_id: 'AKIA', secret_access_key: 'secret' },
    });

    expect(request.settings).toEqual({
      kind: AuditKind.S3,
      bucket: 'acme-audit',
      region: 'eu-west-1',
      endpoint: null,
      prefix: '',
      addressing: 'virtual_host',
    });
    expect(CreateDestinationRequestSchema.parse(request)).toEqual(request);
  });

  it('leaves certificates out of UDP syslog and secrets out when none are typed', () => {
    const request = createRequest({
      ...withProtocol(
        { ...emptyDraft(AuditKind.Syslog), name: 'Syslog', host: '10.0.0.5' },
        SyslogProtocol.Udp
      ),
      secrets: { ...emptyDraft().secrets, ca_certificate: 'PEM' },
    });

    expect(request.settings).toMatchObject({ protocol: SyslogProtocol.Udp, port: 514 });
    expect(request).not.toHaveProperty('secrets');
  });
});

describe('withProtocol', () => {
  it('moves a default port to the new protocol default and keeps a custom one', () => {
    expect(withProtocol(emptyDraft(AuditKind.Syslog), SyslogProtocol.Udp).port).toBe('514');
    expect(
      withProtocol({ ...emptyDraft(AuditKind.Syslog), port: '1514' }, SyslogProtocol.Udp).port
    ).toBe('1514');
  });
});

describe('draftSecrets', () => {
  it('offers certificates only to syslog over TLS', () => {
    expect(draftSecrets(emptyDraft())).toEqual([SecretName.SigningSecret]);
    expect(draftSecrets(emptyDraft(AuditKind.Syslog))).toHaveLength(3);
    expect(draftSecrets(withProtocol(emptyDraft(AuditKind.Syslog), SyslogProtocol.Udp))).toEqual(
      []
    );
  });
});

describe('updateRequest', () => {
  it('sends nothing when nothing changed, keeping saved secrets', () => {
    for (const destination of [FAILING_WEBHOOK, HEALTHY_S3, PAUSED_SYSLOG]) {
      expect(updateRequest(destination, draftOf(destination)), destination.name).toEqual({});
      expect(changed(updateRequest(destination, draftOf(destination)))).toBe(false);
    }
  });

  it('sends only what changed', () => {
    const draft = { ...draftOf(FAILING_WEBHOOK), enabled: false };
    expect(updateRequest(FAILING_WEBHOOK, draft)).toEqual({ enabled: false });

    const renamed = { ...draftOf(HEALTHY_S3), name: 'Cold archive', prefix: 'audit/' };
    const request = updateRequest(HEALTHY_S3, renamed);
    expect(request).toEqual({
      name: 'Cold archive',
      settings: { ...HEALTHY_S3.settings, prefix: 'audit/' },
    });
    expect(UpdateDestinationRequestSchema.parse(request)).toEqual(request);
  });

  it('sends a new secret without touching the settings', () => {
    const draft = {
      ...draftOf(FAILING_WEBHOOK),
      secrets: { ...emptyDraft().secrets, signing_secret: SECRET },
    };
    expect(updateRequest(FAILING_WEBHOOK, draft)).toEqual({ secrets: { signing_secret: SECRET } });
  });

  it('clears a removed certificate with an empty string', () => {
    const draft = { ...draftOf(PAUSED_SYSLOG), cleared: [SecretName.CaCertificate] };
    expect(updateRequest(PAUSED_SYSLOG, draft)).toEqual({ secrets: { ca_certificate: '' } });
  });

  it('clears saved certificates when syslog moves to UDP', () => {
    const request = updateRequest(
      PAUSED_SYSLOG,
      withProtocol(draftOf(PAUSED_SYSLOG), SyslogProtocol.Udp)
    );
    expect(request).toEqual({
      settings: { ...PAUSED_SYSLOG.settings, protocol: SyslogProtocol.Udp, port: 514 },
      secrets: { ca_certificate: '' },
    });
    expect(UpdateDestinationRequestSchema.parse(request)).toEqual(request);
  });
});

describe('draftErrors', () => {
  it('asks for the name, the target and every required secret of a new destination', () => {
    expect(draftErrors(emptyDraft())).toEqual({
      [DestinationField.Name]: NAME_REQUIRED,
      [DestinationField.Url]: URL_REQUIRED,
      [SecretName.SigningSecret]: SECRET_REQUIRED,
    });
    expect(draftErrors({ ...emptyDraft(AuditKind.S3), name: 'x' })).toMatchObject({
      [SecretName.AccessKeyId]: SECRET_REQUIRED,
      [SecretName.SecretAccessKey]: SECRET_REQUIRED,
    });
  });

  it('keeps saved secrets when editing', () => {
    expect(draftErrors(draftOf(FAILING_WEBHOOK), FAILING_WEBHOOK)).toEqual({});
    expect(draftErrors(draftOf(HEALTHY_S3), HEALTHY_S3)).toEqual({});
  });

  it('checks the syslog host and port', () => {
    const draft = { ...emptyDraft(AuditKind.Syslog), name: 'Syslog', port: '70000' };
    expect(draftErrors(draft)).toEqual({
      [DestinationField.Host]: HOST_REQUIRED,
      [DestinationField.Port]: refusalMessage(AuditCode.PortInvalid),
    });
    expect(draftErrors({ ...draft, host: 'siem', port: '6514.5' })).toEqual({
      [DestinationField.Port]: refusalMessage(AuditCode.PortInvalid),
    });
  });

  it('asks for the client certificate and key together over TLS only', () => {
    const draft = {
      ...emptyDraft(AuditKind.Syslog),
      name: 'Syslog',
      host: 'siem',
      secrets: { ...emptyDraft().secrets, client_certificate: 'PEM' },
    };
    expect(draftErrors(draft)).toEqual({ [SecretName.ClientKey]: CLIENT_PAIR_REQUIRED });
    expect(draftErrors(withProtocol(draft, SyslogProtocol.Udp))).toEqual({});
  });
});

describe('generateSecret', () => {
  it('makes a fresh 256-bit hex secret each time', () => {
    const first = generateSecret();
    expect(first).toMatch(/^[0-9a-f]{64}$/);
    expect(generateSecret()).not.toBe(first);
  });
});
