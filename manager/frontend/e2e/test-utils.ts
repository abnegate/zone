import type { Page, BrowserContext, Route } from '@playwright/test';

// Block service worker to allow Playwright route interception to work
export async function blockServiceWorker(context: BrowserContext) {
  await context.route('**/service-worker.js', (route) => {
    route.fulfill({ status: 404, body: '' });
  });
}

export async function routeApi(
  page: Page,
  url: string | RegExp,
  handler: (route: Route) => Promise<void> | void
) {
  await page.route(url, (route) => {
    const type = route.request().resourceType();
    if (type !== 'xhr' && type !== 'fetch') {
      return route.continue();
    }
    return handler(route);
  });
}

export async function routeApiContext(
  context: BrowserContext,
  url: string | RegExp,
  handler: (route: Route) => Promise<void> | void
) {
  await context.route(url, (route) => {
    const type = route.request().resourceType();
    if (type !== 'xhr' && type !== 'fetch') {
      return route.continue();
    }
    return handler(route);
  });
}

// Helper to create a mock JWT token with embedded roles and permissions
// JWT format: header.payload.signature (all base64url encoded)
export function createMockJwt(payload: {
  sub: string;
  email: string;
  roles: string[];
  permissions: string[];
  exp: number;
}) {
  const header = { alg: 'HS256', typ: 'JWT' };
  const fullPayload = { ...payload, iat: Math.floor(Date.now() / 1000), jti: 'test-jti' };
  const base64Header = Buffer.from(JSON.stringify(header)).toString('base64');
  const base64Payload = Buffer.from(JSON.stringify(fullPayload)).toString('base64');
  return `${base64Header}.${base64Payload}.mock-signature`;
}

// Standard user permissions
export const userPermissions = [
  'models:read',
  'models:create',
  'chats:read',
  'chats:create',
  'chats:update',
  'chats:delete',
  'projects:read',
  'projects:create',
  'projects:update',
  'projects:delete',
  'tasks:read',
  'tasks:create',
  'tasks:update',
  'tasks:delete',
  'sources:read',
  'wiki:read',
  'wiki:create',
  'wiki:update',
];

// Admin permissions
export const adminPermissions = [
  'models:read',
  'models:create',
  'models:update',
  'models:delete',
  'chats:read',
  'chats:create',
  'chats:update',
  'chats:delete',
  'projects:read',
  'projects:create',
  'projects:update',
  'projects:delete',
  'tasks:read',
  'tasks:create',
  'tasks:update',
  'tasks:delete',
  'sources:read',
  'sources:create',
  'sources:update',
  'sources:delete',
  'wiki:read',
  'wiki:create',
  'wiki:update',
  'wiki:delete',
  'users:read',
  'users:create',
  'users:update',
  'users:delete',
];

// Default mock user
export const mockUser = {
  id: 'user-1',
  email: 'test@example.com',
  email_verified: true,
  display_name: 'Test User',
  is_active: true,
  is_admin: false,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
  last_login_at: null,
};

// Admin mock user
export const mockAdminUser = {
  id: 'admin-1',
  email: 'admin@example.com',
  email_verified: true,
  display_name: 'Admin User',
  is_active: true,
  is_admin: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
  last_login_at: null,
};

export const EMPTY_SETUP_PLAN = {
  ram_bytes: 68719476736,
  ram_label: '64 GB',
  disk_free_bytes: 500000000000,
  disk_free_label: '500 GB',
  vision_min_ram_bytes: 17179869184,
  disk_margin_bytes: 10000000000,
  chat_preset: '32gb',
  recommended_preset: '32gb',
  chat_presets: [
    {
      id: '32gb',
      label: '32 GB+ RAM',
      min_ram_bytes: 34359738368,
      fast: 'llama3.1:8b',
      reason: 'deepseek-r1:32b',
    },
  ],
  features: [
    {
      id: 'chat',
      label: 'Chat',
      description: 'Fast, reasoning, and embedding models',
      required: true,
      selected: true,
      blocked: false,
      block_reason: null,
      size_bytes: 25600000000,
      size_label: '26 GB',
      present_bytes: 0,
      needed_bytes: 25600000000,
      ready: false,
      licenses: [],
    },
    {
      id: 'vision',
      label: 'Vision',
      description: 'Image attachments',
      required: false,
      selected: true,
      blocked: false,
      block_reason: null,
      size_bytes: 4700000000,
      size_label: '4.7 GB',
      present_bytes: 0,
      needed_bytes: 4700000000,
      ready: false,
      licenses: [],
    },
  ],
  wants_all: true,
  gate: null,
  totals: {
    size_bytes: 30300000000,
    size_label: '30 GB',
    present_bytes: 0,
    present_label: '0 B',
    needed_bytes: 30300000000,
    needed_label: '30 GB',
    working_space_bytes: 10000000000,
    working_space_label: '10 GB',
    required_free_bytes: 40300000000,
    required_free_label: '40 GB',
    free_now_bytes: 500000000000,
    free_now_label: '500 GB',
    short_by_bytes: 0,
    short_by_label: null,
  },
  licenses: [],
  artifacts: [],
  pulls: [] as Array<{ model: string; runtime: 'ollama' | 'comfy' }>,
};

export function remainingSetupPlan() {
  const base = EMPTY_SETUP_PLAN;
  const chat = base.features[0];
  return {
    ...base,
    features: [
      {
        ...chat,
        present_bytes: chat.size_bytes,
        needed_bytes: 0,
        ready: true,
      },
      ...base.features.slice(1),
    ],
    pulls: [
      { model: 'llama3.1:8b', runtime: 'ollama' as const },
      { model: 'llava:7b', runtime: 'ollama' as const },
    ],
  };
}

// Setup authentication for a page with regular user
export async function setupAuth(page: Page, options?: { admin?: boolean }) {
  const isAdmin = options?.admin ?? false;
  const user = isAdmin ? mockAdminUser : mockUser;
  const permissions = isAdmin ? adminPermissions : userPermissions;
  const roles = isAdmin ? ['admin', 'user'] : ['user'];

  const token = createMockJwt({
    sub: user.id,
    email: user.email,
    roles,
    permissions,
    exp: Math.floor(Date.now() / 1000) + 3600, // Expires in 1 hour
  });

  // Navigate to a page first to be able to set localStorage
  await page.goto('/login');

  // Unregister any service workers that might intercept requests
  await page.evaluate(async () => {
    if ('serviceWorker' in navigator) {
      const registrations = await navigator.serviceWorker.getRegistrations();
      await Promise.all(registrations.map((r) => r.unregister()));
    }
  });

  await page.evaluate(
    ({ token, user }) => {
      localStorage.setItem('manager_access_token', token);
      localStorage.setItem('manager_refresh_token', 'mock-refresh-token');
      localStorage.setItem('manager_user', JSON.stringify(user));
      localStorage.setItem(`manager_setup_complete:${user.id}`, '1');
    },
    { token, user }
  );
}
