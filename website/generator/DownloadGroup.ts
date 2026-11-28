import type { Download } from './Download.ts';

export interface DownloadGroup {
  readonly id: string;
  readonly platform: string;
  readonly note: string;
  readonly command?: string;
  readonly downloads: ReadonlyArray<Download>;
}
