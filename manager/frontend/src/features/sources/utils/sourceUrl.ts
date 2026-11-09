import type { SourceType } from '../types';
import { gitLabSourceUrl } from './gitLabSourceUrl';

const GITHUB_ORIGIN = 'https://github.com';

function named(value: unknown): value is string {
  return typeof value === 'string' && value !== '';
}

export function sourceUrl(
  sourceType: SourceType,
  config: unknown,
  url: string | null | undefined
): string {
  if (url) return url;
  if (!config || typeof config !== 'object') return '';
  const fields = config as Record<string, unknown>;
  if (sourceType === 'github' && named(fields.owner) && named(fields.repo)) {
    return `${GITHUB_ORIGIN}/${fields.owner}/${fields.repo}`;
  }
  if (sourceType === 'gitlab') {
    return gitLabSourceUrl(fields) ?? '';
  }
  if ((sourceType === 'web' || sourceType === 'ical') && named(fields.url)) {
    return fields.url;
  }
  return '';
}
