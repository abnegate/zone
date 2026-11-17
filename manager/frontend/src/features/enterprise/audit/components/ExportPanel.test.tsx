import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { chooseSelect } from '../../../../test/select';
import { AuditRefusal } from '../AuditRefusal';
import { EXPORT_FILENAME, ORGANIZATION_SCOPE } from '../fixtures';
import {
  DOWNLOAD_LABEL,
  DOWNLOAD_MEMORY_NOTE,
  DOWNLOADED_LABEL,
  END_DATE_LABEL,
  FORMAT_LABEL,
  READ_ONLY_NOTICE,
  START_DATE_LABEL,
} from '../labels';
import { refusalMessage } from '../messages';
import { AuditCode, ExportFormat } from '../schemas';

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

function renderPanel(entitled = true) {
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <ExportPanel scope={ORGANIZATION_SCOPE} entitled={entitled} />
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

    expect(screen.getByText(refusalMessage(AuditCode.ExportRangeInvalid))).toBeInTheDocument();
    expect(download).not.toHaveBeenCalled();
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
    renderPanel(false);

    expect(screen.getByRole('button', { name: DOWNLOAD_LABEL })).toBeDisabled();
    expect(screen.getByLabelText(START_DATE_LABEL)).toBeDisabled();
    expect(screen.getByText(READ_ONLY_NOTICE)).toBeInTheDocument();
    expect(screen.queryByText(DOWNLOAD_MEMORY_NOTE)).toBeNull();
  });
});
