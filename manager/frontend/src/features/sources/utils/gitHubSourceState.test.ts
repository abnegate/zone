import { describe, expect, it } from 'bun:test';
import { GitHubSourceState, gitHubSourceState, isAppBacked } from './gitHubSourceState';

describe('gitHubSourceState', () => {
  it('is app for an active installation', () => {
    expect(
      gitHubSourceState({
        github_installation_id: 'installation-1',
        github_installation_status: 'active',
        has_credentials: false,
      })
    ).toBe(GitHubSourceState.App);
  });

  it('is app for an installation whose status is null or missing', () => {
    expect(
      gitHubSourceState({
        github_installation_id: 'installation-1',
        github_installation_status: null,
      })
    ).toBe(GitHubSourceState.App);
    expect(gitHubSourceState({ github_installation_id: 'installation-1' })).toBe(
      GitHubSourceState.App
    );
  });

  it('is revoked or suspended from the installation status', () => {
    expect(
      gitHubSourceState({
        github_installation_id: 'installation-1',
        github_installation_status: 'revoked',
      })
    ).toBe(GitHubSourceState.Revoked);
    expect(
      gitHubSourceState({
        github_installation_id: 'installation-1',
        github_installation_status: 'suspended',
      })
    ).toBe(GitHubSourceState.Suspended);
  });

  it('prefers the installation over stored credentials', () => {
    expect(
      gitHubSourceState({
        github_installation_id: 'installation-1',
        github_installation_status: 'revoked',
        has_credentials: true,
      })
    ).toBe(GitHubSourceState.Revoked);
  });

  it('is token when credentials are stored or the field is missing', () => {
    expect(gitHubSourceState({ github_installation_id: null, has_credentials: true })).toBe(
      GitHubSourceState.Token
    );
    expect(gitHubSourceState({})).toBe(GitHubSourceState.Token);
  });

  it('is disconnected with neither an installation nor credentials', () => {
    expect(gitHubSourceState({ github_installation_id: null, has_credentials: false })).toBe(
      GitHubSourceState.Disconnected
    );
    expect(gitHubSourceState({ has_credentials: false })).toBe(GitHubSourceState.Disconnected);
  });

  it('ignores a status without an installation', () => {
    expect(
      gitHubSourceState({
        github_installation_id: null,
        github_installation_status: 'revoked',
        has_credentials: true,
      })
    ).toBe(GitHubSourceState.Token);
  });
});

describe('isAppBacked', () => {
  it('is true only for a non-null installation id', () => {
    expect(isAppBacked({ github_installation_id: 'installation-1' })).toBe(true);
    expect(isAppBacked({ github_installation_id: null })).toBe(false);
    expect(isAppBacked({})).toBe(false);
  });
});
