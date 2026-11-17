import { useMutation } from '@tanstack/react-query';
import { AuditRefusal } from '../AuditRefusal';
import { auditApi } from '../api';
import { saveDownload } from '../save';
import type { AuditScope, ExportDownload, ExportQuery } from '../types';

export interface AuditExportState {
  download(query: ExportQuery, onDownloaded?: (download: ExportDownload) => void): void;
  pending: boolean;
  refusal: AuditRefusal | null;
  error: string | null;
  reset(): void;
}

export function useAuditExport(scope: AuditScope): AuditExportState {
  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (query: ExportQuery) => auditApi.download(scope, query),
    onSuccess: saveDownload,
  });

  return {
    download: (query, onDownloaded) =>
      mutate(query, { onSuccess: (download) => onDownloaded?.(download) }),
    pending: isPending,
    refusal: error instanceof AuditRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
