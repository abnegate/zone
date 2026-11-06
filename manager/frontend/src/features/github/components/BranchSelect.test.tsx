import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { type ComponentType, useState } from 'react';
import type { GitHubBranch, GitHubBranchesResponse, GitHubRepository } from '../types';

const listBranches =
  mock<
    (
      organizationId: string,
      installationId: string,
      owner: string,
      repository: string,
      page?: number
    ) => Promise<GitHubBranchesResponse>
  >();

mock.module('../../../api/github', () => ({
  githubApi: {
    getConnection: mock(),
    registerApp: mock(),
    createInstallation: mock(),
    syncInstallations: mock(),
    removeInstallation: mock(),
    deleteApp: mock(),
    listRepositories: mock(),
    listBranches,
  },
}));

interface BranchSelectProps {
  organizationId: string | null;
  repository: GitHubRepository | null;
  value: string;
  onChange: (branch: string) => void;
}

let BranchSelect: ComponentType<BranchSelectProps>;

beforeAll(async () => {
  const module = (await import('./BranchSelect')) as {
    BranchSelect?: ComponentType<BranchSelectProps>;
    default?: ComponentType<BranchSelectProps>;
  };
  const component = module.BranchSelect ?? module.default;
  if (component === undefined) {
    throw new Error('BranchSelect.tsx exports no BranchSelect component');
  }
  BranchSelect = component;
});

afterAll(() => {
  mock.restore();
});

const repository: GitHubRepository = {
  installation_id: 'installation-1',
  id: 42,
  owner: 'acme',
  name: 'api',
  full_name: 'acme/api',
  private: false,
  archived: false,
  default_branch: 'trunk',
  html_url: 'https://github.com/acme/api',
};

function branches(...names: string[]): GitHubBranchesResponse {
  return {
    branches: names.map((name): GitHubBranch => ({ name, protected: name === 'trunk' })),
    next_page: null,
  };
}

interface RenderOptions {
  organizationId?: string | null;
  repository?: GitHubRepository | null;
  initialValue?: string;
}

function renderSelect({
  organizationId = 'org-1',
  repository: selected = repository,
  initialValue = '',
}: RenderOptions = {}) {
  const onChange = mock<(branch: string) => void>();
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });

  function Harness() {
    const [value, setValue] = useState(initialValue);
    return (
      <BranchSelect
        organizationId={organizationId}
        repository={selected}
        value={value}
        onChange={(branch) => {
          setValue(branch);
          onChange(branch);
        }}
      />
    );
  }

  const user = userEvent.setup();
  render(
    <QueryClientProvider client={queryClient}>
      <Harness />
    </QueryClientProvider>
  );
  return { user, onChange };
}

function branchInput(): HTMLInputElement {
  return screen.getByLabelText('Branch') as HTMLInputElement;
}

function datalistOptions(): string[] {
  const listId = branchInput().getAttribute('list') ?? '';
  expect(listId).not.toBe('');
  const datalist = document.getElementById(listId);
  expect(datalist?.tagName).toBe('DATALIST');
  return Array.from(datalist?.querySelectorAll('option') ?? []).map(
    (option) => option.getAttribute('value') ?? ''
  );
}

beforeEach(() => {
  listBranches.mockReset();
  listBranches.mockImplementation(async () => branches('main', 'develop', 'trunk'));
});

describe('BranchSelect', () => {
  it('is disabled and fetches nothing without a repository', async () => {
    renderSelect({ repository: null });

    const input = branchInput();
    expect(input).toBeDisabled();
    expect(input).toHaveAttribute('placeholder', 'main');
    await new Promise((settle) => setTimeout(settle, 50));
    expect(listBranches).not.toHaveBeenCalled();
  });

  it('loads the first page of branches for the repository installation', async () => {
    renderSelect();

    await waitFor(() =>
      expect(listBranches).toHaveBeenCalledWith('org-1', 'installation-1', 'acme', 'api', 1)
    );
    expect(listBranches).toHaveBeenCalledTimes(1);
    expect(branchInput()).toBeEnabled();
    expect(branchInput()).toHaveAttribute('placeholder', 'trunk');
  });

  it('lists the default branch first when the API returns it late', async () => {
    renderSelect();

    await waitFor(() => expect(datalistOptions()).toEqual(['trunk', 'main', 'develop']));
  });

  it('lists the default branch first when the API omits it', async () => {
    listBranches.mockImplementation(async () => branches('main', 'develop'));
    renderSelect();

    await waitFor(() => expect(datalistOptions()).toEqual(['trunk', 'main', 'develop']));
  });

  it('accepts a branch typed as free text', async () => {
    const { user, onChange } = renderSelect();

    await user.type(branchInput(), 'feature/x');

    expect(onChange).toHaveBeenLastCalledWith('feature/x');
    expect(branchInput()).toHaveValue('feature/x');
  });

  it('shows the value it is given without changing it', async () => {
    const { onChange } = renderSelect({ initialValue: 'release' });

    await waitFor(() => expect(datalistOptions()).toContain('trunk'));
    expect(branchInput()).toHaveValue('release');
    expect(onChange).not.toHaveBeenCalled();
  });
});
