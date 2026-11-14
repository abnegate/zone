import type { WorkspaceMcpServer } from './types';

export const Override = {
  Inherit: 'inherit',
  On: 'on',
  Off: 'off',
} as const;

export type Override = (typeof Override)[keyof typeof Override];

export const OVERRIDE_LABELS: Readonly<Record<Override, string>> = {
  [Override.Inherit]: 'Inherit',
  [Override.On]: 'On',
  [Override.Off]: 'Off',
};

export const OVERRIDES: readonly Override[] = [Override.Inherit, Override.On, Override.Off];

export function overrideOf(server: Pick<WorkspaceMcpServer, 'workspace_enabled'>): Override {
  if (server.workspace_enabled === null) {
    return Override.Inherit;
  }
  return server.workspace_enabled ? Override.On : Override.Off;
}

export function overrideValue(override: Override): boolean | null {
  switch (override) {
    case Override.Inherit:
      return null;
    case Override.On:
      return true;
    case Override.Off:
      return false;
  }
}
