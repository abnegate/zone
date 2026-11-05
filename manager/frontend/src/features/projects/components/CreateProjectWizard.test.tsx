import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { clientModule } from '../../../test/client';
import type { Source } from '../../../types';
import type { WorkspaceRole } from '../../auth/types';
import type { CreateProjectRequest, Project } from '../types';

const existing: Source = {
  id: 'source-jbs',
  name: 'jbs',
  source_type: 'filesystem',
  category: 'file',
  config: { base_path: '/srv/jbs', allow_writes: true },
  url: null,
  description: null,
  is_active: true,
  last_verified_at: null,
  last_error: null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
};

const created: Source = {
  ...existing,
  id: 'source-zone',
  name: 'abnegate/zone',
  source_type: 'github',
  config: { owner: 'abnegate', repo: 'zone', branch: 'main' },
  url: 'https://github.com/abnegate/zone',
};

let role: WorkspaceRole = 'member';

const mockGetSources = mock(() => Promise.resolve([existing]));
const mockCreateSource = mock(() => Promise.resolve(created));
const mockGetWorkspaceMembers = mock(() =>
  Promise.resolve({ members: [{ user_id: 'user-1', role }] })
);
const mockGetSourceTypes = mock(() =>
  Promise.resolve(
    (['github', 'gitlab', 'filesystem', 'web', 'text'] as const).map((id) => ({
      id,
      name: id,
      category: 'file' as const,
      enabled: true,
    }))
  )
);

mock.module('../../../api/client', () =>
  clientModule({
    getSources: mockGetSources,
    createSource: mockCreateSource,
    getWorkspaceMembers: mockGetWorkspaceMembers,
  })
);

mock.module('../../../api/sources', () => ({
  sourcesApi: { getSourceTypes: mockGetSourceTypes },
}));

mock.module('../../auth', () => ({
  useAuth: () => ({ isAuthenticated: true, user: { id: 'user-1' } }),
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({ currentWorkspace: { id: 'workspace-1', name: 'Workspace' } }),
}));

let CreateProjectWizard: typeof import('./CreateProjectWizard').CreateProjectWizard;

beforeAll(async () => {
  ({ CreateProjectWizard } = await import('./CreateProjectWizard'));
});

afterAll(() => {
  mock.restore();
});

const mockCreateProject = mock((request: CreateProjectRequest) =>
  Promise.resolve({ id: 'project-1', name: request.name } as Project)
);
const mockClose = mock(() => {});

function renderWizard() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  queryClient.setQueryData(['sources', 'workspace-1'], [existing]);
  render(
    <QueryClientProvider client={queryClient}>
      <CreateProjectWizard
        isOpen
        onClose={mockClose}
        onCreated={() => {}}
        createProject={mockCreateProject}
      />
    </QueryClientProvider>
  );
  return queryClient;
}

const projectDialog = () => screen.getByRole('dialog', { name: 'New Project' });
const sourceDialog = () => screen.queryByRole('dialog', { name: 'Add Source' });
const option = (name: string | RegExp) =>
  within(projectDialog()).getByRole('button', { name: new RegExp(name) });

async function reachSourceStep(user: ReturnType<typeof userEvent.setup>) {
  await user.type(within(projectDialog()).getByLabelText('Project Name'), 'Console');
  await user.type(within(projectDialog()).getByLabelText(/Description/), 'Operator console');
  await user.click(within(projectDialog()).getByRole('button', { name: 'Next' }));
  await within(projectDialog()).findByRole('button', { name: /jbs/ });
}

async function openNewSource(user: ReturnType<typeof userEvent.setup>) {
  await user.click(option('New source'));
  await waitFor(() => expect(sourceDialog()).not.toBeNull());
  return sourceDialog() as HTMLElement;
}

async function addGitHubSource(user: ReturnType<typeof userEvent.setup>, nested: HTMLElement) {
  await within(nested).findByRole('button', { name: /GitHub/ });
  await user.click(within(nested).getByRole('button', { name: 'Next' }));
  await user.type(await within(nested).findByLabelText(/Owner/), 'abnegate');
  await user.type(within(nested).getByLabelText(/Repository/), 'zone');
  await user.click(within(nested).getByRole('button', { name: 'Next' }));
  await within(nested).findByLabelText(/Display Name/);
  await user.click(within(nested).getByRole('button', { name: 'Add Source' }));
}

describe('CreateProjectWizard new source', () => {
  beforeEach(() => {
    role = 'member';
    mockGetSources.mockReset();
    mockGetSources.mockImplementation(() => Promise.resolve([existing]));
    mockCreateSource.mockClear();
    mockCreateProject.mockClear();
    mockClose.mockClear();
  });

  it('offers a new source card to a member', async () => {
    const user = userEvent.setup();
    renderWizard();
    await reachSourceStep(user);

    expect(await within(projectDialog()).findByRole('button', { name: /New source/ })).toBeTruthy();
  });

  it('hides the new source card from a viewer', async () => {
    role = 'viewer';
    const user = userEvent.setup();
    renderWizard();
    await reachSourceStep(user);
    await waitFor(() => expect(mockGetWorkspaceMembers).toHaveBeenCalled());

    expect(within(projectDialog()).queryByRole('button', { name: /New source/ })).toBeNull();
  });

  it('puts the new source card right after No Source in an empty workspace', async () => {
    mockGetSources.mockImplementation(() => Promise.resolve([]));
    const user = userEvent.setup();
    renderWizard();
    await user.type(within(projectDialog()).getByLabelText('Project Name'), 'Console');
    await user.click(within(projectDialog()).getByRole('button', { name: 'Next' }));
    await within(projectDialog()).findByRole('button', { name: /New source/ });

    const names = Array.from(
      projectDialog().querySelectorAll('.source-selection-option .source-selection-name')
    ).map((node) => node.textContent);
    expect(names).toEqual(['No Source', 'New source']);
    expect(projectDialog().querySelector('.wizard-empty-state')).toBeNull();
  });

  it('selects the created source, lists it without a reload, refreshes the sources query and keeps every project field', async () => {
    const user = userEvent.setup();
    const queryClient = renderWizard();
    await reachSourceStep(user);
    const nested = await openNewSource(user);
    await addGitHubSource(user, nested);

    await waitFor(() => expect(sourceDialog()).toBeNull());
    expect(mockCreateSource).toHaveBeenCalledWith(
      'workspace-1',
      expect.objectContaining({
        name: 'abnegate/zone',
        source_type: 'github',
        config: { owner: 'abnegate', repo: 'zone', branch: 'main' },
      })
    );
    expect(mockGetSources).toHaveBeenCalledTimes(1);
    expect(queryClient.getQueryState(['sources', 'workspace-1'])?.isInvalidated).toBe(true);
    expect(option('jbs').className).not.toContain('selected');
    expect(option('abnegate/zone').className).toContain('selected');
    expect(document.activeElement).toBe(option('abnegate/zone'));

    await user.click(within(projectDialog()).getByRole('button', { name: 'Previous' }));
    expect(await within(projectDialog()).findByLabelText('Project Name')).toHaveValue('Console');
    expect(within(projectDialog()).getByLabelText(/Description/)).toHaveValue('Operator console');

    await user.click(within(projectDialog()).getByRole('button', { name: 'Next' }));
    await within(projectDialog()).findByRole('button', { name: /abnegate\/zone/ });
    await user.click(within(projectDialog()).getByRole('button', { name: 'Next' }));
    await user.click(
      await within(projectDialog()).findByRole('button', { name: 'Create Project' })
    );

    await waitFor(() => expect(mockCreateProject).toHaveBeenCalledTimes(1));
    expect(mockCreateProject).toHaveBeenCalledWith(
      expect.objectContaining({
        name: 'Console',
        description: 'Operator console',
        source_id: 'source-zone',
        workspace_id: 'workspace-1',
      })
    );
  });

  it('keeps the previous selection and creates nothing when source creation is cancelled', async () => {
    const user = userEvent.setup();
    renderWizard();
    await reachSourceStep(user);
    await user.click(option('jbs'));
    const nested = await openNewSource(user);

    await user.click(within(nested).getByRole('button', { name: 'Cancel' }));

    await waitFor(() => expect(sourceDialog()).toBeNull());
    expect(mockCreateSource).not.toHaveBeenCalled();
    expect(option('jbs').className).toContain('selected');
    expect(within(projectDialog()).queryByRole('button', { name: /abnegate/ })).toBeNull();
    expect(document.activeElement).toBe(option('New source'));
  });

  it('closes only the source wizard on Escape and hands focus back to the card', async () => {
    const user = userEvent.setup();
    renderWizard();
    await reachSourceStep(user);
    await openNewSource(user);

    await user.keyboard('{Escape}');

    await waitFor(() => expect(sourceDialog()).toBeNull());
    expect(mockClose).not.toHaveBeenCalled();
    expect(projectDialog().hasAttribute('inert')).toBe(false);
    expect(document.activeElement).toBe(option('New source'));

    await user.keyboard('{Escape}');
    expect(mockClose).toHaveBeenCalledTimes(1);
  });

  it('hands focus back to the card even when clicking it never focused it, as in Safari', async () => {
    const user = userEvent.setup();
    renderWizard();
    await reachSourceStep(user);
    fireEvent.click(option('New source'));
    await waitFor(() => expect(sourceDialog()).not.toBeNull());

    fireEvent.keyDown(document, { key: 'Escape' });

    await waitFor(() => expect(sourceDialog()).toBeNull());
    expect(document.activeElement).toBe(option('New source'));
  });

  it('holds focus inside the source wizard while the project wizard sits inert beneath it', async () => {
    const user = userEvent.setup();
    renderWizard();
    await reachSourceStep(user);
    const nested = await openNewSource(user);

    expect(projectDialog().hasAttribute('inert')).toBe(true);
    expect(nested.contains(document.activeElement)).toBe(true);

    const nextButton = within(nested).getByRole('button', { name: 'Next' });
    nextButton.focus();
    await user.tab();
    expect(nested.contains(document.activeElement)).toBe(true);
    expect(document.activeElement).not.toBe(nextButton);

    within(nested).getAllByRole('button', { name: 'Close wizard' })[0]?.focus();
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(nextButton);
  });
});
