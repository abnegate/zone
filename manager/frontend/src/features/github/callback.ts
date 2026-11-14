import { SettingsGroup, SettingsSection, settingsPath } from '../settings/navigation/sections';

export const GitHubSearchParameter = {
  Outcome: 'github',
  Organization: 'organization',
} as const;

export const GITHUB_SETTINGS_PATH = settingsPath(
  SettingsGroup.Organization,
  SettingsSection.GitHub
);

export const GitHubOutcome = {
  Installed: 'installed',
  Requested: 'requested',
  Sync: 'sync',
  Expired: 'expired',
  Failed: 'failed',
} as const;

export type GitHubOutcome = (typeof GitHubOutcome)[keyof typeof GitHubOutcome];

const OUTCOMES: ReadonlySet<string> = new Set(Object.values(GitHubOutcome));

export function isGitHubOutcome(value: string | null): value is GitHubOutcome {
  return value !== null && OUTCOMES.has(value);
}
