import { AnalyticsFormat } from '../AnalyticsFormat';
import { AnalyticsLabel } from '../AnalyticsLabel';
import type { TaskSummary } from '../types';
import { Figures } from './Figures';
import { SectionCard } from './SectionCard';
import { TallyBars } from './TallyBars';

export const TASKS_TITLE = 'Tasks';

export interface TaskSummaryCardProps {
  tasks: TaskSummary;
}

export function TaskSummaryCard({ tasks }: TaskSummaryCardProps) {
  return (
    <SectionCard title={TASKS_TITLE} description="Where the workspace's tasks stand" wide>
      <Figures
        label="Task activity"
        figures={[
          { label: 'Created', value: AnalyticsFormat.count(tasks.created) },
          { label: 'Shipped', value: AnalyticsFormat.count(tasks.shipped) },
          { label: 'Reopened', value: AnalyticsFormat.count(tasks.reopened) },
        ]}
      />
      <div className="analytics-columns">
        <TallyBars
          label="By status"
          tallies={tasks.statuses}
          labelOf={AnalyticsLabel.taskStatus}
          toneOf={AnalyticsLabel.taskStatusTone}
        />
        <TallyBars
          label="By origin"
          tallies={tasks.origins}
          labelOf={AnalyticsLabel.origin}
          toneOf={AnalyticsLabel.originTone}
        />
      </div>
    </SectionCard>
  );
}
