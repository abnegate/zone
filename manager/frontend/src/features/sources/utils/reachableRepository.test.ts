import { describe, expect, it } from 'bun:test';
import type {
  GitHubRepositoriesResponse,
  GitHubRepository,
  GitHubRepositoryInstallation,
} from '../../github/types';
import { reachableRepository } from './reachableRepository';

function installation(
  id: string,
  error: GitHubRepositoryInstallation['error'] = null
): GitHubRepositoryInstallation {
  return {
    id,
    account_login: `account-${id}`,
    html_url: `https://github.com/settings/installations/${id}`,
    error,
  };
}

let nextRepositoryId = 1;

function repository(fullName: string, installationId: string): GitHubRepository {
  const [owner, name] = fullName.split('/');
  return {
    installation_id: installationId,
    id: nextRepositoryId++,
    owner,
    name,
    full_name: fullName,
    private: false,
    archived: false,
    default_branch: 'main',
    html_url: `https://github.com/${fullName}`,
  };
}

function response(
  repositories: GitHubRepository[],
  installations: GitHubRepositoryInstallation[]
): GitHubRepositoriesResponse {
  return { repositories, installations, total: repositories.length, next_page: null };
}

describe('reachableRepository', () => {
  it('matches the full name case-insensitively', () => {
    const match = repository('Acme/API', 'installation-1');

    expect(
      reachableRepository(response([match], [installation('installation-1')]), 'acme/api')
    ).toBe(match);
  });

  it('ignores names that only contain the full name', () => {
    expect(
      reachableRepository(
        response(
          [
            repository('acme/api-gateway', 'installation-1'),
            repository('acme/apis', 'installation-1'),
          ],
          [installation('installation-1')]
        ),
        'acme/api'
      )
    ).toBeNull();
  });

  it('skips repositories whose installation has an error', () => {
    const reachable = repository('acme/api', 'installation-4');

    expect(
      reachableRepository(
        response(
          [
            repository('acme/api', 'installation-1'),
            repository('acme/api', 'installation-2'),
            repository('acme/api', 'installation-3'),
            reachable,
          ],
          [
            installation('installation-1', 'revoked'),
            installation('installation-2', 'suspended'),
            installation('installation-3', 'unavailable'),
            installation('installation-4'),
          ]
        ),
        'acme/api'
      )
    ).toBe(reachable);
  });

  it('skips repositories whose installation is missing from the response', () => {
    expect(
      reachableRepository(
        response([repository('acme/api', 'installation-9')], [installation('installation-1')]),
        'acme/api'
      )
    ).toBeNull();
  });

  it('takes the first reachable match in API order', () => {
    const first = repository('acme/api', 'installation-1');
    const second = repository('acme/api', 'installation-2');

    expect(
      reachableRepository(
        response([first, second], [installation('installation-1'), installation('installation-2')]),
        'acme/api'
      )
    ).toBe(first);
  });

  it('is null for an empty response', () => {
    expect(reachableRepository(response([], []), 'acme/api')).toBeNull();
  });
});
