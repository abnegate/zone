import { describe, expect, it } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { overview } from '../../../test/analytics';
import { NOT_MEASURED } from '../AnalyticsFormat';
import { PULL_REQUESTS_TITLE, PullRequestSummaryCard } from './PullRequestSummaryCard';

const { pull_requests: pullRequests } = overview();

function figures(): (string | null)[] {
  const activity = screen.getByLabelText('Pull request activity');
  return within(activity)
    .getAllByRole('definition')
    .map((value) => value.textContent);
}

describe('PullRequestSummaryCard', () => {
  it('shows counts, merge rate, time to merge, review cycles and approvals', () => {
    render(<PullRequestSummaryCard pullRequests={pullRequests} />);

    expect(screen.getByRole('region', { name: PULL_REQUESTS_TITLE })).toBeDefined();
    expect(figures()).toEqual(['14', '9', '2', '3', '82%', '1h 35m', '1.6', '1.2']);
    expect(screen.getByRole('list', { name: 'Review quality' })).toBeDefined();
  });

  it('shows nothing measured rather than zero for figures with no merges behind them', () => {
    render(
      <PullRequestSummaryCard
        pullRequests={{
          ...pullRequests,
          merge_rate: null,
          median_minutes_to_merge: null,
          mean_review_cycles: null,
          mean_approvals: null,
        }}
      />
    );

    expect(figures().slice(4)).toEqual([NOT_MEASURED, NOT_MEASURED, NOT_MEASURED, NOT_MEASURED]);
  });
});
