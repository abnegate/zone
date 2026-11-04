import { SHELL_ATTRIBUTE, SHELL_ID } from './paint';

export function releaseShell(target: Document = document): void {
  target.getElementById(SHELL_ID)?.remove();
  target.documentElement.removeAttribute(SHELL_ATTRIBUTE);
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
