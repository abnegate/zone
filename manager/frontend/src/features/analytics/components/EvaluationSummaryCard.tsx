import { EmptyState } from '@zone/ui';
import { count } from '../format';
import { Tone } from '../labels';
import type { EvaluationSummary } from '../types';
import { SectionCard } from './SectionCard';

export const EVALUATIONS_TITLE = 'Evaluations';

export const NO_EVALUATIONS_TITLE = 'No evaluations yet';

export const NO_EVALUATIONS_DESCRIPTION =
  'Set ZONE_TASK_EVALUATION to measure whether each finished task improved, held or regressed the project.';

interface Verdict {
  key: keyof EvaluationSummary;
  label: string;
  tone: Tone;
}

const VERDICTS: readonly Verdict[] = [
  { key: 'improved', label: 'Improved', tone: Tone.Success },
  { key: 'unchanged', label: 'Unchanged', tone: Tone.Info },
  { key: 'regressed', label: 'Regressed', tone: Tone.Destructive },
  { key: 'not_measured', label: 'Not measured', tone: Tone.Neutral },
];

export interface EvaluationSummaryCardProps {
  evaluations: EvaluationSummary;
}

export function EvaluationSummaryCard({ evaluations }: EvaluationSummaryCardProps) {
  const total = VERDICTS.reduce((sum, verdict) => sum + evaluations[verdict.key], 0);
  return (
    <SectionCard
      title={EVALUATIONS_TITLE}
      description="How each finished task moved the project's own measures"
    >
      {total === 0 ? (
        <EmptyState
          className="analytics-empty"
          title={NO_EVALUATIONS_TITLE}
          description={NO_EVALUATIONS_DESCRIPTION}
        />
      ) : (
        <>
          <div className="analytics-split" aria-hidden="true">
            {VERDICTS.filter((verdict) => evaluations[verdict.key] > 0).map((verdict) => (
              <span
                key={verdict.key}
                className="analytics-split-part"
                data-tone={verdict.tone}
                style={{ flexGrow: evaluations[verdict.key] }}
              />
            ))}
          </div>
          <dl className="analytics-figures analytics-verdicts" aria-label={EVALUATIONS_TITLE}>
            {VERDICTS.map((verdict) => (
              <div key={verdict.key} className="analytics-figure" data-tone={verdict.tone}>
                <dt className="analytics-figure-label">
                  <span className="analytics-dot" aria-hidden="true" />
                  {verdict.label}
                </dt>
                <dd className="analytics-figure-value">{count(evaluations[verdict.key])}</dd>
              </div>
            ))}
          </dl>
        </>
      )}
    </SectionCard>
  );
}
