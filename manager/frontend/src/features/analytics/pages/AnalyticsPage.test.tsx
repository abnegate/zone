import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { emptyOverview, overview } from '../../../test/analytics';
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

function renderPage() {
  const AnalyticsPage = page.default;
  return render(
    <MemoryRouter initialEntries={['/analytics']}>
      <AnalyticsPage />
    </MemoryRouter>
  );
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
    expect(screen.queryByText(page.TRUNCATED)).toBeNull();
  });

  it('never mentions cost', () => {
    state = { ...IDLE, overview: overview() };
    const { container } = renderPage();

    expect(container.textContent?.toLowerCase()).not.toMatch(/cost|spend|\$/);
  });

  it('warns when the window held more runs than were read', () => {
    state = { ...IDLE, overview: overview({ truncated: true }) };
    renderPage();

    expect(screen.getByRole('status').textContent).toBe(page.TRUNCATED);
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
