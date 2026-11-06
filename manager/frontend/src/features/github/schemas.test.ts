import { describe, expect, it } from 'bun:test';
import {
  GitHubAccountTypeSchema,
  GitHubAppOwnerTypeSchema,
  GitHubAppRegistrationSchema,
  GitHubAppSchema,
  GitHubBranchesResponseSchema,
  GitHubBranchSchema,
  GitHubConnectionSchema,
  GitHubInstallationSchema,
  GitHubInstallationUrlSchema,
  GitHubRepositoriesResponseSchema,
  GitHubRepositoryInstallationErrorSchema,
  GitHubRepositoryInstallationSchema,
  GitHubRepositorySchema,
  GitHubRepositorySelectionSchema,
} from './schemas';

const app = {
  id: 'app-1',
  app_id: 101,
  slug: 'zone-acme',
  name: 'Zone (acme)',
  owner_login: 'acme',
  owner_type: 'Organization',
  html_url: 'https://github.com/apps/zone-acme',
  settings_url: 'https://github.com/organizations/acme/settings/apps/zone-acme',
  created_at: '2026-11-01T00:00:00Z',
};

const installation = {
  id: 'inst-1',
  github_app_id: 'app-1',
  installation_id: 5001,
  account_login: 'acme',
  account_type: 'Organization',
  repository_selection: 'selected',
  html_url: 'https://github.com/organizations/acme/settings/installations/5001',
  suspended_at: '2026-11-02T00:00:00Z',
  revoked_at: '2026-11-03T00:00:00Z',
  created_at: '2026-11-01T00:05:00Z',
};

const repository = {
  installation_id: 'inst-1',
  id: 9001,
  owner: 'acme',
  name: 'zone',
  full_name: 'acme/zone',
  private: true,
  archived: false,
  default_branch: 'main',
  html_url: 'https://github.com/acme/zone',
};

const repositoryInstallation = {
  id: 'inst-1',
  account_login: 'acme',
  html_url: 'https://github.com/organizations/acme/settings/installations/5001',
  error: 'suspended',
};

const repositories = {
  repositories: [repository],
  installations: [repositoryInstallation],
  total: 51,
  next_page: 2,
};

const branches = {
  branches: [
    { name: 'main', protected: true },
    { name: 'feature/picker', protected: false },
  ],
  next_page: 2,
};

describe('GitHub enums', () => {
  it('names the owners an app can be registered on', () => {
    expect(GitHubAppOwnerTypeSchema.options).toEqual(['User', 'Organization']);
  });

  it('names the accounts an app can be installed on', () => {
    expect(GitHubAccountTypeSchema.options).toEqual(['User', 'Organization', 'Enterprise']);
  });

  it('names the repository selections GitHub reports', () => {
    expect(GitHubRepositorySelectionSchema.options).toEqual(['all', 'selected']);
  });

  it('names why an installation cannot list repositories', () => {
    expect(GitHubRepositoryInstallationErrorSchema.options).toEqual([
      'revoked',
      'suspended',
      'unavailable',
    ]);
  });
});

describe('GitHubAppSchema', () => {
  it('parses a full app', () => {
    expect(GitHubAppSchema.parse(app)).toEqual(app);
  });

  it('accepts an app registered on a user', () => {
    expect(GitHubAppSchema.safeParse({ ...app, owner_type: 'User' }).success).toBe(true);
  });

  it('refuses an app registered on an enterprise', () => {
    expect(GitHubAppSchema.safeParse({ ...app, owner_type: 'Enterprise' }).success).toBe(false);
  });

  it('refuses an app whose GitHub id is not a whole number', () => {
    expect(GitHubAppSchema.safeParse({ ...app, app_id: '101' }).success).toBe(false);
    expect(GitHubAppSchema.safeParse({ ...app, app_id: 101.5 }).success).toBe(false);
  });
});

describe('GitHubInstallationSchema', () => {
  it('parses a full installation', () => {
    expect(GitHubInstallationSchema.parse(installation)).toEqual(installation);
  });

  it('accepts an installation that is neither suspended nor revoked', () => {
    const active = { ...installation, suspended_at: null, revoked_at: null };
    expect(GitHubInstallationSchema.parse(active)).toEqual(active);
  });

  it('accepts an installation on an enterprise', () => {
    expect(
      GitHubInstallationSchema.safeParse({ ...installation, account_type: 'Enterprise' }).success
    ).toBe(true);
  });

  it('refuses an installation without the app it belongs to', () => {
    const { github_app_id: _, ...orphan } = installation;
    expect(GitHubInstallationSchema.safeParse(orphan).success).toBe(false);
  });

  it('refuses a repository selection GitHub does not report', () => {
    expect(
      GitHubInstallationSchema.safeParse({ ...installation, repository_selection: 'none' }).success
    ).toBe(false);
  });
});

describe('GitHubConnectionSchema', () => {
  it('parses the apps and installations of an organization', () => {
    const connection = { apps: [app], installations: [installation] };
    expect(GitHubConnectionSchema.parse(connection)).toEqual(connection);
  });

  it('parses an organization with nothing connected', () => {
    expect(GitHubConnectionSchema.parse({ apps: [], installations: [] })).toEqual({
      apps: [],
      installations: [],
    });
  });
});

describe('GitHubAppRegistrationSchema', () => {
  it('parses the manifest form the browser posts to GitHub', () => {
    const registration = {
      action: 'https://github.com/organizations/acme/settings/apps/new?state=state-1',
      manifest: '{"name":"Zone"}',
      state: 'state-1',
    };
    expect(GitHubAppRegistrationSchema.parse(registration)).toEqual(registration);
  });

  it('refuses a registration without a manifest', () => {
    expect(
      GitHubAppRegistrationSchema.safeParse({ action: 'https://github.com', state: 'state-1' })
        .success
    ).toBe(false);
  });
});

describe('GitHubInstallationUrlSchema', () => {
  it('parses the url that starts an installation', () => {
    const url = { url: 'https://github.com/apps/zone-acme/installations/new?state=state-2' };
    expect(GitHubInstallationUrlSchema.parse(url)).toEqual(url);
  });
});

describe('GitHubRepositorySchema', () => {
  it('parses a full repository', () => {
    expect(GitHubRepositorySchema.parse(repository)).toEqual(repository);
  });

  it('refuses a repository whose installation id is a number', () => {
    expect(GitHubRepositorySchema.safeParse({ ...repository, installation_id: 5001 }).success).toBe(
      false
    );
  });

  it('refuses a repository whose visibility is not a boolean', () => {
    expect(GitHubRepositorySchema.safeParse({ ...repository, private: 'true' }).success).toBe(
      false
    );
  });
});

describe('GitHubRepositoryInstallationSchema', () => {
  it('accepts an installation that lists its repositories', () => {
    const healthy = { ...repositoryInstallation, error: null };
    expect(GitHubRepositoryInstallationSchema.parse(healthy)).toEqual(healthy);
  });

  it('refuses an error it does not know', () => {
    expect(
      GitHubRepositoryInstallationSchema.safeParse({ ...repositoryInstallation, error: 'gone' })
        .success
    ).toBe(false);
  });
});

describe('GitHubRepositoriesResponseSchema', () => {
  it('parses a page of repositories with the next page', () => {
    expect(GitHubRepositoriesResponseSchema.parse(repositories)).toEqual(repositories);
  });

  it('accepts the last page', () => {
    const last = { ...repositories, next_page: null };
    expect(GitHubRepositoriesResponseSchema.parse(last)).toEqual(last);
  });

  it('refuses a next page sent as a string', () => {
    expect(
      GitHubRepositoriesResponseSchema.safeParse({ ...repositories, next_page: '2' }).success
    ).toBe(false);
  });

  it('refuses a total that is not a whole number', () => {
    expect(
      GitHubRepositoriesResponseSchema.safeParse({ ...repositories, total: 1.5 }).success
    ).toBe(false);
  });
});

describe('GitHubBranchesResponseSchema', () => {
  it('parses a page of branches with the next page', () => {
    expect(GitHubBranchesResponseSchema.parse(branches)).toEqual(branches);
  });

  it('accepts the last page', () => {
    const last = { ...branches, next_page: null };
    expect(GitHubBranchesResponseSchema.parse(last)).toEqual(last);
  });

  it('refuses a next page sent as a string', () => {
    expect(GitHubBranchesResponseSchema.safeParse({ ...branches, next_page: '2' }).success).toBe(
      false
    );
  });

  it('refuses a branch without its protection flag', () => {
    expect(GitHubBranchSchema.safeParse({ name: 'main' }).success).toBe(false);
  });
});
