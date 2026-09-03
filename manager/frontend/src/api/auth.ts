import type { AuthResponse, LoginRequest, RegisterRequest } from '../types';
import { parse } from '../validation';
import { AuthResponseSchema } from '../validation/schemas';

const API_BASE = import.meta.env.VITE_API_URL || '';

export async function login(request: LoginRequest): Promise<AuthResponse> {
  const response = await fetch(`${API_BASE}/api/auth/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: 'Login failed' }));
    throw new Error(error.error || 'Login failed');
  }

  const data = await response.json();
  return parse(AuthResponseSchema, data);
}

export async function register(request: RegisterRequest): Promise<AuthResponse> {
  const response = await fetch(`${API_BASE}/api/auth/register`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: 'Registration failed' }));
    throw new Error(error.error || 'Registration failed');
  }

  const data = await response.json();
  return parse(AuthResponseSchema, data);
}

/// A refresh that failed. `status` distinguishes a rejected credential from a
/// request that never reached the server, which decides whether the session is
/// actually over.
export class RefreshError extends Error {
  readonly status?: number;

  constructor(message: string, status?: number) {
    super(message);
    this.name = 'RefreshError';
    this.status = status;
  }

  /// Only the server saying "no" ends a session. A proxy reload, a restarting
  /// backend or a dropped connection must not sign the user out.
  get credentialRejected(): boolean {
    return this.status === 401 || this.status === 403;
  }
}

export async function refreshToken(token: string): Promise<AuthResponse> {
  let response: Response;
  try {
    response = await fetch(`${API_BASE}/api/auth/refresh`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ refresh_token: token }),
    });
  } catch (cause) {
    throw new RefreshError(`Token refresh could not reach the server: ${cause}`);
  }

  if (!response.ok) {
    throw new RefreshError(`Token refresh failed: ${response.status}`, response.status);
  }

  const data = await response.json();
  return parse(AuthResponseSchema, data);
}

export async function logout(token: string): Promise<void> {
  await fetch(`${API_BASE}/api/auth/logout`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ refresh_token: token }),
  }).catch(() => {
    // Ignore logout errors - we clear local state anyway
  });
}
