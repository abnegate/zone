import { beforeEach, describe, expect, it } from 'bun:test';
import { SHELL_ATTRIBUTE, SHELL_ID } from './paint';
import { releaseShell, releaseShellOnRender } from './release';

function root(): HTMLElement {
  return document.getElementById('root') as HTMLElement;
}

function shell(): HTMLElement | null {
  return document.getElementById(SHELL_ID);
}

async function mutationsDelivered() {
  await Promise.resolve();
}

describe('releaseShell', () => {
  beforeEach(() => {
    document.documentElement.setAttribute(SHELL_ATTRIBUTE, '/login');
    document.body.innerHTML = `<div id="${SHELL_ID}"></div><div id="root"><div class="initial-loader"></div></div>`;
  });

  it('removes the shell and shows the loader again', () => {
    releaseShell();
    expect(shell()).toBeNull();
    expect(document.documentElement.hasAttribute(SHELL_ATTRIBUTE)).toBe(false);
  });

  it('keeps the shell while the app renders nothing, as on a redirect', async () => {
    releaseShellOnRender(root());
    root().replaceChildren();
    await mutationsDelivered();
    expect(shell()).not.toBeNull();
  });

  it('removes the shell once the app puts its page in the root', async () => {
    releaseShellOnRender(root());
    root().replaceChildren();
    await mutationsDelivered();
    root().append(document.createElement('div'));
    await mutationsDelivered();
    expect(shell()).toBeNull();
    expect(document.documentElement.hasAttribute(SHELL_ATTRIBUTE)).toBe(false);
  });

  it('removes the shell when the first render replaces the loader with a page', async () => {
    releaseShellOnRender(root());
    root().replaceChildren(document.createElement('div'));
    await mutationsDelivered();
    expect(shell()).toBeNull();
  });

  it('does nothing without a shell', async () => {
    shell()?.remove();
    releaseShellOnRender(root());
    root().replaceChildren(document.createElement('div'));
    await mutationsDelivered();
    expect(document.documentElement.getAttribute(SHELL_ATTRIBUTE)).toBe('/login');
  });
});
