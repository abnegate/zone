import type { GitHubRepositoriesResponse, GitHubRepository } from '../../github/types';

export function reachableRepository(
  response: GitHubRepositoriesResponse,
  fullName: string
): GitHubRepository | null {
  const target = fullName.toLowerCase();
  const reachableInstallations = new Set(
    response.installations
      .filter((installation) => installation.error === null)
      .map((installation) => installation.id)
  );
  return (
    response.repositories.find(
      (repository) =>
        repository.full_name.toLowerCase() === target &&
        reachableInstallations.has(repository.installation_id)
    ) ?? null
  );
}
