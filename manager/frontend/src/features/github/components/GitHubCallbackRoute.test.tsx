import { describe, expect, it } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from 'react-router-dom';
import type { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { type PathLike, pathAfterAuth } from '../../../shared/lastPath';
import type { Organization } from '../../../types';
import type { useAuth } from '../../auth/context';
import GitHubCallbackRoute from './GitHubCallbackRoute';

type Auth = ReturnType<typeof useAuth>;
type Workspace = ReturnType<typeof useWorkspace>;

const CALLBACK_PATH = '/settings/organization/github';

const other: Organization = {
  id: 'org-other',
  name: 'Other',
  slug: 'other',
  description: null,
  is_active: true,
  role: 'admin',
  created_at: '',
  updated_at: '',
};

function auth(isAuthenticated: boolean): Auth {
  return {
    isAuthenticated,
    isLoading: false,
    hasPermission: () => true,
    hasAnyPermission: () => true,
    hasAllPermissions: () => true,
  } as unknown as Auth;
}

function workspace(signedIn: boolean, switched: Organization[]): Workspace {
  return {
    organizations: signedIn ? [other] : [],
    currentOrganization: null,
    currentWorkspace: null,
    workspaces: [],
    loading: false,
    resolvingRole: false,
    error: null,
    setCurrentOrganization: (organization) => switched.push(organization),
    setCurrentWorkspace: () => {},
    refreshOrganizations: async () => {},
    refreshWorkspaces: async () => {},
  };
}

function LocationProbe() {
  const location = useLocation();
  return <p data-testid="location">{`${location.pathname}${location.search}`}</p>;
}

function LoginProbe({ onSignIn }: { onSignIn: () => void }) {
  const location = useLocation();
  const navigate = useNavigate();
  const from = (location.state as { from?: PathLike } | null)?.from ?? null;
  return (
    <button
      type="button"
      onClick={() => {
        onSignIn();
        navigate(pathAfterAuth(from), { replace: true });
      }}
    >
      Sign in
    </button>
  );
}

function location(): string {
  return screen.getByTestId('location').textContent ?? '';
}

describe('GitHubCallbackRoute', () => {
  it('keeps the GitHub outcome of a signed-out callback through login', async () => {
    const switched: Organization[] = [];

    function Harness() {
      const [signedIn, setSignedIn] = useState(false);
      return (
        <Routes>
          <Route path="/login" element={<LoginProbe onSignIn={() => setSignedIn(true)} />} />
          <Route
            path="/settings/*"
            element={
              <GitHubCallbackRoute
                useAuthHook={() => auth(signedIn)}
                useWorkspaceHook={() => workspace(signedIn, switched)}
              >
                <p>Settings</p>
              </GitHubCallbackRoute>
            }
          />
        </Routes>
      );
    }

    render(
      <MemoryRouter initialEntries={[`${CALLBACK_PATH}?github=installed&organization=${other.id}`]}>
        <Harness />
        <LocationProbe />
      </MemoryRouter>
    );

    await waitFor(() => expect(location()).toBe('/login'));

    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));

    await waitFor(() => expect(location()).toBe(`${CALLBACK_PATH}?github=installed`));
    expect(switched).toEqual([other]);
    expect(screen.getByText('Settings')).toBeTruthy();
  });
});
