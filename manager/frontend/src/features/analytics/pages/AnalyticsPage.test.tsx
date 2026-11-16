import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { emptyOverview, overview } from '../../../test/analytics';
import { NO_TREND } from '../components';
import type { Analytics } from '../hooks';
import type { TimePeriod } from '../types';

const mockRetry = mock();
const requested: TimePeriod[] = [];

const IDLE: Omit<Analytics, 'retry'> = {
  overview: undefined,
  loading: false,
  refreshing: false,
  error: null,
  missingWorkspace: false,
};

let state: Omit<Analytics, 'retry'> = IDLE;

mock.module('../hooks', () => ({
  useAnalytics: (period: TimePeriod) => {
    requested.push(period);
    return { ...state, retry: mockRetry };
  },
}));

let page: typeof import('./AnalyticsPage');

beforeAll(async () => {
  page = await import('./AnalyticsPage');
});

afterAll(() => {
  mock.restore();
});

function LocationProbe() {
  const location = useLocation();
  return <p data-testid="location">{location.search}</p>;
}

function renderPage(entry = '/analytics') {
  const AnalyticsPage = page.default;
  return render(
    <MemoryRouter initialEntries={[entry]}>
      <AnalyticsPage />
      <LocationProbe />
    </MemoryRouter>
  );
}

function search(): string | null {
  return screen.getByTestId('location').textContent;
}

function region(name: string): HTMLElement {
  return screen.getByRole('region', { name });
}

describe('AnalyticsPage', () => {
  beforeEach(() => {
    state = IDLE;
    requested.length = 0;
    mockRetry.mockReset();
  });

  it('titles the page bar and offers the periods as tabs, a week by default', () => {
    state = { ...IDLE, overview: overview() };
    const { container } = renderPage();

    expect(screen.getByRole('heading', { level: 1, name: page.TITLE })).toBeDefined();
    const tabs = within(screen.getByRole('tablist', { name: page.PERIOD_LABEL }));
    expect(tabs.getAllByRole('tab').map((tab) => tab.textContent)).toEqual([
      '24 hours',
      '7 days',
      '30 days',
      '90 days',
    ]);
    expect(tabs.getByRole('tab', { name: '7 days' }).getAttribute('aria-selected')).toBe('true');
    expect(container.querySelector('.page-bar-actions .ui-tabs-list')).not.toBeNull();
    expect(requested.at(-1)).toBe('week');
  });

  it('asks for the period a tab names', () => {
    state = { ...IDLE, overview: overview() };
    renderPage();

    const month = screen.getByRole('tab', { name: '30 days' });
    fireEvent.mouseDown(month);
    fireEvent.click(month);

    expect(requested.at(-1)).toBe('month');
    expect(month.getAttribute('aria-selected')).toBe('true');
    expect(search()).toBe('?period=month');
  });

  it('offers the last 24 hours', () => {
    state = { ...IDLE, overview: overview() };
    renderPage();

    const day = screen.getByRole('tab', { name: '24 hours' });
    fireEvent.mouseDown(day);
    fireEvent.click(day);

    expect(requested.at(-1)).toBe('day');
    expect(search()).toBe('?period=day');
  });

  it('opens on the period the address names', () => {
    state = { ...IDLE, overview: overview({ period: 'quarter' }) };
    renderPage('/analytics?period=quarter');

    expect(requested.at(-1)).toBe('quarter');
    expect(screen.getByRole('tab', { name: '90 days' }).getAttribute('aria-selected')).toBe('true');
  });

  it('falls back to a week when the address names no period it knows', () => {
    state = { ...IDLE, overview: overview() };
    renderPage('/analytics?period=year');

    expect(requested.at(-1)).toBe('week');
    expect(screen.getByRole('tab', { name: '7 days' }).getAttribute('aria-selected')).toBe('true');
  });

  it('keeps other search params when a tab changes the period', () => {
    state = { ...IDLE, overview: overview() };
    renderPage('/analytics?period=month&from=sidebar');

    const quarter = screen.getByRole('tab', { name: '90 days' });
    fireEvent.mouseDown(quarter);
    fireEvent.click(quarter);

    expect(search()).toBe('?period=quarter&from=sidebar');
  });

  it('labels the numbers by the period they cover while the next period loads', () => {
    state = { ...IDLE, overview: emptyOverview(), refreshing: true };
    renderPage('/analytics?period=month');

    expect(requested.at(-1)).toBe('month');
    expect(within(region('Runs')).getByText(page.noRunsTitle('week'))).toBeDefined();
    expect(screen.queryByText(page.noRunsTitle('month'))).toBeNull();
  });

  it('labels the run chart by the period it covers while the next period loads', () => {
    state = { ...IDLE, overview: overview(), refreshing: true };
    renderPage('/analytics?period=quarter');

    expect(screen.getByRole('img', { name: /over the last 7 days$/ })).toBeDefined();
  });

  it('lays out tiles, the run chart and every section', () => {
    state = { ...IDLE, overview: overview() };
    renderPage();

    expect(screen.getAllByTestId('analytics-tile')).toHaveLength(6);
    expect(screen.getByRole('img', { name: /Finished runs by outcome/ })).toBeDefined();
    for (const name of [
      'Runs',
      'Failure kinds',
      'Sources',
      'Models',
      'Tasks',
      'Pull requests',
      'Evaluations',
    ]) {
      expect(region(name)).toBeDefined();
    }
    expect(within(region('Sources')).getByText('No source')).toBeDefined();
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('never mentions cost', () => {
    state = { ...IDLE, overview: overview() };
    const { container } = renderPage();

    expect(container.textContent?.toLowerCase()).not.toMatch(/cost|spend|\$/);
  });

  it('says what span the runs cover when they were capped', () => {
    state = {
      ...IDLE,
      overview: overview({
        truncated: { runs: true, pull_requests: false },
        measured_from: '2026-11-13T00:00:00',
      }),
    };
    renderPage();

    expect(screen.getByRole('status').textContent).toBe(
      'Showing the newest 10,000 runs, since Nov 13.'
    );
  });

  it('says the pull request figures were capped apart from the runs', () => {
    state = {
      ...IDLE,
      overview: overview({ truncated: { runs: false, pull_requests: true } }),
    };
    renderPage();

    expect(screen.getByRole('status').textContent).toBe(
      'Pull request figures cover the newest 10,000 pull requests in this period.'
    );
  });

  it('draws a truncated window from the runs it read, with no trend', () => {
    const full = overview();
    state = {
      ...IDLE,
      overview: overview({
        truncated: { runs: true, pull_requests: false },
        measured_from: full.runs.series[4].window.start,
        runs: {
          ...full.runs,
          series: full.runs.series.slice(4),
          trend: {
            direction: 'steady',
            earlier_success_rate: null,
            later_success_rate: null,
            change: 0,
          },
        },
      }),
    };
    const { container } = renderPage();

    expect(screen.getByRole('status').textContent).toBe(
      'Showing the newest 10,000 runs, since Nov 13.'
    );
    expect(container.querySelectorAll('.analytics-chart-bar')).toHaveLength(3);
    expect(screen.getByText(NO_TREND)).toBeDefined();
  });

  it('shows the loading skeleton before the first overview arrives', () => {
    state = { ...IDLE, loading: true };
    renderPage();

    expect(screen.getByRole('status', { name: 'Loading analytics' })).toBeDefined();
    expect(screen.queryAllByTestId('analytics-tile')).toHaveLength(0);
  });

  it('shows the error with a retry when nothing has loaded', () => {
    state = { ...IDLE, error: 'Failed to load analytics: period must be one of' };
    renderPage();

    expect(screen.getByText(page.FAILED_TITLE)).toBeDefined();
    expect(screen.getByText('Failed to load analytics: period must be one of')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: page.RETRY }));
    expect(mockRetry).toHaveBeenCalledTimes(1);
  });

  it('keeps the last numbers under a banner when a refresh fails', () => {
    state = { ...IDLE, overview: overview(), error: 'Failed to load analytics: 502' };
    renderPage();

    expect(screen.getByRole('alert').textContent).toBe('Failed to load analytics: 502');
    expect(screen.getAllByTestId('analytics-tile')).toHaveLength(6);
  });

  it('dims the last period while the next one loads', () => {
    state = { ...IDLE, overview: overview(), refreshing: true };
    const { container } = renderPage();

    expect(container.querySelector('.analytics-overview')?.getAttribute('aria-busy')).toBe('true');
  });

  it('says no runs happened instead of drawing an empty chart', () => {
    state = { ...IDLE, overview: emptyOverview() };
    renderPage();

    expect(within(region('Runs')).getByText(page.noRunsTitle('week'))).toBeDefined();
    expect(screen.queryByRole('img')).toBeNull();
    expect(screen.queryByRole('region', { name: 'Failure kinds' })).toBeNull();
    expect(region('Tasks')).toBeDefined();
    expect(region('Pull requests')).toBeDefined();
  });

  it('asks for a workspace when none is selected', () => {
    state = { ...IDLE, missingWorkspace: true };
    renderPage();

    expect(screen.getByText(page.NO_WORKSPACE_TITLE)).toBeDefined();
    expect(screen.getByText(page.NO_WORKSPACE_DESCRIPTION)).toBeDefined();
    for (const tab of screen.getAllByRole('tab')) {
      expect(tab.hasAttribute('disabled')).toBe(true);
    }
  });
});
