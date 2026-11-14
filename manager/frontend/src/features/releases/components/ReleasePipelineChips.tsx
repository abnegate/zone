import { Badge } from '@zone/ui';
import type { Tint } from '../../github';
import type { Lookup, PipelineRun, ReleasePipeline } from '../types';
import { PIPELINE_LABELS, PIPELINE_TINTS } from '../utils/pipeline';

export const VISIBLE_RUNS = 3;

export const PIPELINE_LIST = 'Pipeline runs';

interface LookupChip {
  label: string;
  tint: Tint;
}

export const LOOKUP_CHIPS: Readonly<Record<Lookup, LookupChip>> = {
  pending: { label: 'Waiting for runs', tint: 'info' },
  unavailable: { label: 'Pipeline unavailable', tint: 'outline' },
  complete: { label: 'No runs', tint: 'neutral' },
};

export function runDescription(run: PipelineRun): string {
  return `${run.name}: ${PIPELINE_LABELS[run.state]}`;
}

export function hiddenRuns(count: number): string {
  return `${count} more ${count === 1 ? 'run' : 'runs'}`;
}

interface ReleasePipelineChipsProps {
  pipeline: ReleasePipeline;
}

export function ReleasePipelineChips({ pipeline }: ReleasePipelineChipsProps) {
  const { runs, lookup } = pipeline;

  if (runs.length === 0) {
    const chip = LOOKUP_CHIPS[lookup];
    return (
      <ul className="release-pipeline-chips" aria-label={PIPELINE_LIST}>
        <li>
          <Badge variant={chip.tint} className="release-pipeline-chip" title={chip.label}>
            <span className="release-pipeline-chip-text">{chip.label}</span>
          </Badge>
        </li>
      </ul>
    );
  }

  const shown = runs.slice(0, VISIBLE_RUNS);
  const hidden = runs.length - shown.length;

  return (
    <ul className="release-pipeline-chips" aria-label={PIPELINE_LIST}>
      {shown.map((run) => {
        const description = runDescription(run);
        return (
          <li key={run.id}>
            <a
              className="release-pipeline-link"
              href={run.url}
              target="_blank"
              rel="noreferrer"
              title={description}
              aria-label={description}
              data-state={run.state}
            >
              <Badge variant={PIPELINE_TINTS[run.state]} className="release-pipeline-chip">
                <span className="release-pipeline-chip-text">{run.name}</span>
              </Badge>
            </a>
          </li>
        );
      })}
      {hidden > 0 && (
        <li>
          <Badge variant="outline" className="release-pipeline-chip" title={hiddenRuns(hidden)}>
            <span aria-hidden="true">+{hidden}</span>
            <span className="sr-only">{hiddenRuns(hidden)}</span>
          </Badge>
        </li>
      )}
    </ul>
  );
}
