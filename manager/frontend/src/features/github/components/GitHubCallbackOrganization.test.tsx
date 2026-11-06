import { describe, expect, it } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';
import { type ReactNode, useState } from 'react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import type { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { Organization } from '../../../types';
import GitHubCallbackOrganization from './GitHubCallbackOrganization';

type Workspace = ReturnType<typeof useWorkspace>;

function organization(id: string): Organization {
  return {
    id,
    name: id,
    slug: id,
    description: null,
    is_active: true,
    role: 'admin',
    created_at: '',
    updated_at: '',
  };
}

const current = organization('org-current');
const other = organization('org-other');

function workspace(overrides: Partial<Workspace> = {}): Workspace {
  return {
    organizations: [current, other],
    currentOrganization: current,
    currentWorkspace: null,
    workspaces: [],
    loading: false,
    resolvingRole: false,
    error: null,
    setCurrentOrganization: () => {},
    setCurrentWorkspace: () => {},
    refreshOrganizations: async () => {},
    refreshWorkspaces: async () => {},
    ...overrides,
  };
}

function LocationProbe() {
  const location = useLocation();
  return <p data-testid="location">{`${location.pathname}${location.search}`}</p>;
}

function renderAt(entry: string, children: ReactNode, useWorkspaceHook: () => Workspace) {
  return render(
    <MemoryRouter initialEntries={[entry]}>
      <GitHubCallbackOrganization useWorkspaceHook={useWorkspaceHook}>
        {children}
      </GitHubCallbackOrganization>
      <LocationProbe />
    </MemoryRouter>
  );
}

function location(): string {
  return screen.getByTestId('location').textContent ?? '';
}

describe('GitHubCallbackOrganization', () => {
  it('renders the children untouched without an organization parameter', () => {
    const switched: Organization[] = [];
    const state = workspace({ setCurrentOrganization: (match) => switched.push(match) });
    renderAt('/org-settings?tab=github&github=installed', <p>Settings</p>, () => state);

    expect(screen.getByText('Settings')).toBeTruthy();
    expect(screen.queryByRole('status')).toBeNull();
    expect(location()).toBe('/org-settings?tab=github&github=installed');
    expect(switched).toEqual([]);
  });

  it('switches to a different known organization and strips only its parameter', async () => {
    const switched: Organization[] = [];
    const state = workspace({ setCurrentOrganization: (match) => switched.push(match) });
    renderAt(
      `/org-settings?tab=github&github=installed&organization=${other.id}`,
      <p>Settings</p>,
      () => state
    );

    await waitFor(() => expect(location()).toBe('/org-settings?tab=github&github=installed'));
    expect(switched).toEqual([other]);
    expect(screen.getByText('Settings')).toBeTruthy();
  });

  it('strips the parameter without switching when it names the current organization', async () => {
    const switched: Organization[] = [];
    const state = workspace({ setCurrentOrganization: (match) => switched.push(match) });
    renderAt(
      `/org-settings?tab=github&github=sync&organization=${current.id}`,
      <p>Settings</p>,
      () => state
    );

    await waitFor(() => expect(location()).toBe('/org-settings?tab=github&github=sync'));
    expect(switched).toEqual([]);
    expect(screen.getByText('Settings')).toBeTruthy();
  });

  it('drops the outcome too when the organization is unknown', async () => {
    const switched: Organization[] = [];
    const state = workspace({ setCurrentOrganization: (match) => switched.push(match) });
    renderAt(
      '/org-settings?tab=github&github=installed&organization=org-unknown',
      <p>Settings</p>,
      () => state
    );

    await waitFor(() => expect(location()).toBe('/org-settings?tab=github'));
    expect(switched).toEqual([]);
    expect(screen.getByText('Settings')).toBeTruthy();
  });

  it('waits while organizations are loading, then acts once they arrive', async () => {
    const switched: Organization[] = [];
    let state = workspace({
      loading: true,
      organizations: [],
      currentOrganization: null,
      setCurrentOrganization: (match) => switched.push(match),
    });
    const entry = `/org-settings?tab=github&github=installed&organization=${other.id}`;
    const view = renderAt(entry, <p>Settings</p>, () => state);

    expect(screen.getByRole('status').textContent).toBe('Switching organization…');
    expect(screen.queryByText('Settings')).toBeNull();
    expect(location()).toBe(entry);
    expect(switched).toEqual([]);

    state = workspace({ setCurrentOrganization: (match) => switched.push(match) });
    view.rerender(
      <MemoryRouter initialEntries={[entry]}>
        <GitHubCallbackOrganization useWorkspaceHook={() => state}>
          <p>Settings</p>
        </GitHubCallbackOrganization>
        <LocationProbe />
      </MemoryRouter>
    );

    await waitFor(() => expect(location()).toBe('/org-settings?tab=github&github=installed'));
    expect(switched).toEqual([other]);
    expect(screen.getByText('Settings')).toBeTruthy();
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('renders the children only once the switched organization is current', async () => {
    const seen: (string | undefined)[] = [];

    function useSwitchingWorkspace(): Workspace {
      const [currentOrganization, setCurrentOrganization] = useState<Organization | null>(current);
      return workspace({ currentOrganization, setCurrentOrganization });
    }

    function Harness() {
      const state = useSwitchingWorkspace();
      function Gate() {
        seen.push(state.currentOrganization?.id);
        return <p>Settings</p>;
      }
      return (
        <GitHubCallbackOrganization useWorkspaceHook={() => state}>
          <Gate />
        </GitHubCallbackOrganization>
      );
    }

    render(
      <MemoryRouter initialEntries={[`/org-settings?tab=github&organization=${other.id}`]}>
        <Harness />
        <LocationProbe />
      </MemoryRouter>
    );

    await waitFor(() => expect(screen.getByText('Settings')).toBeTruthy());
    expect(location()).toBe('/org-settings?tab=github');
    expect(seen.length).toBeGreaterThan(0);
    expect(new Set(seen)).toEqual(new Set([other.id]));
  });
});
