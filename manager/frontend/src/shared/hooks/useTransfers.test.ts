import { describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import { cancelled } from '../../api/upload';
import { type TransferContext, TransferStatus, useTransfers } from './useTransfers';

type Held = {
  file: File;
  context: TransferContext;
  resolve: (value: string) => void;
  reject: (error: unknown) => void;
};

function heldRun() {
  const held: Held[] = [];
  const run = (file: File, context: TransferContext) =>
    new Promise<string>((resolve, reject) => {
      held.push({ file, context, resolve, reject });
      context.signal.addEventListener('abort', () => reject(cancelled()));
    });
  return { held, run };
}

function file(name: string): File {
  return new File([name], name, { type: 'text/plain' });
}

describe('useTransfers', () => {
  it('reports progress, then processing, then hands the result over and drops the row', async () => {
    const { held, run } = heldRun();
    const onDone = mock();
    const { result } = renderHook(() => useTransfers({ run, onDone }));

    act(() => result.current.start([file('a.txt')]));
    expect(result.current.transfers).toMatchObject([
      { name: 'a.txt', status: TransferStatus.Uploading, progress: 0 },
    ]);
    expect(result.current.pending).toBe(1);

    act(() => held[0].context.onProgress(0.5));
    expect(result.current.transfers[0]).toMatchObject({ progress: 0.5 });

    act(() => held[0].context.onProgress(1));
    expect(result.current.transfers[0]).toMatchObject({ status: TransferStatus.Processing });

    await act(async () => held[0].resolve('done'));
    expect(result.current.transfers).toEqual([]);
    expect(result.current.pending).toBe(0);
    expect(onDone).toHaveBeenCalledWith('done', held[0].file, expect.any(String));
  });

  it('keeps a failed upload with its reason until it is retried', async () => {
    const { held, run } = heldRun();
    const onDone = mock();
    const { result } = renderHook(() => useTransfers({ run, onDone }));

    act(() => result.current.start([file('a.txt')]));
    await act(async () => held[0].reject(new Error('Server said no')));
    expect(result.current.transfers[0]).toMatchObject({
      status: TransferStatus.Failed,
      error: 'Server said no',
    });
    expect(result.current.pending).toBe(0);

    act(() => result.current.retry(result.current.transfers[0].id));
    expect(held).toHaveLength(2);
    expect(result.current.transfers[0]).toMatchObject({
      status: TransferStatus.Uploading,
      progress: 0,
      error: undefined,
    });

    await act(async () => held[1].resolve('second time'));
    expect(onDone).toHaveBeenCalledWith('second time', held[1].file, expect.any(String));
    expect(result.current.transfers).toEqual([]);
  });

  it('aborts and forgets a cancelled upload without reporting a failure', async () => {
    const { held, run } = heldRun();
    const onDone = mock();
    const { result } = renderHook(() => useTransfers({ run, onDone }));

    act(() => result.current.start([file('a.txt')]));
    act(() => result.current.cancel(result.current.transfers[0].id));

    expect(held[0].context.signal.aborted).toBe(true);
    await waitFor(() => expect(result.current.transfers).toEqual([]));
    expect(onDone).not.toHaveBeenCalled();
  });

  it('queues past the limit and starts the next upload when a slot frees', async () => {
    const { held, run } = heldRun();
    const { result } = renderHook(() => useTransfers({ run, onDone: () => {}, limit: 1 }));

    act(() => result.current.start([file('a.txt'), file('b.txt')]));
    expect(held).toHaveLength(1);
    expect(result.current.transfers.map((transfer) => transfer.status)).toEqual([
      TransferStatus.Uploading,
      TransferStatus.Queued,
    ]);
    expect(result.current.pending).toBe(2);

    await act(async () => held[0].resolve('a'));
    expect(held).toHaveLength(2);
    expect(result.current.transfers).toMatchObject([
      { name: 'b.txt', status: TransferStatus.Uploading },
    ]);
  });

  it('ignores a file that is already on its way', () => {
    const { held, run } = heldRun();
    const { result } = renderHook(() =>
      useTransfers({ run, onDone: () => {}, identify: (dropped) => dropped.name })
    );

    act(() => result.current.start([file('a.txt')]));
    act(() => result.current.start([file('a.txt')]));

    expect(held).toHaveLength(1);
    expect(result.current.transfers).toHaveLength(1);
  });

  it('aborts every upload in flight when the owner unmounts', () => {
    const { held, run } = heldRun();
    const { result, unmount } = renderHook(() => useTransfers({ run, onDone: () => {} }));

    act(() => result.current.start([file('a.txt'), file('b.txt')]));
    unmount();

    expect(held.every((entry) => entry.context.signal.aborted)).toBe(true);
  });
});
