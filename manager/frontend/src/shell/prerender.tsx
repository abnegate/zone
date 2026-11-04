import type { ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { StaticRouter } from 'react-router-dom';
import App from '../App';
import { ACCESS_TOKEN_KEY, REFRESH_TOKEN_KEY } from '../api/session';
import { TOKEN_PATTERN } from '../features/auth/utils';
import { THEME_STORAGE_KEY } from '../shared/context/ThemeContext';
import {
  paintShell,
  SHELL_ATTRIBUTE,
  SHELL_ID,
  SHELL_TEMPLATE_ATTRIBUTE,
  type ShellConfig,
  shellKey,
  type TokenState,
} from './paint';

const ROUTES: Record<string, string> = {
  '/': '/login',
  '/login': '/login',
  '/register': '/register',
  '/forgot-password': '/forgot-password',
  '/reset-password': '/reset-password',
  '/verify-email': '/verify-email',
  '/verify': '/verify-email',
  '/invitations': '/invitations',
  '/unauthorized': '/unauthorized',
};

const TOKEN_ROUTES = ['/reset-password', '/verify-email'];

const SAMPLE_TOKENS: Record<TokenState, string | null> = {
  missing: null,
  invalid: 'invalid',
  valid: '0'.repeat(32),
};

class MemoryStorage implements Storage {
  private readonly items = new Map<string, string>();

  get length(): number {
    return this.items.size;
  }

  clear(): void {
    this.items.clear();
  }

  getItem(key: string): string | null {
    return this.items.get(key) ?? null;
  }

  key(index: number): string | null {
    return [...this.items.keys()][index] ?? null;
  }

  removeItem(key: string): void {
    this.items.delete(key);
  }

  setItem(key: string, value: string): void {
    this.items.set(key, value);
  }
}

function signedOut<Result>(render: () => Result): Result {
  const localStorage = new MemoryStorage();
  const stubs: Record<string, unknown> = {
    localStorage,
    window: { localStorage, matchMedia: () => ({ matches: false }) },
  };
  const previous = Object.keys(stubs).map(
    (name) => [name, Object.getOwnPropertyDescriptor(globalThis, name)] as const
  );
  for (const [name, value] of Object.entries(stubs)) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  }
  try {
    return render();
  } finally {
    for (const [name, descriptor] of previous) {
      if (descriptor) {
        Object.defineProperty(globalThis, name, descriptor);
      } else {
        Reflect.deleteProperty(globalThis, name);
      }
    }
  }
}

export function renderRoute(href: string): string {
  const router = ({ children }: { children: ReactNode }) => (
    <StaticRouter location={href}>{children}</StaticRouter>
  );
  return signedOut(() => renderToStaticMarkup(<App router={router} />));
}

function samples(): Map<string, string> {
  const hrefs = new Map<string, string>();
  for (const route of new Set(Object.values(ROUTES))) {
    if (!TOKEN_ROUTES.includes(route)) {
      hrefs.set(shellKey(route), route);
      continue;
    }
    for (const [state, token] of Object.entries(SAMPLE_TOKENS) as [TokenState, string | null][]) {
      hrefs.set(shellKey(route, state), token ? `${route}?token=${token}` : route);
    }
  }
  return hrefs;
}

export const config: ShellConfig = {
  id: SHELL_ID,
  attribute: SHELL_ATTRIBUTE,
  templateAttribute: SHELL_TEMPLATE_ATTRIBUTE,
  themeKey: THEME_STORAGE_KEY,
  sessionKeys: [ACCESS_TOKEN_KEY, REFRESH_TOKEN_KEY],
  routes: ROUTES,
  tokenRoutes: TOKEN_ROUTES,
  tokenPattern: TOKEN_PATTERN.source,
};

export function prerender(): string {
  const templates = [...samples()].map(
    ([key, href]) =>
      `<template ${SHELL_TEMPLATE_ATTRIBUTE}="${key}">${renderRoute(href)}</template>`
  );
  const script = `(${paintShell.toString()})(${JSON.stringify(config).replaceAll('<', '\\u003c')})`;
  return `${templates.join('')}<script>${script}</script>`;
}
