import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import {
  createMemoryRouter,
  MemoryRouter,
  NavigationType,
  Route,
  RouterProvider,
  Routes,
  useLocation,
} from 'react-router-dom';
import SettingsRedirect, { LegacySettings } from './SettingsRedirect';

function LocationProbe() {
  const { pathname, search, hash } = useLocation();
  return (
    <output data-testid="location">
      {pathname}
      {search}
      {hash}
    </output>
  );
}

function renderAt(entry: string) {
  render(
    <MemoryRouter initialEntries={[entry]}>
      <Routes>
        <Route
          path="/org-settings"
          element={<SettingsRedirect from={LegacySettings.Organization} />}
        />
        <Route path="/sessions" element={<SettingsRedirect from={LegacySettings.Sessions} />} />
        <Route path="*" element={<LocationProbe />} />
      </Routes>
    </MemoryRouter>
  );
}

describe('SettingsRedirect', () => {
  it('sends the bare organization settings page to organization AI defaults', () => {
    renderAt('/org-settings');

    expect(screen.getByTestId('location').textContent).toBe('/settings/organization/ai');
  });

  it('maps the legacy tab and keeps the remaining query and hash', () => {
    renderAt('/org-settings?tab=invitations&x=1#y');

    expect(screen.getByTestId('location').textContent).toBe('/settings/organization/members?x=1#y');
  });

  it('sends an unknown organization tab to AI defaults', () => {
    renderAt('/org-settings?tab=unknown');

    expect(screen.getByTestId('location').textContent).toBe('/settings/organization/ai');
  });

  it('sends the sessions page to the You sessions section', () => {
    renderAt('/sessions');

    expect(screen.getByTestId('location').textContent).toBe('/settings/you/sessions');
  });

  it('replaces the legacy entry instead of pushing a new one', () => {
    const router = createMemoryRouter(
      [
        { path: '/', element: <LocationProbe /> },
        { path: '/org-settings', element: <SettingsRedirect from={LegacySettings.Organization} /> },
        { path: '*', element: <LocationProbe /> },
      ],
      { initialEntries: ['/', '/org-settings?tab=billing'], initialIndex: 1 }
    );

    render(<RouterProvider router={router} />);

    expect(router.state.location.pathname).toBe('/settings/organization/billing');
    expect(router.state.historyAction).toBe(NavigationType.Replace);
  });
});
