import { useMutation } from '@tanstack/react-query';
import { saveDownload } from '../../audit/save';
import type { ExportDownload } from '../../audit/types';
import { spendApi } from '../api';
import { SpendRefusal } from '../SpendRefusal';
import type { SpendQuery } from '../schemas';

export interface SpendExportState {
  download(query: SpendQuery, onDownloaded?: (download: ExportDownload) => void): void;
  pending: boolean;
  refusal: SpendRefusal | null;
  error: string | null;
  reset(): void;
}

export function useSpendExport(organizationId: string): SpendExportState {
  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (query: SpendQuery) => spendApi.download(organizationId, query),
    onSuccess: saveDownload,
  });

  return {
    download: (query, onDownloaded) =>
      mutate(query, { onSuccess: (download) => onDownloaded?.(download) }),
    pending: isPending,
    refusal: error instanceof SpendRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
