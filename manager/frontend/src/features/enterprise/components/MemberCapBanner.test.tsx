import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import {
  COMMUNITY,
  COMMUNITY_AT_CAP,
  COMMUNITY_OVER_CAP,
  OVER_SEATS,
  TEAM,
  TEAM_LAPSED,
  UNCOUNTED_COMMUNITY,
} from '../fixtures';
import { PRICING_URL } from '../pricing';
import type { LicenseResponse } from '../types';

const get = mock();
const useAuth = mock();

mock.module('../api', () => ({ licenseApi: { get, set: mock(), remove: mock() } }));
mock.module('../../auth/context', () => ({ useAuth }));

let MemberCapBanner: typeof import('./MemberCapBanner').MemberCapBanner;
let LICENSE_PATH: string;

beforeAll(async () => {
  ({ MemberCapBanner } = await import('./MemberCapBanner'));
  ({ LICENSE_PATH } = await import('./PlanUpgradeNotice'));
});

afterAll(() => {
  mock.restore();
});

function renderBanner(response: LicenseResponse, admin: boolean, path = '/chats') {
  get.mockResolvedValue(response);
  useAuth.mockReturnValue({ user: { id: '1', email: 'admin@test.com', is_admin: admin } });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[path]}>
        <MemberCapBanner />
      </MemoryRouter>
    </QueryClientProvider>
  );
}

describe('MemberCapBanner', () => {
  beforeEach(() => {
    get.mockReset();
    useAuth.mockReset();
  });

  it('tells an admin the server is over the cap without anyone losing access', async () => {
    renderBanner(COMMUNITY_OVER_CAP, true);

    const banner = await screen.findByRole('status');
    expect(banner).toHaveClass('member-cap-banner');
    expect(banner).toHaveTextContent(
      '5 people were active this month, 2 more than the 3 members Zone Community allows'
    );
    expect(banner).toHaveTextContent('Everyone keeps access');
    expect(banner).not.toHaveTextContent(/block|lock|suspend|disable|lose|remov/i);
  });

  it('links an admin to the License settings and to the plans', async () => {
    renderBanner(COMMUNITY_OVER_CAP, true);

    await screen.findByRole('status');
    expect(screen.getByRole('link', { name: 'Go to License' })).toHaveAttribute(
      'href',
      LICENSE_PATH
    );
    const plans = screen.getByRole('link', { name: 'Compare plans (opens in a new tab)' });
    expect(plans).toHaveAttribute('href', PRICING_URL);
    expect(plans).toHaveAttribute('target', '_blank');
  });

  it('leaves the License page to its own over-cap alert', async () => {
    const { container } = renderBanner(COMMUNITY_OVER_CAP, true, LICENSE_PATH);

    await waitFor(() => expect(get).toHaveBeenCalledTimes(1));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(container.childElementCount).toBe(0);
  });

  it('shows on a lapsed license that has gone over the cap', async () => {
    renderBanner(TEAM_LAPSED, true);

    expect(await screen.findByRole('status')).toHaveTextContent('12 people were active');
  });

  for (const [name, response] of [
    ['under the cap', COMMUNITY],
    ['at the cap', COMMUNITY_AT_CAP],
    ['uncounted', UNCOUNTED_COMMUNITY],
    ['on Team', TEAM],
    ['over licensed seats but uncapped', OVER_SEATS],
  ] as const) {
    it(`stays hidden ${name}`, async () => {
      const { container } = renderBanner(response, true);

      await waitFor(() => expect(get).toHaveBeenCalledTimes(1));
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(container.childElementCount).toBe(0);
    });
  }

  it('never shows to someone who is not a server admin, and never asks for the license', async () => {
    const { container } = renderBanner(COMMUNITY_OVER_CAP, false);

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(container.childElementCount).toBe(0);
    expect(get).not.toHaveBeenCalled();
  });

  it('stays hidden while nobody is signed in', async () => {
    get.mockResolvedValue(COMMUNITY_OVER_CAP);
    useAuth.mockReturnValue({ user: null });
    const queryClient = new QueryClient();
    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter>
          <MemberCapBanner />
        </MemoryRouter>
      </QueryClientProvider>
    );

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(container.childElementCount).toBe(0);
    expect(get).not.toHaveBeenCalled();
  });
});
