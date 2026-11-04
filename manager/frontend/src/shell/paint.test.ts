import { afterEach, beforeEach, describe, expect, it } from 'bun:test';
import {
  paintShell,
  SHELL_ATTRIBUTE,
  SHELL_ID,
  SHELL_TEMPLATE_ATTRIBUTE,
  type ShellConfig,
  shellKey,
} from './paint';

const config: ShellConfig = {
  id: SHELL_ID,
  attribute: SHELL_ATTRIBUTE,
  templateAttribute: SHELL_TEMPLATE_ATTRIBUTE,
  themeKey: 'theme',
  sessionKeys: ['access', 'refresh'],
  routes: { '/': '/login', '/login': '/login', '/reset-password': '/reset-password' },
  tokenRoutes: ['/reset-password'],
  tokenPattern: '^[a-z]{20}$',
};

const TEMPLATES: Record<string, string> = {
  [shellKey('/login')]: '<main>Sign in</main>',
  [shellKey('/reset-password', 'missing')]: '<main>No token</main>',
  [shellKey('/reset-password', 'invalid')]: '<main>Bad token</main>',
  [shellKey('/reset-password', 'valid')]: '<main>New password</main>',
};

function visit(href: string) {
  window.history.replaceState(null, '', href);
}

function shell(): HTMLElement | null {
  return document.getElementById(SHELL_ID);
}

describe('paintShell', () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute('data-theme');
    document.documentElement.removeAttribute(SHELL_ATTRIBUTE);
    document.body.innerHTML = `<div id="root"><div class="initial-loader"></div></div>${Object.entries(
      TEMPLATES
    )
      .map(([key, markup]) => `<template ${SHELL_TEMPLATE_ATTRIBUTE}="${key}">${markup}</template>`)
      .join('')}`;
  });

  afterEach(() => {
    localStorage.clear();
    visit('/');
  });

  it('paints the signed-out page above the root before the app loads', () => {
    visit('/login');
    paintShell(config);
    expect(shell()?.textContent).toBe('Sign in');
    expect(shell()?.nextElementSibling?.id).toBe('root');
    expect(shell()?.hasAttribute('inert')).toBe(true);
    expect(document.documentElement.getAttribute(SHELL_ATTRIBUTE)).toBe('/login');
  });

  it('paints the login page for the root a signed-out visitor is sent away from', () => {
    visit('/');
    paintShell(config);
    expect(shell()?.textContent).toBe('Sign in');
  });

  it('ignores a trailing slash', () => {
    visit('/login/');
    paintShell(config);
    expect(shell()?.textContent).toBe('Sign in');
  });

  it('matches the path regardless of case, as the router does', () => {
    visit('/Login');
    paintShell(config);
    expect(shell()?.textContent).toBe('Sign in');
  });

  it('paints nothing for a stored session', () => {
    for (const key of config.sessionKeys) {
      localStorage.clear();
      localStorage.setItem(key, 'token');
      visit('/login');
      paintShell(config);
      expect(shell()).toBeNull();
      expect(document.documentElement.hasAttribute(SHELL_ATTRIBUTE)).toBe(false);
      expect(document.documentElement.hasAttribute('data-theme')).toBe(false);
    }
  });

  it('paints nothing for a route without a shell', () => {
    visit('/chats');
    paintShell(config);
    expect(shell()).toBeNull();
  });

  it('picks the variant for the token in the link', () => {
    const cases: [string, string][] = [
      ['/reset-password', 'No token'],
      ['/reset-password?token=short', 'Bad token'],
      [`/reset-password?token=${'a'.repeat(20)}`, 'New password'],
    ];
    for (const [href, text] of cases) {
      shell()?.remove();
      visit(href);
      paintShell(config);
      expect(shell()?.textContent).toBe(text);
    }
  });

  it('applies the stored theme before the first paint', () => {
    localStorage.setItem('theme', 'dark');
    visit('/login');
    paintShell(config);
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
  });

  it('falls back to the system theme', () => {
    const matchMedia = window.matchMedia;
    window.matchMedia = ((query: string) => ({
      matches: query.includes('dark'),
    })) as typeof matchMedia;
    try {
      visit('/login');
      paintShell(config);
      expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    } finally {
      window.matchMedia = matchMedia;
    }
  });

  it('paints nothing when storage is unavailable', () => {
    const descriptor = Object.getOwnPropertyDescriptor(window, 'localStorage');
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      get: () => {
        throw new Error('blocked');
      },
    });
    try {
      visit('/login');
      expect(() => paintShell(config)).not.toThrow();
      expect(shell()).toBeNull();
    } finally {
      if (descriptor) {
        Object.defineProperty(window, 'localStorage', descriptor);
      }
    }
  });

  it('runs from its source text alone', () => {
    const inlined = new Function(`return (${paintShell.toString()})`)() as typeof paintShell;
    visit('/login');
    inlined(config);
    expect(shell()?.textContent).toBe('Sign in');
  });
});
