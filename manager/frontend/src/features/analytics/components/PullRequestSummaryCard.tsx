import { AnalyticsFormat } from '../AnalyticsFormat';
import { AnalyticsLabel } from '../AnalyticsLabel';
import type { PullRequestSummary } from '../types';
import { Figures } from './Figures';
import { SectionCard } from './SectionCard';
import { TallyBars } from './TallyBars';

export const PULL_REQUESTS_TITLE = 'Pull requests';

export interface PullRequestSummaryCardProps {
  pullRequests: PullRequestSummary;
}

export function PullRequestSummaryCard({ pullRequests }: PullRequestSummaryCardProps) {
  return (
    <SectionCard title={PULL_REQUESTS_TITLE} description="What agents opened and how review went">
      <Figures
        label="Pull request activity"
        figures={[
          { label: 'Opened', value: AnalyticsFormat.count(pullRequests.opened) },
          { label: 'Merged', value: AnalyticsFormat.count(pullRequests.merged) },
          { label: 'Closed', value: AnalyticsFormat.count(pullRequests.closed) },
          { label: 'Still open', value: AnalyticsFormat.count(pullRequests.open) },
          { label: 'Merge rate', value: AnalyticsFormat.share(pullRequests.merge_rate) },
          {
            label: 'Time to merge',
            value: AnalyticsFormat.minutes(pullRequests.median_minutes_to_merge),
          },
          {
            label: 'Review cycles',
            value: AnalyticsFormat.decimal(pullRequests.mean_review_cycles),
          },
          { label: 'Approvals', value: AnalyticsFormat.decimal(pullRequests.mean_approvals) },
        ]}
      />
      <TallyBars
        label="Review quality"
        tallies={pullRequests.quality}
        labelOf={AnalyticsLabel.quality}
        toneOf={AnalyticsLabel.qualityTone}
      />
    </SectionCard>
  );
}
