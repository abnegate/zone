import { useCallback, useEffect, useRef, useState } from 'react';
import { isCancelled } from '../../api/upload';

export const TransferStatus = {
  Queued: 'queued',
  Uploading: 'uploading',
  Processing: 'processing',
  Failed: 'failed',
} as const;

export type TransferStatus = (typeof TransferStatus)[keyof typeof TransferStatus];

export type Transfer = {
  id: string;
  name: string;
  size: number;
  status: TransferStatus;
  progress: number;
  error?: string;
};

export type TransferContext = {
  signal: AbortSignal;
  onProgress: (fraction: number) => void;
};

export type TransferOptions<T> = {
  run: (file: File, context: TransferContext) => Promise<T>;
  onDone: (result: T, file: File, id: string) => void;
  identify?: (file: File) => string;
  limit?: number;
};

export type Transfers = {
  transfers: Transfer[];
  pending: number;
  start: (files: File[]) => void;
  cancel: (id: string) => void;
  retry: (id: string) => void;
  clear: () => void;
};

function reason(error: unknown): string {
  return error instanceof Error && error.message ? error.message : 'The upload failed';
}

function fraction(value: number): number {
  return Number.isFinite(value) ? Math.min(Math.max(value, 0), 1) : 0;
}

export function useTransfers<T>(options: TransferOptions<T>): Transfers {
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const latest = useRef(options);
  latest.current = options;
  const files = useRef(new Map<string, File>());
  const controllers = useRef(new Map<string, AbortController>());
  const queue = useRef<string[]>([]);

  const update = useCallback((id: string, patch: Partial<Transfer>) => {
    setTransfers((current) =>
      current.map((transfer) => (transfer.id === id ? { ...transfer, ...patch } : transfer))
    );
  }, []);

  const forget = useCallback((id: string) => {
    files.current.delete(id);
    controllers.current.delete(id);
    queue.current = queue.current.filter((queued) => queued !== id);
    setTransfers((current) => current.filter((transfer) => transfer.id !== id));
  }, []);

  const pump = useCallback(() => {
    const limit = latest.current.limit ?? Number.POSITIVE_INFINITY;
    while (controllers.current.size < limit) {
      const id = queue.current.shift();
      if (id === undefined) return;
      const file = files.current.get(id);
      if (!file) continue;

      const controller = new AbortController();
      controllers.current.set(id, controller);
      update(id, { status: TransferStatus.Uploading, progress: 0, error: undefined });

      const onProgress = (value: number) => {
        if (controller.signal.aborted) return;
        const progress = fraction(value);
        update(id, {
          progress,
          status: progress >= 1 ? TransferStatus.Processing : TransferStatus.Uploading,
        });
      };

      new Promise<T>((resolve) =>
        resolve(latest.current.run(file, { signal: controller.signal, onProgress }))
      )
        .then(
          (result) => {
            if (controller.signal.aborted) return;
            forget(id);
            latest.current.onDone(result, file, id);
          },
          (error: unknown) => {
            if (controller.signal.aborted) return;
            if (isCancelled(error)) {
              forget(id);
              return;
            }
            controllers.current.delete(id);
            update(id, { status: TransferStatus.Failed, error: reason(error) });
          }
        )
        .finally(pump);
    }
  }, [forget, update]);

  const retry = useCallback(
    (id: string) => {
      if (!files.current.has(id) || controllers.current.has(id)) return;
      if (queue.current.includes(id)) return;
      queue.current.push(id);
      update(id, { status: TransferStatus.Queued, progress: 0, error: undefined });
      pump();
    },
    [pump, update]
  );

  const start = useCallback(
    (incoming: File[]) => {
      const added: Transfer[] = [];
      for (const file of incoming) {
        const id = latest.current.identify?.(file) ?? crypto.randomUUID();
        if (files.current.has(id)) {
          retry(id);
          continue;
        }
        files.current.set(id, file);
        queue.current.push(id);
        added.push({
          id,
          name: file.name,
          size: file.size,
          status: TransferStatus.Queued,
          progress: 0,
        });
      }
      if (added.length === 0) return;
      setTransfers((current) => [...current, ...added]);
      pump();
    },
    [pump, retry]
  );

  const cancel = useCallback(
    (id: string) => {
      controllers.current.get(id)?.abort();
      forget(id);
      pump();
    },
    [forget, pump]
  );

  const clear = useCallback(() => {
    for (const controller of controllers.current.values()) controller.abort();
    controllers.current.clear();
    files.current.clear();
    queue.current = [];
    setTransfers([]);
  }, []);

  useEffect(
    () => () => {
      for (const controller of controllers.current.values()) controller.abort();
    },
    []
  );

  const pending = transfers.filter((transfer) => transfer.status !== TransferStatus.Failed).length;

  return { transfers, pending, start, cancel, retry, clear };
}
