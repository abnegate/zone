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

describe('TruncationNotice', () => {
  it('says nothing when neither runs nor pull requests were capped', () => {
    const { container } = render(
      <TruncationNotice
        overview={{ truncated: { runs: false, pull_requests: false }, measured_from: null }}
      />
    );

    expect(container.firstChild).toBeNull();
  });

  it('names the span the capped runs actually cover', () => {
    expect(MAXIMUM_RUNS).toBe(10_000);
    expect(runsNotice(MEASURED_FROM)).toBe('Showing the newest 10,000 runs, since Nov 13.');
  });

  it('dates the span on the UTC calendar the chart buckets use', () => {
    expect(runsNotice('2026-11-13T23:30:00')).toBe('Showing the newest 10,000 runs, since Nov 13.');
  });

  it('falls back to the period when the server named no start', () => {
    expect(runsNotice(null)).toBe('Showing the newest 10,000 runs in this period.');
  });

  it('words a pull request cap apart from a run cap', () => {
    expect(MAXIMUM_PULL_REQUESTS).toBe(10_000);
    expect(
      truncationNotices({ truncated: { runs: false, pull_requests: true }, measured_from: null })
    ).toEqual([PULL_REQUESTS_NOTICE]);
    expect(PULL_REQUESTS_NOTICE).toBe(
      'Pull request figures cover the newest 10,000 pull requests in this period.'
    );
  });

  it('says both when both were capped', () => {
    render(
      <TruncationNotice
        overview={{ truncated: { runs: true, pull_requests: true }, measured_from: MEASURED_FROM }}
      />
    );

    expect(screen.getByRole('status').textContent).toBe(
      `${runsNotice(MEASURED_FROM)} ${PULL_REQUESTS_NOTICE}`
    );
  });
});
