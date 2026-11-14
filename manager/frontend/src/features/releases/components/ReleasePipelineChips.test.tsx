import { describe, expect, it } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { pipelineRun, release } from '../../../test/releases';
import {
  hiddenRuns,
  LOOKUP_CHIPS,
  PIPELINE_LIST,
  ReleasePipelineChips,
  runDescription,
} from './ReleasePipelineChips';

function chips() {
  return within(screen.getByRole('list', { name: PIPELINE_LIST }));
}

describe('ReleasePipelineChips', () => {
  it('links each run to its workflow page, tinted by its state', () => {
    const failed = pipelineRun({ id: 8, name: 'Publish', state: 'failed' });
    const subject = release({ runs: [pipelineRun(), failed] });
    render(<ReleasePipelineChips pipeline={subject.pipeline} />);

    const succeeded = chips().getByRole('link', { name: runDescription(pipelineRun()) });
    expect(succeeded).toHaveAttribute('href', pipelineRun().url);
    expect(succeeded.querySelector('.ui-badge-success')).not.toBeNull();
    const publish = chips().getByRole('link', { name: 'Publish: Failed' });
    expect(publish.querySelector('.ui-badge-destructive')).not.toBeNull();
  });

  it('says the runs are still being looked up', () => {
    const subject = release({ lookup: 'pending', runs: [] });
    render(<ReleasePipelineChips pipeline={subject.pipeline} />);
    expect(chips().getByText(LOOKUP_CHIPS.pending.label)).toBeInTheDocument();
    expect(chips().queryByRole('link')).toBeNull();
  });

  it('says the pipeline could not be read', () => {
    const subject = release({ lookup: 'unavailable', runs: [] });
    render(<ReleasePipelineChips pipeline={subject.pipeline} />);
    expect(chips().getByText(LOOKUP_CHIPS.unavailable.label)).toBeInTheDocument();
  });

  it('folds runs past the third into a count', () => {
    const runs = [1, 2, 3, 4, 5].map((id) => pipelineRun({ id, name: `Run ${id}` }));
    render(<ReleasePipelineChips pipeline={release({ runs }).pipeline} />);
    expect(chips().getAllByRole('link')).toHaveLength(3);
    const folded = chips().getByTitle(hiddenRuns(2));
    expect(folded.querySelector('[aria-hidden="true"]')?.textContent).toBe('+2');
    expect(folded.querySelector('.sr-only')?.textContent).toBe('2 more runs');
  });
});
