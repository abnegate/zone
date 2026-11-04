import { describe, expect, it } from 'bun:test';
import { SHELL_TEMPLATE_ATTRIBUTE, shellKey } from './paint';
import { config, prerender, renderRoute } from './prerender';

function templates(html: string): Map<string, string> {
  const pattern = new RegExp(
    `<template ${SHELL_TEMPLATE_ATTRIBUTE}="([^"]+)">(.*?)</template>`,
    'g'
  );
  return new Map([...html.matchAll(pattern)].map(([, key, markup]) => [key, markup]));
}

describe('prerender', () => {
  const html = prerender();
  const shells = templates(html);

  it('renders the page the app renders first for every signed-out route', () => {
    expect(shells.get(shellKey('/login'))).toBe(renderRoute('/login'));
    expect(shells.get(shellKey('/login'))).toContain('Sign in to your account');
    expect(shells.get(shellKey('/register'))).toContain('Create your account');
    expect(shells.get(shellKey('/forgot-password'))).toContain('auth-page');
    expect(shells.get(shellKey('/unauthorized'))).toContain('Access Denied');
    expect(shells.get(shellKey('/invitations'))).toContain('Loading invitation...');
  });

  it('has a shell for every route the script can pick', () => {
    const keys = Object.values(config.routes).flatMap((route) =>
      config.tokenRoutes.includes(route)
        ? (['missing', 'invalid', 'valid'] as const).map((state) => shellKey(route, state))
        : [shellKey(route)]
    );
    expect([...new Set(keys)].sort()).toEqual([...shells.keys()].sort());
  });

  it('renders each token state of a token link', () => {
    expect(shells.get(shellKey('/reset-password', 'missing'))).toContain('No token provided');
    expect(shells.get(shellKey('/reset-password', 'invalid'))).toContain('Invalid token format');
    expect(shells.get(shellKey('/reset-password', 'valid'))).toContain('Set new password');
    expect(shells.get(shellKey('/verify-email', 'missing'))).toContain('No token provided');
    expect(shells.get(shellKey('/verify-email', 'invalid'))).toContain('Invalid token format');
    expect(shells.get(shellKey('/verify-email', 'valid'))).toContain('Verifying your email...');
  });

  it('renders signed out even when the build machine has a session stored', () => {
    localStorage.setItem(config.sessionKeys[0], 'token');
    try {
      expect(renderRoute('/login')).toContain('Sign in to your account');
    } finally {
      localStorage.clear();
    }
  });

  it('inlines the script that paints the shell', () => {
    expect(html).toMatch(
      /<script>\(function paintShell\(config\) \{[\s\S]+\}\)\(\{[\s\S]+\}\)<\/script>$/
    );
    expect(html).not.toMatch(/<script>[\s\S]*<\/[a-z]+>[\s\S]*<\/script>$/);
  });
});
