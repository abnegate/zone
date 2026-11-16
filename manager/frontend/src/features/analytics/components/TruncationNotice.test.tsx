import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import {
  MAXIMUM_PULL_REQUESTS,
  MAXIMUM_RUNS,
  PULL_REQUESTS_NOTICE,
  runsNotice,
  TruncationNotice,
  truncationNotices,
} from './TruncationNotice';

const MEASURED_FROM = '2026-11-13T00:00:00';
const WINDOW = { start: '2026-11-10T09:30:00', end: '2026-11-17T09:30:00' };

describe('TruncationNotice', () => {
  it('says nothing when neither runs nor pull requests were capped', () => {
    const { container } = render(
      <TruncationNotice
        overview={{
          truncated: { runs: false, pull_requests: false },
          measured_from: null,
          window: WINDOW,
        }}
      />
    );

    expect(container.firstChild).toBeNull();
  });

  it('names the span the capped runs actually cover', () => {
    expect(MAXIMUM_RUNS).toBe(10_000);
    expect(runsNotice(MEASURED_FROM, WINDOW.end)).toBe(
      'Showing the newest 10,000 runs, since Nov 13.'
    );
  });

  it('dates the span on the UTC calendar the chart buckets use', () => {
    expect(runsNotice('2026-11-13T23:30:00', WINDOW.end)).toBe(
      'Showing the newest 10,000 runs, since Nov 13.'
    );
  });

  it('falls back to the period when the server named no start', () => {
    expect(runsNotice(null, WINDOW.end)).toBe('Showing the newest 10,000 runs in this period.');
  });

  it('says the capped runs are too recent to chart when no complete bucket holds them', () => {
    expect(runsNotice(WINDOW.end, WINDOW.end)).toBe(
      'Showing the newest 10,000 runs, all too recent to chart yet.'
    );
  });

  it('words a pull request cap apart from a run cap', () => {
    expect(MAXIMUM_PULL_REQUESTS).toBe(10_000);
    expect(
      truncationNotices({
        truncated: { runs: false, pull_requests: true },
        measured_from: null,
        window: WINDOW,
      })
    ).toEqual([PULL_REQUESTS_NOTICE]);
    expect(PULL_REQUESTS_NOTICE).toBe(
      'Pull request figures cover the newest 10,000 pull requests in this period.'
    );
  });

  it('says both when both were capped', () => {
    render(
      <TruncationNotice
        overview={{
          truncated: { runs: true, pull_requests: true },
          measured_from: MEASURED_FROM,
          window: WINDOW,
        }}
      />
    );

    expect(screen.getByRole('status').textContent).toBe(
      `${runsNotice(MEASURED_FROM, WINDOW.end)} ${PULL_REQUESTS_NOTICE}`
    );
  });
});
