import { ApiError } from '../../../api/ApiError';
import { DraftField } from './draft';

const BAD_REQUEST = 400;
const URL_REFUSAL = new RegExp(`: (\`${DraftField.Url}\` .+)$`);

export function messageOf(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

export function urlRefusal(error: unknown): string | null {
  if (!(error instanceof ApiError) || error.status !== BAD_REQUEST) {
    return null;
  }
  return URL_REFUSAL.exec(error.message)?.[1] ?? null;
}
