export const SHELL_ID = 'signed-out-shell';
export const SHELL_ATTRIBUTE = 'data-shell';
export const SHELL_TEMPLATE_ATTRIBUTE = 'data-shell-template';

export type TokenState = 'missing' | 'invalid' | 'valid';

export interface ShellConfig {
  id: string;
  attribute: string;
  templateAttribute: string;
  themeKey: string;
  systemDarkQuery: string;
  sessionKeys: string[];
  routes: Record<string, string>;
  tokenRoutes: string[];
  tokenPattern: string;
}

export function shellKey(route: string, token?: TokenState): string {
  return token ? `${route}?token=${token}` : route;
}

// Inlined into index.html by its source text, so it must not reference anything outside its body.
export function paintShell(config: ShellConfig): void {
  try {
    const storage = window.localStorage;
    if (config.sessionKeys.some((key) => storage.getItem(key))) {
      return;
    }
    const pathname = window.location.pathname.toLowerCase().replace(/\/+$/, '') || '/';
    const route = config.routes[pathname];
    if (!route) {
      return;
    }
    let key = route;
    if (config.tokenRoutes.includes(route)) {
      const token = new URLSearchParams(window.location.search).get('token');
      const state = !token
        ? 'missing'
        : new RegExp(config.tokenPattern).test(token)
          ? 'valid'
          : 'invalid';
      key = `${route}?token=${state}`;
    }
    const template = Array.from(
      document.querySelectorAll<HTMLTemplateElement>(`template[${config.templateAttribute}]`)
    ).find((candidate) => candidate.getAttribute(config.templateAttribute) === key);
    if (!template) {
      return;
    }
    const stored = storage.getItem(config.themeKey);
    const theme =
      stored === 'light' || stored === 'dark'
        ? stored
        : window.matchMedia(config.systemDarkQuery).matches
          ? 'dark'
          : 'light';
    const shell = document.createElement('div');
    shell.id = config.id;
    shell.setAttribute('inert', '');
    shell.append(template.content.cloneNode(true));
    document.documentElement.setAttribute('data-theme', theme);
    document.documentElement.setAttribute(config.attribute, key);
    document.body.prepend(shell);
  } catch {
    return;
  }
}
