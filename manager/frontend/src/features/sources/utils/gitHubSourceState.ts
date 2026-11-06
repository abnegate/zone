import type { Source } from '../types';

export const GitHubSourceState = {
  App: 'app',
  Revoked: 'revoked',
  Suspended: 'suspended',
  Token: 'token',
  Disconnected: 'disconnected',
} as const;

export type GitHubSourceState = (typeof GitHubSourceState)[keyof typeof GitHubSourceState];

type GitHubSourceFields = Pick<
  Source,
  'github_installation_id' | 'github_installation_status' | 'has_credentials'
>;

export function isAppBacked(source: Pick<Source, 'github_installation_id'>): boolean {
  return source.github_installation_id !== null && source.github_installation_id !== undefined;
}

export function gitHubSourceState(source: GitHubSourceFields): GitHubSourceState {
  if (isAppBacked(source)) {
    switch (source.github_installation_status) {
      case 'revoked':
        return GitHubSourceState.Revoked;
      case 'suspended':
        return GitHubSourceState.Suspended;
      default:
        return GitHubSourceState.App;
    }
  }
  return source.has_credentials === false
    ? GitHubSourceState.Disconnected
    : GitHubSourceState.Token;
}
