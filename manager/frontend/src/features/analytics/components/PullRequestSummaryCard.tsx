import { count, decimal, minutes, share } from '../format';
import { qualityLabel, qualityTone } from '../labels';
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
          { label: 'Opened', value: count(pullRequests.opened) },
          { label: 'Merged', value: count(pullRequests.merged) },
          { label: 'Closed', value: count(pullRequests.closed) },
          { label: 'Still open', value: count(pullRequests.open) },
          { label: 'Merge rate', value: share(pullRequests.merge_rate) },
          { label: 'Time to merge', value: minutes(pullRequests.median_minutes_to_merge) },
          { label: 'Review cycles', value: decimal(pullRequests.mean_review_cycles) },
          { label: 'Approvals', value: decimal(pullRequests.mean_approvals) },
        ]}
      />
      <TallyBars
        label="Review quality"
        tallies={pullRequests.quality}
        labelOf={qualityLabel}
        toneOf={qualityTone}
      />
    </SectionCard>
  );
}
