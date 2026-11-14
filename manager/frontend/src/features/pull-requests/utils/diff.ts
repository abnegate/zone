import type { Tint } from '../../github';
import type { DiffLine, FileStatus, Hunk, LineKind } from '../types';

export const STATUS_LABELS: Readonly<Record<FileStatus, string>> = {
  added: 'Added',
  removed: 'Deleted',
  modified: 'Modified',
  renamed: 'Renamed',
  copied: 'Copied',
  changed: 'Changed',
  unchanged: 'Unchanged',
  unknown: 'Changed',
};

export const STATUS_TINTS: Readonly<Record<FileStatus, Tint>> = {
  added: 'success',
  removed: 'destructive',
  modified: 'neutral',
  renamed: 'info',
  copied: 'info',
  changed: 'neutral',
  unchanged: 'neutral',
  unknown: 'neutral',
};

export const MARKERS: Readonly<Record<LineKind, string>> = {
  addition: '+',
  deletion: '-',
  context: ' ',
  meta: '',
};

export const LINE_LABELS: Readonly<Record<LineKind, string | null>> = {
  addition: 'Added',
  deletion: 'Removed',
  context: null,
  meta: null,
};

export function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? '' : 's'}`;
}

export function keyed<T>(items: readonly T[], identity: (item: T) => string): [string, T][] {
  const seen = new Map<string, number>();
  return items.map((item) => {
    const base = identity(item);
    const occurrence = seen.get(base) ?? 0;
    seen.set(base, occurrence + 1);
    return [occurrence === 0 ? base : `${base}#${occurrence}`, item];
  });
}

export function lineIdentity(line: DiffLine): string {
  return `${line.kind}:${line.old_line ?? ''}:${line.new_line ?? ''}`;
}

export function hunkIdentity(hunk: Hunk): string {
  return `${hunk.old_start}:${hunk.new_start}:${hunk.header}`;
}
