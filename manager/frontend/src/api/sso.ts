import type {
  AuthResponse,
  SsoAuthorization,
  SsoCallbackRequest,
  SsoPublicProviders,
} from '../features/auth/types';
import { authResponseParser, jsonHeaders, refused } from './auth';

const API_BASE = import.meta.env.VITE_API_URL || '';
const PROVIDERS_URL = `${API_BASE}/api/auth/sso/providers`;
const CALLBACKS_URL = `${API_BASE}/api/auth/sso/callbacks`;

async function schemas() {
  const [{ parse }, schemas] = await Promise.all([
    import('../validation'),
    import('../features/auth/schemas'),
  ]);
  return { parse, ...schemas };
}

export async function listProviders(): Promise<SsoPublicProviders> {
  const { parse, SsoPublicProvidersSchema } = await schemas();
  const response = await fetch(PROVIDERS_URL, { headers: { Accept: 'application/json' } });
  if (!response.ok) {
    await refused(response, 'Single sign-on providers could not be loaded');
  }
  return parse(SsoPublicProvidersSchema, await response.json());
}

export async function authorize(providerId: string): Promise<SsoAuthorization> {
  const { parse, SsoAuthorizationSchema } = await schemas();
  const response = await fetch(
    `${PROVIDERS_URL}/${encodeURIComponent(providerId)}/authorizations`,
    { method: 'POST', headers: jsonHeaders(), body: JSON.stringify({}) }
  );
  if (!response.ok) {
    await refused(response, 'Single sign-on could not start');
  }
  return parse(SsoAuthorizationSchema, await response.json());
}

export async function completeCallback(request: SsoCallbackRequest): Promise<AuthResponse> {
  const parseResponse = await authResponseParser();
  const response = await fetch(CALLBACKS_URL, {
    method: 'POST',
    headers: jsonHeaders(),
    body: JSON.stringify({ state: request.state, code: request.code }),
  });
  if (!response.ok) {
    await refused(response, 'Single sign-on failed');
  }
  return parseResponse(await response.json());
}
