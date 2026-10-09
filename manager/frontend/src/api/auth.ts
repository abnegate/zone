import type { AuthResponse, LoginRequest, RegisterRequest } from '../types';
import { parse } from '../validation';
import { AuthResponseSchema } from '../validation/schemas';
import { deviceHeaders } from './device';

const API_BASE = import.meta.env.VITE_API_URL || '';

function jsonHeaders(): HeadersInit {
  return {
    'Content-Type': 'application/json',
    ...deviceHeaders(),
  };
}

async function refused(response: Response, fallback: string): Promise<never> {
  const error = await response.json().catch(() => ({ error: fallback }));
  const message = error.error || fallback;
  if (error.code === 'device_pending' || error.code === 'device_blocked') {
    throw new AuthRequestError(message, error.code);
  }
  throw new Error(message);
}

export class AuthRequestError extends Error {
  readonly code: string;

  constructor(message: string, code: string) {
    super(message);
    this.name = 'AuthRequestError';
    this.code = code;
  }
}

export async function login(request: LoginRequest): Promise<AuthResponse> {
  const response = await fetch(`${API_BASE}/api/auth/login`, {
    method: 'POST',
    headers: jsonHeaders(),
    body: JSON.stringify(request),
  });

  if (!response.ok) {
    await refused(response, 'Login failed');
  }

  const data = await response.json();
  return parse(AuthResponseSchema, data);
}

export async function register(request: RegisterRequest): Promise<AuthResponse> {
  const response = await fetch(`${API_BASE}/api/auth/register`, {
    method: 'POST',
    headers: jsonHeaders(),
    body: JSON.stringify(request),
  });

  if (!response.ok) {
    await refused(response, 'Registration failed');
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
      headers: jsonHeaders(),
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
    headers: jsonHeaders(),
    body: JSON.stringify({ refresh_token: token }),
  }).catch(() => {
    // Ignore logout errors - we clear local state anyway
  });
}
