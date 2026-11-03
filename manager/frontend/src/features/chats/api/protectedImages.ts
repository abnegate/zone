import { API_BASE, client } from '../../../api/client';
import { TRAIN_PREVIEW_PATH } from '../../models/utils/trainProgress';

const PROTECTED_ARTIFACT_PREFIX = '/api/artifacts/';
const PROTECTED_IMAGE_PREFIXES = [PROTECTED_ARTIFACT_PREFIX, TRAIN_PREVIEW_PATH] as const;

export function isProtectedArtifactUrl(url: string): boolean {
  return url.startsWith(PROTECTED_ARTIFACT_PREFIX);
}

export function isProtectedImageUrl(url: string): boolean {
  return PROTECTED_IMAGE_PREFIXES.some((prefix) => url.startsWith(prefix));
}

export async function fetchProtectedImage(
  url: string,
  signal?: AbortSignal,
  accessToken = client.getAccessToken()
): Promise<Blob> {
  if (!isProtectedImageUrl(url)) {
    throw new Error('Only protected images can be fetched with authentication');
  }
  if (!accessToken) {
    throw new Error('Authentication is required to load this image');
  }

  const response = await fetch(`${API_BASE}${url}`, {
    headers: {
      Authorization: `Bearer ${accessToken}`,
    },
    signal,
  });

  if (!response.ok) {
    throw new Error(`Failed to load image: ${response.status}`);
  }

  return response.blob();
}

export async function fetchSignedArtifactUrl(
  url: string,
  signal?: AbortSignal,
  accessToken = client.getAccessToken()
): Promise<string> {
  if (!isProtectedArtifactUrl(url)) {
    throw new Error('Only protected artifacts can be signed');
  }
  if (!accessToken) {
    throw new Error('Authentication is required to load this artifact');
  }

  const response = await fetch(`${API_BASE}${url}/signature`, {
    headers: {
      Authorization: `Bearer ${accessToken}`,
    },
    signal,
  });

  if (!response.ok) {
    throw new Error(`Failed to sign artifact: ${response.status}`);
  }

  const signed = (await response.json()) as { url?: unknown };
  if (typeof signed.url !== 'string') {
    throw new Error('Signature response did not include a URL');
  }
  return `${API_BASE}${signed.url}`;
}
