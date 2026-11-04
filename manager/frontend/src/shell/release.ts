import { SHELL_ATTRIBUTE, SHELL_ID } from './paint';

export const STARTUP_FAILURE_MESSAGE = 'Zone could not load. Reload the page to try again.';

export function releaseShell(target: Document = document): void {
  target.getElementById(SHELL_ID)?.remove();
  target.documentElement.removeAttribute(SHELL_ATTRIBUTE);
}

export function showStartupFailure(target: Document = document): void {
  releaseShell(target);
  const text = target.querySelector('#root > .initial-loader .loader-text');
  if (text) {
    text.textContent = STARTUP_FAILURE_MESSAGE;
  }
}

export function releaseShellOnRender(root: HTMLElement, target: Document = document): void {
  if (!target.getElementById(SHELL_ID)) {
    return;
  }
  const observer = new MutationObserver(() => {
    if (root.childElementCount > 0) {
      observer.disconnect();
      releaseShell(target);
    }
  });
  observer.observe(root, { childList: true });
}
