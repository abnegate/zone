import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ApiError } from '../../../../api/ApiError';
import { AdministersServerProvider } from '../../components/AdministersServer';
import { AuditRefusal } from '../AuditRefusal';
import {
  COMMUNITY_DESTINATIONS,
  COMMUNITY_RETENTION,
  DELIVERED_TEST,
  DISABLED_UDP_SYSLOG,
  ENTERPRISE_DESTINATIONS,
  ENTERPRISE_RETENTION,
  FAILED_TEST,
  FAILING_WEBHOOK,
  FOREVER_RETENTION,
  HEALTHY_S3,
  LAPSED_DESTINATIONS,
  LAPSED_RETENTION,
  ORGANIZATION_SCOPE,
  PAUSED_SYSLOG,
  PLAIN_HTTP_WEBHOOK,
  SERVER_DESTINATIONS,
  SERVER_RETENTION,
  SERVER_SYSLOG,
  STALLED_DESTINATIONS,
  UNVERIFIED_DESTINATIONS,
  UNVERIFIED_RETENTION,
  WAITING_RETENTION,
} from '../fixtures';
import {
  ADD_DESTINATION_LABEL,
  ASK_FOR_KEY_LABEL,
  ASK_FOR_LICENSE_LABEL,
  ASK_FOR_RENEWAL_LABEL,
  DELETE_CONFIRM_LABEL,
  DESTINATIONS_TITLE,
  DOWNLOAD_LABEL,
  DOWNLOAD_READ_ONLY,
  DOWNLOAD_TITLE,
  DOWNLOAD_UNVERIFIED,
  EDIT_DESTINATION_TITLE,
  HOLDERS_LABEL,
  LAPSED_NOTICE,
  LICENSE_LINK_LABEL,
  LOADING_LABEL,
  NEVER_DELIVERED,
  NO_DESTINATIONS,
  NO_DESTINATIONS_READ_ONLY,
  ORGANIZATION_DESCRIPTION,
  PASTE_KEY_LINK_LABEL,
  READ_ONLY_LAPSED,
  READ_ONLY_UNLICENSED,
  READ_ONLY_UNVERIFIED,
  RENEW_LINK_LABEL,
  RETENTION_NOT_APPLIED,
  RETENTION_STILL_APPLIED,
  RETENTION_TITLE,
  SAVE_RETENTION_LABEL,
  SECTION_TITLE,
  SERVER_DESCRIPTION,
  UNENCRYPTED_LABEL,
  UNVERIFIED_NOTICE,
  UP_TO_DATE,
  UPGRADE_NOTICE,
} from '../labels';
import {
  backlogText,
  deleteMessage,
  deliveryError,
  enabledLabel,
  refusalMessage,
  stalledMessage,
  testLabel,
  testResultMessage,
  waitMessage,
} from '../messages';
import { AuditCode, RetentionWait } from '../schemas';
import { SERVER_SCOPE } from '../scope';
import type { AuditScope, DestinationsResponse, RetentionResponse } from '../types';

const destinations = mock();
const updateDestination = mock();
const deleteDestination = mock();
const testDestination = mock();
const retention = mock();

mock.module('../api', () => ({
  auditApi: {
    destinations,
    destination: mock(),
    createDestination: mock(),
    updateDestination,
    deleteDestination,
    testDestination,
    retention,
    setRetention: mock(),
    download: mock(),
  },
}));

let AuditExportSection: typeof import('./AuditExportSection').AuditExportSection;

beforeAll(async () => {
  ({ AuditExportSection } = await import('./AuditExportSection'));
});

afterAll(() => {
  mock.restore();
});

function renderSection(scope: AuditScope = ORGANIZATION_SCOPE, administersServer = true) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <AdministersServerProvider value={administersServer}>
          <AuditExportSection scope={scope} />
        </AdministersServerProvider>
      </MemoryRouter>
    </QueryClientProvider>
  );
}

async function loaded(
  list: DestinationsResponse,
  policy: RetentionResponse,
  scope: AuditScope = ORGANIZATION_SCOPE,
  administersServer = true
) {
  destinations.mockResolvedValue(list);
  retention.mockResolvedValue(policy);
  renderSection(scope, administersServer);
  await screen.findByRole('heading', { name: SECTION_TITLE });
}

function row(name: string): HTMLElement {
  return screen.getByRole('listitem', { name });
}

function card(title: string): HTMLElement {
  const heading = screen.getByRole('heading', { name: title });
  return heading.closest('.settings-card') as HTMLElement;
}

describe('AuditExportSection', () => {
  beforeEach(() => {
    destinations.mockReset();
    updateDestination.mockReset();
    deleteDestination.mockReset();
    testDestination.mockReset();
    retention.mockReset();
  });

  describe('states', () => {
    it('shows a loading state until both loads arrive', () => {
      destinations.mockImplementation(() => new Promise(() => {}));
      retention.mockImplementation(() => new Promise(() => {}));
      renderSection();

      expect(screen.getByText(LOADING_LABEL)).toBeInTheDocument();
    });

    it('offers a retry when the destinations fail to load', async () => {
      destinations
        .mockRejectedValueOnce(
          new ApiError('Failed to load the audit destinations: Not found', 404)
        )
        .mockResolvedValueOnce(ENTERPRISE_DESTINATIONS);
      retention.mockResolvedValue(ENTERPRISE_RETENTION);
      renderSection();

      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('Failed to load the audit destinations');
      fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));

      await screen.findByRole('listitem', { name: FAILING_WEBHOOK.name });
      expect(destinations).toHaveBeenCalledTimes(2);
    });

    it('shows Community the upgrade notice and everything read-only', async () => {
      await loaded(COMMUNITY_DESTINATIONS, COMMUNITY_RETENTION);

      const notice = screen.getByRole('note');
      expect(notice).toHaveTextContent(UPGRADE_NOTICE);
      expect(within(notice).getByRole('link', { name: LICENSE_LINK_LABEL })).toHaveAttribute(
        'href',
        '/settings/server/license'
      );
      expect(screen.getByText(ORGANIZATION_DESCRIPTION)).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: ADD_DESTINATION_LABEL })).not.toBeInTheDocument();
      expect(screen.getByText(NO_DESTINATIONS_READ_ONLY)).toBeInTheDocument();
      expect(screen.getByText(RETENTION_NOT_APPLIED)).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: SAVE_RETENTION_LABEL })).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: DOWNLOAD_LABEL })).toBeDisabled();
      expect(within(card(RETENTION_TITLE)).getByText(READ_ONLY_UNLICENSED)).toBeInTheDocument();
      expect(within(card(DOWNLOAD_TITLE)).getByText(DOWNLOAD_READ_ONLY)).toBeInTheDocument();
    });

    it('warns that a lapsed license pauses streaming and keeps the backlog, read-only', async () => {
      await loaded(LAPSED_DESTINATIONS, LAPSED_RETENTION);

      const warning = screen.getByRole('status', { name: '' });
      expect(warning).toHaveClass('alert-warning');
      expect(warning).toHaveTextContent(`${LAPSED_NOTICE} ${RETENTION_STILL_APPLIED}`);
      expect(within(warning).getByRole('link', { name: RENEW_LINK_LABEL })).toHaveAttribute(
        'href',
        '/settings/server/license'
      );
      expect(screen.queryByRole('note')).not.toBeInTheDocument();
      const syslog = row(PAUSED_SYSLOG.name);
      expect(
        within(syslog).getByText(backlogText(PAUSED_SYSLOG.delivery.backlog))
      ).toBeInTheDocument();
      expect(within(syslog).queryByRole('button', { name: /^Edit/ })).not.toBeInTheDocument();
      expect(
        within(syslog).getByRole('button', { name: `Delete ${PAUSED_SYSLOG.name}` })
      ).toBeEnabled();
      expect(
        within(syslog).getByRole('switch', { name: enabledLabel(PAUSED_SYSLOG.name) })
      ).toBeEnabled();
      expect(screen.getAllByText(waitMessage(RetentionWait.Paused))).toHaveLength(2);
      expect(within(card(RETENTION_TITLE)).getByText(READ_ONLY_LAPSED)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: DOWNLOAD_LABEL })).toBeDisabled();
    });

    it('asks for the license key again when it cannot be verified, not for a renewal', async () => {
      await loaded(UNVERIFIED_DESTINATIONS, UNVERIFIED_RETENTION);

      const warning = screen.getByRole('status', { name: '' });
      expect(warning).toHaveClass('alert-warning');
      expect(warning).toHaveTextContent(UNVERIFIED_NOTICE);
      expect(within(warning).getByRole('link', { name: PASTE_KEY_LINK_LABEL })).toHaveAttribute(
        'href',
        '/settings/server/license'
      );
      expect(screen.queryByRole('link', { name: RENEW_LINK_LABEL })).not.toBeInTheDocument();
      expect(screen.queryByRole('note')).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: DOWNLOAD_LABEL })).toBeDisabled();
      expect(within(card(DOWNLOAD_TITLE)).getByText(DOWNLOAD_UNVERIFIED)).toBeInTheDocument();
      expect(screen.queryByText(DOWNLOAD_READ_ONLY)).not.toBeInTheDocument();
      expect(within(card(RETENTION_TITLE)).getByText(READ_ONLY_UNVERIFIED)).toBeInTheDocument();
      expect(screen.queryByText(RETENTION_NOT_APPLIED)).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: SAVE_RETENTION_LABEL })).not.toBeInTheDocument();
    });

    it('asks an organization owner who does not administer the server for a license instead of linking to it', async () => {
      await loaded(COMMUNITY_DESTINATIONS, COMMUNITY_RETENTION, ORGANIZATION_SCOPE, false);

      const notice = screen.getByRole('note');
      expect(notice).toHaveTextContent(UPGRADE_NOTICE);
      expect(notice).toHaveTextContent(ASK_FOR_LICENSE_LABEL);
      expect(screen.queryByRole('link')).not.toBeInTheDocument();
    });

    it('asks an organization owner who does not administer the server to get a lapsed license renewed', async () => {
      await loaded(LAPSED_DESTINATIONS, LAPSED_RETENTION, ORGANIZATION_SCOPE, false);

      const warning = screen.getByRole('status', { name: '' });
      expect(warning).toHaveTextContent(ASK_FOR_RENEWAL_LABEL);
      expect(screen.queryByRole('link')).not.toBeInTheDocument();
    });

    it('asks an organization owner who does not administer the server to get the license key pasted again', async () => {
      await loaded(UNVERIFIED_DESTINATIONS, UNVERIFIED_RETENTION, ORGANIZATION_SCOPE, false);

      const warning = screen.getByRole('status', { name: '' });
      expect(warning).toHaveTextContent(ASK_FOR_KEY_LABEL);
      expect(screen.queryByRole('link')).not.toBeInTheDocument();
    });

    it('shows the server-wide audit export with its own description and data', async () => {
      await loaded(SERVER_DESTINATIONS, SERVER_RETENTION, SERVER_SCOPE);

      expect(screen.getByText(SERVER_DESCRIPTION)).toBeInTheDocument();
      expect(row(SERVER_SYSLOG.name)).toBeInTheDocument();
      expect(destinations).toHaveBeenCalledWith(SERVER_SCOPE);
      expect(retention).toHaveBeenCalledWith(SERVER_SCOPE);
    });

    it('explains a delivery stalled behind an open transaction', async () => {
      await loaded(STALLED_DESTINATIONS, ENTERPRISE_RETENTION);

      expect(
        screen.getByText(stalledMessage(STALLED_DESTINATIONS.stalled_since as string))
      ).toBeInTheDocument();
    });
  });

  describe('destinations', () => {
    it('shows each destination with its kind, health, backlog and last delivery', async () => {
      await loaded(ENTERPRISE_DESTINATIONS, ENTERPRISE_RETENTION);

      expect(screen.queryByRole('note')).not.toBeInTheDocument();
      const failing = row(FAILING_WEBHOOK.name);
      expect(within(failing).getByText('Webhook')).toBeInTheDocument();
      expect(within(failing).getByText('Failing')).toBeInTheDocument();
      expect(
        within(failing).getByText(
          FAILING_WEBHOOK.settings.kind === 'webhook' ? FAILING_WEBHOOK.settings.url : ''
        )
      ).toBeInTheDocument();
      expect(within(failing).getByText('342 events waiting')).toBeInTheDocument();
      expect(failing).toHaveTextContent(deliveryError(FAILING_WEBHOOK.delivery) as string);
      expect(within(failing).queryByText(UNENCRYPTED_LABEL)).not.toBeInTheDocument();

      const archive = row(HEALTHY_S3.name);
      expect(within(archive).getByText('S3')).toBeInTheDocument();
      expect(within(archive).getByText('Healthy')).toBeInTheDocument();
      expect(within(archive).getByText(UP_TO_DATE)).toBeInTheDocument();
      expect(within(archive).getByText('s3://acme-audit/zone/ · eu-west-1')).toBeInTheDocument();
      expect(within(archive).queryByText(/Last error/)).not.toBeInTheDocument();

      expect(within(row(PLAIN_HTTP_WEBHOOK.name)).getByText(UNENCRYPTED_LABEL)).toBeInTheDocument();

      const legacy = row(DISABLED_UDP_SYSLOG.name);
      expect(within(legacy).getByText('Disabled')).toBeInTheDocument();
      expect(within(legacy).getByText(UNENCRYPTED_LABEL)).toBeInTheDocument();
      expect(within(legacy).getByText('10.0.0.5:514 · UDP')).toBeInTheDocument();
      expect(
        within(legacy).getByRole('switch', { name: enabledLabel(DISABLED_UDP_SYSLOG.name) })
      ).toHaveAttribute('aria-checked', 'false');
    });

    it('says when a destination has delivered nothing yet', async () => {
      await loaded(
        {
          ...ENTERPRISE_DESTINATIONS,
          destinations: [
            { ...HEALTHY_S3, delivery: { ...HEALTHY_S3.delivery, last_success_at: null } },
          ],
        },
        ENTERPRISE_RETENTION
      );

      expect(within(row(HEALTHY_S3.name)).getByText(NEVER_DELIVERED)).toBeInTheDocument();
    });

    it('invites an entitled admin to add the first destination', async () => {
      await loaded({ ...ENTERPRISE_DESTINATIONS, destinations: [] }, FOREVER_RETENTION);

      expect(screen.getByText(NO_DESTINATIONS)).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: ADD_DESTINATION_LABEL }));
      expect(
        await screen.findByRole('dialog', { name: ADD_DESTINATION_LABEL })
      ).toBeInTheDocument();
    });

    it('disables a destination from its switch', async () => {
      updateDestination.mockResolvedValue({ ...FAILING_WEBHOOK, enabled: false });
      await loaded(ENTERPRISE_DESTINATIONS, ENTERPRISE_RETENTION);

      fireEvent.click(screen.getByRole('switch', { name: enabledLabel(FAILING_WEBHOOK.name) }));

      await waitFor(() =>
        expect(updateDestination).toHaveBeenCalledWith(ORGANIZATION_SCOPE, FAILING_WEBHOOK.id, {
          enabled: false,
        })
      );
    });

    it('shows a refused switch on its row', async () => {
      updateDestination.mockRejectedValue(
        new AuditRefusal({ error: 'License required', code: AuditCode.LicenseRequired }, 403)
      );
      await loaded(ENTERPRISE_DESTINATIONS, ENTERPRISE_RETENTION);

      const legacy = row(DISABLED_UDP_SYSLOG.name);
      fireEvent.click(
        within(legacy).getByRole('switch', { name: enabledLabel(DISABLED_UDP_SYSLOG.name) })
      );

      expect(await within(legacy).findByRole('alert')).toHaveTextContent(
        refusalMessage(AuditCode.LicenseRequired)
      );
    });

    it('lets a lapsed admin disable an enabled destination but not enable a disabled one', async () => {
      await loaded(
        {
          ...LAPSED_DESTINATIONS,
          destinations: [PAUSED_SYSLOG, { ...DISABLED_UDP_SYSLOG }],
        },
        LAPSED_RETENTION
      );

      expect(screen.getByRole('switch', { name: enabledLabel(PAUSED_SYSLOG.name) })).toBeEnabled();
      expect(
        screen.getByRole('switch', { name: enabledLabel(DISABLED_UDP_SYSLOG.name) })
      ).toBeDisabled();
    });

    it('reports a delivered test event', async () => {
      testDestination.mockResolvedValue(DELIVERED_TEST);
      await loaded(ENTERPRISE_DESTINATIONS, ENTERPRISE_RETENTION);

      const archive = row(HEALTHY_S3.name);
      fireEvent.click(within(archive).getByRole('button', { name: testLabel(HEALTHY_S3.name) }));

      expect(
        await within(archive).findByText(testResultMessage(DELIVERED_TEST))
      ).toBeInTheDocument();
      expect(testDestination).toHaveBeenCalledWith(ORGANIZATION_SCOPE, HEALTHY_S3.id);
      expect(within(row(FAILING_WEBHOOK.name)).queryByRole('status')).not.toBeInTheDocument();
    });

    it('reports why a test event was not delivered', async () => {
      testDestination.mockResolvedValue(FAILED_TEST);
      await loaded(ENTERPRISE_DESTINATIONS, ENTERPRISE_RETENTION);

      const failing = row(FAILING_WEBHOOK.name);
      fireEvent.click(
        within(failing).getByRole('button', { name: testLabel(FAILING_WEBHOOK.name) })
      );

      const result = await within(failing).findByText(testResultMessage(FAILED_TEST));
      expect(result).toHaveClass('audit-test-failed');
    });

    it('opens the edit form for a destination', async () => {
      await loaded(ENTERPRISE_DESTINATIONS, ENTERPRISE_RETENTION);

      fireEvent.click(screen.getByRole('button', { name: `Edit ${HEALTHY_S3.name}` }));

      const dialog = await screen.findByRole('dialog', { name: EDIT_DESTINATION_TITLE });
      expect(within(dialog).getByLabelText('Bucket')).toHaveValue('acme-audit');
    });

    it('deletes a destination after confirming', async () => {
      deleteDestination.mockResolvedValue(undefined);
      await loaded(ENTERPRISE_DESTINATIONS, ENTERPRISE_RETENTION);

      fireEvent.click(screen.getByRole('button', { name: `Delete ${FAILING_WEBHOOK.name}` }));
      const dialog = await screen.findByRole('dialog');
      expect(dialog).toHaveTextContent(deleteMessage(FAILING_WEBHOOK.name));
      fireEvent.click(within(dialog).getByRole('button', { name: DELETE_CONFIRM_LABEL }));

      await waitFor(() =>
        expect(
          screen.queryByRole('listitem', { name: FAILING_WEBHOOK.name })
        ).not.toBeInTheDocument()
      );
      expect(deleteDestination).toHaveBeenCalledWith(ORGANIZATION_SCOPE, FAILING_WEBHOOK.id);
    });

    it('keeps the delete dialog open with the reason when it is refused', async () => {
      deleteDestination.mockRejectedValue(
        new AuditRefusal({ error: 'Gone', code: AuditCode.DestinationNotFound }, 404)
      );
      await loaded(ENTERPRISE_DESTINATIONS, ENTERPRISE_RETENTION);

      fireEvent.click(screen.getByRole('button', { name: `Delete ${FAILING_WEBHOOK.name}` }));
      const dialog = await screen.findByRole('dialog');
      fireEvent.click(within(dialog).getByRole('button', { name: DELETE_CONFIRM_LABEL }));

      expect(await within(dialog).findByRole('alert')).toHaveTextContent(
        refusalMessage(AuditCode.DestinationNotFound)
      );
    });
  });

  describe('retention', () => {
    it('shows the period, cutoff and holders retention is waiting on', async () => {
      await loaded(ENTERPRISE_DESTINATIONS, WAITING_RETENTION);

      const panel = card(RETENTION_TITLE);
      expect(panel).toHaveTextContent('Events older than 90 days are deleted');
      expect(panel).toHaveTextContent('412');
      expect(panel).toHaveTextContent('342');
      expect(within(panel).getByRole('heading', { name: HOLDERS_LABEL })).toBeInTheDocument();
      expect(within(panel).getByText(FAILING_WEBHOOK.name)).toBeInTheDocument();
      expect(within(panel).getByText(waitMessage(RetentionWait.Failing))).toBeInTheDocument();
      expect(within(panel).getByRole('button', { name: SAVE_RETENTION_LABEL })).toBeDisabled();
    });

    it('hides the holders when nothing is waiting', async () => {
      await loaded(ENTERPRISE_DESTINATIONS, ENTERPRISE_RETENTION);

      expect(screen.queryByRole('heading', { name: HOLDERS_LABEL })).not.toBeInTheDocument();
      expect(card(DESTINATIONS_TITLE)).toBeInTheDocument();
    });
  });
});
