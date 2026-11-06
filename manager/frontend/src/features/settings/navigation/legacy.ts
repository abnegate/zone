import { SettingsGroup, SettingsSection, settingsPath } from './sections';

const TAB_PARAMETER = 'tab';

const ORGANIZATION_TABS: ReadonlyMap<string, SettingsSection> = new Map([
  ['ai', SettingsSection.Ai],
  ['members', SettingsSection.Members],
  ['invitations', SettingsSection.Members],
  ['devices', SettingsSection.Devices],
  ['billing', SettingsSection.Billing],
  ['audit', SettingsSection.Audit],
]);

const WORKSPACE_TABS: ReadonlyMap<string, string> = new Map([
  ['theme', settingsPath(SettingsGroup.Workspace, SettingsSection.General)],
  ['ai', settingsPath(SettingsGroup.Workspace, SettingsSection.Ai)],
  ['features', settingsPath(SettingsGroup.Server, SettingsSection.Features)],
  ['folders', settingsPath(SettingsGroup.Workspace, SettingsSection.Folders)],
  ['members', settingsPath(SettingsGroup.Workspace, SettingsSection.Members)],
  ['devices', settingsPath(SettingsGroup.Server, SettingsSection.Connect)],
]);

export const SESSIONS_PATH = settingsPath(SettingsGroup.You, SettingsSection.Sessions);

function withoutTab(path: string, parameters: URLSearchParams, hash: string): string {
  parameters.delete(TAB_PARAMETER);
  const remaining = parameters.toString();
  return `${path}${remaining ? `?${remaining}` : ''}${hash}`;
}

export function legacyOrganizationPath(search: string, hash: string): string {
  const parameters = new URLSearchParams(search);
  const section = ORGANIZATION_TABS.get(parameters.get(TAB_PARAMETER) ?? '') ?? SettingsSection.Ai;
  return withoutTab(settingsPath(SettingsGroup.Organization, section), parameters, hash);
}

export function legacyWorkspacePath(search: string, hash: string): string | null {
  const parameters = new URLSearchParams(search);
  const path = WORKSPACE_TABS.get(parameters.get(TAB_PARAMETER) ?? '');
  return path === undefined ? null : withoutTab(path, parameters, hash);
}
