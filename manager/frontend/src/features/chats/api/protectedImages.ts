import { API_BASE, client } from '../../../api/client';

const PROTECTED_ARTIFACT_PREFIX = '/api/artifacts/';

export function isProtectedArtifactUrl(url: string): boolean {
  return url.startsWith(PROTECTED_ARTIFACT_PREFIX);
}

export async function fetchProtectedImage(
  url: string,
  signal?: AbortSignal,
  accessToken = client.getAccessToken()
): Promise<Blob> {
  if (!isProtectedArtifactUrl(url)) {
    throw new Error('Only protected artifact images can be fetched with authentication');
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
