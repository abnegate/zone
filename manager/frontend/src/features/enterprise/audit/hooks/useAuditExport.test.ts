import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode } from 'react';
import { AuditRefusal } from '../AuditRefusal';
import { EXPORT_FILENAME, EXPORT_QUERY, ORGANIZATION_SCOPE } from '../fixtures';
import { AuditCode } from '../schemas';

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

let useAuditExport: typeof import('./useAuditExport').useAuditExport;

beforeAll(async () => {
  ({ useAuditExport } = await import('./useAuditExport'));
});

afterAll(() => {
  mock.restore();
});

function render() {
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return renderHook(() => useAuditExport(ORGANIZATION_SCOPE), { wrapper });
}

describe('useAuditExport', () => {
  beforeEach(() => {
    download.mockReset();
    saveDownload.mockReset();
  });

  it('downloads the range and saves it under the server filename', async () => {
    const file = { blob: new Blob(['{}\n']), filename: EXPORT_FILENAME };
    download.mockResolvedValue(file);
    const downloaded = mock();
    const { result } = render();

    act(() => result.current.download(EXPORT_QUERY, downloaded));

    await waitFor(() => expect(downloaded).toHaveBeenCalledWith(file));
    expect(download).toHaveBeenCalledWith(ORGANIZATION_SCOPE, EXPORT_QUERY);
    expect(saveDownload).toHaveBeenCalledTimes(1);
    expect(saveDownload.mock.calls[0][0]).toBe(file);
  });

  it('saves nothing when the export is refused', async () => {
    download.mockRejectedValue(
      new AuditRefusal({ error: 'Bad range', code: AuditCode.ExportRangeInvalid }, 400)
    );
    const { result } = render();

    act(() => result.current.download(EXPORT_QUERY));

    await waitFor(() => expect(result.current.refusal?.code).toBe(AuditCode.ExportRangeInvalid));
    expect(saveDownload).not.toHaveBeenCalled();
    expect(result.current.pending).toBe(false);
  });
});
