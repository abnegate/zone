import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { chooseSelect } from '../../../../test/select';
import { AuditRefusal } from '../AuditRefusal';
import {
  COMMUNITY_STANDING,
  ENTERPRISE_STANDING,
  EXPORT_FILENAME,
  LAPSED_STANDING,
  ORGANIZATION_SCOPE,
} from '../fixtures';
import {
  DOWNLOAD_LABEL,
  DOWNLOAD_MEMORY_NOTE,
  DOWNLOAD_READ_ONLY,
  DOWNLOADED_LABEL,
  END_DATE_LABEL,
  FORMAT_LABEL,
  START_DATE_LABEL,
} from '../labels';
import { refusalMessage } from '../messages';
import { AuditCode, ExportFormat } from '../schemas';
import type { Standing } from '../types';

const download = mock();
const saveDownload = mock();

mock.module('../api', () => ({
  auditApi: {
    destinations: mock(),
    destination: mock(),
    createDestination: mock(),
    updateDestination: mock(),
    deleteDestination: mock(),
    testDestination: mock(),
    retention: mock(),
    setRetention: mock(),
    download,
  },
}));

mock.module('../save', () => ({ saveDownload }));

let ExportPanel: typeof import('./ExportPanel').ExportPanel;

beforeAll(async () => {
  ({ ExportPanel } = await import('./ExportPanel'));
});

afterAll(() => {
  mock.restore();
});

function renderPanel(standing: Standing = ENTERPRISE_STANDING) {
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <ExportPanel scope={ORGANIZATION_SCOPE} standing={standing} />
    </QueryClientProvider>
  );
}

function dates(start: string, end: string) {
  fireEvent.change(screen.getByLabelText(START_DATE_LABEL), { target: { value: start } });
  fireEvent.change(screen.getByLabelText(END_DATE_LABEL), { target: { value: end } });
}

describe('ExportPanel', () => {
  beforeEach(() => {
    download.mockReset();
    saveDownload.mockReset();
  });

  it('downloads the chosen range and format and names the file', async () => {
    const file = { blob: new Blob(['{}\n']), filename: EXPORT_FILENAME };
    download.mockResolvedValue(file);
    renderPanel();

    expect(screen.getByText(DOWNLOAD_MEMORY_NOTE)).toBeInTheDocument();
    dates('2026-11-01', '2026-11-15');
    chooseSelect(FORMAT_LABEL, 'CSV');
    fireEvent.click(screen.getByRole('button', { name: DOWNLOAD_LABEL }));

    expect(await screen.findByText(`${DOWNLOADED_LABEL} ${EXPORT_FILENAME}`)).toBeInTheDocument();
    expect(download).toHaveBeenCalledWith(ORGANIZATION_SCOPE, {
      format: ExportFormat.Csv,
      start_date: '2026-11-01',
      end_date: '2026-11-15',
    });
    expect(saveDownload.mock.calls[0][0]).toEqual(file);
  });

  it('defaults to NDJSON over the last thirty days', () => {
    renderPanel();

    const start = (screen.getByLabelText(START_DATE_LABEL) as HTMLInputElement).value;
    const end = (screen.getByLabelText(END_DATE_LABEL) as HTMLInputElement).value;
    expect(Date.parse(end) - Date.parse(start)).toBe(30 * 86_400_000);
    expect(screen.getByRole('combobox', { name: FORMAT_LABEL })).toHaveTextContent('NDJSON');
  });

  it('refuses a range that ends before it starts', () => {
    renderPanel();

    dates('2026-11-15', '2026-11-01');
    fireEvent.click(screen.getByRole('button', { name: DOWNLOAD_LABEL }));

    const error = screen.getByText(refusalMessage(AuditCode.ExportRangeInvalid));
    for (const label of [START_DATE_LABEL, END_DATE_LABEL]) {
      const input = screen.getByLabelText(label);
      expect(input, label).toHaveAttribute('aria-invalid', 'true');
      expect(input.getAttribute('aria-describedby'), label).toBe(error.id);
    }
    expect(download).not.toHaveBeenCalled();
  });

  it('marks neither date invalid before a bad range is submitted', () => {
    renderPanel();

    dates('2026-11-15', '2026-11-01');

    for (const label of [START_DATE_LABEL, END_DATE_LABEL]) {
      expect(screen.getByLabelText(label), label).toHaveAttribute('aria-invalid', 'false');
    }
  });

  it('shows a refused download', async () => {
    download.mockRejectedValue(
      new AuditRefusal({ error: 'License', code: AuditCode.LicenseRequired }, 403)
    );
    renderPanel();

    fireEvent.click(screen.getByRole('button', { name: DOWNLOAD_LABEL }));

    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent(refusalMessage(AuditCode.LicenseRequired))
    );
  });

  it('is read-only without a license that covers downloads', () => {
    for (const standing of [COMMUNITY_STANDING, LAPSED_STANDING]) {
      const { unmount } = renderPanel(standing);

      expect(screen.getByRole('button', { name: DOWNLOAD_LABEL })).toBeDisabled();
      expect(screen.getByLabelText(START_DATE_LABEL)).toBeDisabled();
      expect(screen.getByText(DOWNLOAD_READ_ONLY)).toBeInTheDocument();
      expect(screen.queryByText(DOWNLOAD_MEMORY_NOTE)).toBeNull();
      unmount();
    }
  });
});
