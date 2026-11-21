import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { chooseSelect } from '../../../../test/select';
import { AuditRefusal } from '../AuditRefusal';
import { FAILING_WEBHOOK, HEALTHY_S3, ORGANIZATION_SCOPE, PAUSED_SYSLOG } from '../fixtures';
import {
  ADD_DESTINATION_LABEL,
  BUCKET_LABEL,
  CANCEL_LABEL,
  CERTIFICATES_REMOVED,
  CLIENT_PAIR_REQUIRED,
  EDIT_DESTINATION_TITLE,
  ENDPOINT_LABEL,
  GENERATE_SECRET_LABEL,
  HOST_LABEL,
  KIND_LABEL,
  LICENSE_LINK_LABEL,
  NAME_LABEL,
  NAME_REQUIRED,
  PORT_LABEL,
  PROTOCOL_LABEL,
  REGION_LABEL,
  SAVE_DESTINATION_LABEL,
  SECRET_CLEAR_LABEL,
  SECRET_REMOVED,
  SECRET_REQUIRED,
  SECRET_SAVED_PLACEHOLDER,
  UNENCRYPTED_ENDPOINT_WARNING,
  UNENCRYPTED_HTTP_WARNING,
  UNENCRYPTED_UDP_WARNING,
  URL_LABEL,
  URL_REQUIRED,
} from '../labels';
import { refusalMessage } from '../messages';
import { AuditCode, AuditKind, SyslogProtocol } from '../schemas';
import type { DestinationResponse } from '../types';

const createDestination = mock();
const updateDestination = mock();

mock.module('../api', () => ({
  auditApi: {
    destinations: mock(),
    destination: mock(),
    createDestination,
    updateDestination,
    deleteDestination: mock(),
    testDestination: mock(),
    retention: mock(),
    setRetention: mock(),
    download: mock(),
  },
}));

let DestinationForm: typeof import('./DestinationForm').DestinationForm;

beforeAll(async () => {
  ({ DestinationForm } = await import('./DestinationForm'));
});

afterAll(() => {
  mock.restore();
});

const onClose = mock();

function renderForm(destination?: DestinationResponse) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <DestinationForm scope={ORGANIZATION_SCOPE} destination={destination} onClose={onClose} />
      </MemoryRouter>
    </QueryClientProvider>
  );
}

function type(label: string | RegExp, value: string) {
  fireEvent.change(screen.getByLabelText(label), { target: { value } });
}

function submit(label: string) {
  fireEvent.click(screen.getByRole('button', { name: label }));
}

describe('DestinationForm', () => {
  beforeEach(() => {
    createDestination.mockReset();
    updateDestination.mockReset();
    onClose.mockReset();
  });

  describe('adding', () => {
    it('adds a signed webhook with a generated secret', async () => {
      createDestination.mockResolvedValue(FAILING_WEBHOOK);
      renderForm();

      expect(screen.getByRole('dialog', { name: ADD_DESTINATION_LABEL })).toBeInTheDocument();
      type(NAME_LABEL, 'Splunk');
      type(URL_LABEL, 'https://siem.acme.com/zone/audit');
      fireEvent.click(screen.getByRole('button', { name: GENERATE_SECRET_LABEL }));
      const secret = (screen.getByLabelText('Signing secret') as HTMLInputElement).value;
      expect(secret).toMatch(/^[0-9a-f]{64}$/);
      submit(ADD_DESTINATION_LABEL);

      await waitFor(() => expect(onClose).toHaveBeenCalled());
      expect(createDestination).toHaveBeenCalledWith(ORGANIZATION_SCOPE, {
        name: 'Splunk',
        enabled: true,
        settings: { kind: AuditKind.Webhook, url: 'https://siem.acme.com/zone/audit' },
        secrets: { signing_secret: secret },
      });
    });

    it('asks for what is missing before sending anything', () => {
      renderForm();

      submit(ADD_DESTINATION_LABEL);

      expect(screen.getByText(NAME_REQUIRED)).toBeInTheDocument();
      expect(screen.getByText(URL_REQUIRED)).toBeInTheDocument();
      expect(screen.getByText(SECRET_REQUIRED)).toBeInTheDocument();
      expect(createDestination).not.toHaveBeenCalled();
    });

    it('warns that a plain http webhook is unencrypted', () => {
      renderForm();

      expect(screen.queryByText(UNENCRYPTED_HTTP_WARNING)).not.toBeInTheDocument();
      type(URL_LABEL, 'http://splunk:8088/services/collector');

      expect(screen.getByText(UNENCRYPTED_HTTP_WARNING)).toBeInTheDocument();
    });

    it('shows the S3 fields and warns about a plain http endpoint', async () => {
      createDestination.mockResolvedValue(HEALTHY_S3);
      renderForm();

      chooseSelect(KIND_LABEL, 'S3');
      expect(screen.queryByLabelText(URL_LABEL)).not.toBeInTheDocument();
      type(NAME_LABEL, 'MinIO');
      type(BUCKET_LABEL, 'audit');
      type(REGION_LABEL, 'us-east-1');
      type(ENDPOINT_LABEL, 'http://minio:9000');
      expect(screen.getByText(UNENCRYPTED_ENDPOINT_WARNING)).toBeInTheDocument();
      type('Access key ID', 'minio');
      type('Secret access key', 'minio-secret');
      submit(ADD_DESTINATION_LABEL);

      await waitFor(() => expect(createDestination).toHaveBeenCalled());
      expect(createDestination.mock.calls[0][1]).toEqual({
        name: 'MinIO',
        enabled: true,
        settings: {
          kind: AuditKind.S3,
          bucket: 'audit',
          region: 'us-east-1',
          endpoint: 'http://minio:9000',
          prefix: '',
          addressing: 'virtual_host',
        },
        secrets: { access_key_id: 'minio', secret_access_key: 'minio-secret' },
      });
    });

    it('offers certificates for syslog over TLS and warns about UDP instead', () => {
      renderForm();

      chooseSelect(KIND_LABEL, 'Syslog');
      expect(screen.getByLabelText(PORT_LABEL)).toHaveValue('6514');
      expect(screen.getByLabelText('CA certificate')).toBeInTheDocument();
      expect(screen.queryByText(UNENCRYPTED_UDP_WARNING)).not.toBeInTheDocument();

      chooseSelect(PROTOCOL_LABEL, 'UDP');

      expect(screen.getByLabelText(PORT_LABEL)).toHaveValue('514');
      expect(screen.getByText(UNENCRYPTED_UDP_WARNING)).toBeInTheDocument();
      expect(screen.queryByLabelText('CA certificate')).not.toBeInTheDocument();
      expect(screen.queryByText(CERTIFICATES_REMOVED)).not.toBeInTheDocument();
    });

    it('asks for the client certificate and key together', () => {
      renderForm();

      chooseSelect(KIND_LABEL, 'Syslog');
      type(NAME_LABEL, 'Graylog');
      type(HOST_LABEL, 'graylog.internal');
      type('Client certificate', '-----BEGIN CERTIFICATE-----');
      submit(ADD_DESTINATION_LABEL);

      expect(screen.getByText(CLIENT_PAIR_REQUIRED)).toBeInTheDocument();
      expect(createDestination).not.toHaveBeenCalled();
    });

    it('puts a refusal next to the field it is about', async () => {
      createDestination.mockRejectedValue(
        new AuditRefusal({ error: 'Duplicate', code: AuditCode.DestinationDuplicate }, 409)
      );
      renderForm();

      type(NAME_LABEL, 'Splunk');
      type(URL_LABEL, 'https://siem.acme.com');
      type('Signing secret', 'secret');
      submit(ADD_DESTINATION_LABEL);

      const name = screen.getByLabelText(NAME_LABEL);
      await waitFor(() => expect(name).toHaveAttribute('aria-invalid', 'true'));
      expect(screen.getByText(refusalMessage(AuditCode.DestinationDuplicate))).toBeInTheDocument();
      expect(onClose).not.toHaveBeenCalled();
    });

    it('shows the upgrade notice when the license no longer allows streaming', async () => {
      createDestination.mockRejectedValue(
        new AuditRefusal({ error: 'License', code: AuditCode.LicenseRequired }, 403)
      );
      renderForm();

      type(NAME_LABEL, 'Splunk');
      type(URL_LABEL, 'https://siem.acme.com');
      type('Signing secret', 'secret');
      submit(ADD_DESTINATION_LABEL);

      const notice = await screen.findByRole('note');
      expect(notice).toHaveTextContent(refusalMessage(AuditCode.LicenseRequired));
      expect(within(notice).getByRole('link', { name: LICENSE_LINK_LABEL })).toBeInTheDocument();
    });

    it('closes on cancel', () => {
      renderForm();

      fireEvent.click(screen.getByRole('button', { name: CANCEL_LABEL }));

      expect(onClose).toHaveBeenCalled();
    });
  });

  describe('editing', () => {
    it('keeps the type fixed and the saved secret write-only', () => {
      renderForm(FAILING_WEBHOOK);

      expect(screen.getByRole('dialog', { name: EDIT_DESTINATION_TITLE })).toBeInTheDocument();
      expect(screen.getByRole('combobox', { name: KIND_LABEL })).toBeDisabled();
      const secret = screen.getByLabelText('Signing secret');
      expect(secret).toHaveValue('');
      expect(secret).toHaveAttribute('placeholder', SECRET_SAVED_PLACEHOLDER);
      expect(screen.getByRole('button', { name: SAVE_DESTINATION_LABEL })).toBeDisabled();
    });

    it('sends only what changed', async () => {
      updateDestination.mockResolvedValue(FAILING_WEBHOOK);
      renderForm(FAILING_WEBHOOK);

      type(URL_LABEL, 'https://siem.acme.com/zone/v2');
      submit(SAVE_DESTINATION_LABEL);

      await waitFor(() => expect(onClose).toHaveBeenCalled());
      expect(updateDestination).toHaveBeenCalledWith(ORGANIZATION_SCOPE, FAILING_WEBHOOK.id, {
        settings: { kind: AuditKind.Webhook, url: 'https://siem.acme.com/zone/v2' },
      });
    });

    it('removes a saved certificate', async () => {
      updateDestination.mockResolvedValue(PAUSED_SYSLOG);
      renderForm(PAUSED_SYSLOG);

      expect(screen.getByLabelText('CA certificate')).toHaveAttribute(
        'placeholder',
        SECRET_SAVED_PLACEHOLDER
      );
      fireEvent.click(screen.getByRole('button', { name: SECRET_CLEAR_LABEL }));
      expect(screen.getByText(SECRET_REMOVED)).toBeInTheDocument();
      submit(SAVE_DESTINATION_LABEL);

      await waitFor(() =>
        expect(updateDestination).toHaveBeenCalledWith(ORGANIZATION_SCOPE, PAUSED_SYSLOG.id, {
          secrets: { ca_certificate: '' },
        })
      );
    });

    it('drops saved certificates when syslog moves to UDP, and says so first', async () => {
      updateDestination.mockResolvedValue(PAUSED_SYSLOG);
      renderForm(PAUSED_SYSLOG);

      expect(screen.queryByText(CERTIFICATES_REMOVED)).not.toBeInTheDocument();
      chooseSelect(PROTOCOL_LABEL, 'UDP');
      expect(screen.getByText(CERTIFICATES_REMOVED)).toBeInTheDocument();
      chooseSelect(PROTOCOL_LABEL, 'TCP with TLS');
      expect(screen.queryByText(CERTIFICATES_REMOVED)).not.toBeInTheDocument();
      chooseSelect(PROTOCOL_LABEL, 'UDP');
      submit(SAVE_DESTINATION_LABEL);

      await waitFor(() => expect(updateDestination).toHaveBeenCalled());
      expect(updateDestination.mock.calls[0][2]).toEqual({
        settings: { ...PAUSED_SYSLOG.settings, protocol: SyslogProtocol.Udp, port: 514 },
        secrets: { ca_certificate: '' },
      });
    });
  });
});
