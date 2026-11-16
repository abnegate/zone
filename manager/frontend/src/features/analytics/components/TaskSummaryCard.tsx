import { count } from '../format';
import { originLabel, Tone, taskStatusLabel, taskStatusTone } from '../labels';
import type { TaskSummary } from '../types';
import { Figures } from './Figures';
import { SectionCard } from './SectionCard';
import { TallyBars } from './TallyBars';

export const TASKS_TITLE = 'Tasks';

const originTone = (): Tone => Tone.Accent;

export interface TaskSummaryCardProps {
  tasks: TaskSummary;
}

export function TaskSummaryCard({ tasks }: TaskSummaryCardProps) {
  return (
    <SectionCard title={TASKS_TITLE} description="Where the workspace's tasks stand" wide>
      <Figures
        label="Task activity"
        figures={[
          { label: 'Created', value: count(tasks.created) },
          { label: 'Shipped', value: count(tasks.shipped) },
          { label: 'Reopened', value: count(tasks.reopened) },
        ]}
      />
      <div className="analytics-columns">
        <TallyBars
          label="By status"
          tallies={tasks.statuses}
          labelOf={taskStatusLabel}
          toneOf={taskStatusTone}
        />
        <TallyBars
          label="By origin"
          tallies={tasks.origins}
          labelOf={originLabel}
          toneOf={originTone}
        />
      </div>
    </SectionCard>
  );
}
