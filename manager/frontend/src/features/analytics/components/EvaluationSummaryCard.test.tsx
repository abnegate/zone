import { describe, expect, it } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { overview } from '../../../test/analytics';
import {
  EVALUATIONS_TITLE,
  EvaluationSummaryCard,
  NO_EVALUATIONS_DESCRIPTION,
  NO_EVALUATIONS_TITLE,
} from './EvaluationSummaryCard';

const { evaluations } = overview();

describe('EvaluationSummaryCard', () => {
  it('counts every verdict', () => {
    render(<EvaluationSummaryCard evaluations={evaluations} />);

    const region = screen.getByRole('region', { name: EVALUATIONS_TITLE });
    const figures = within(region)
      .getAllByRole('term')
      .map((term) => term.textContent);
    expect(figures).toEqual(['Improved', 'Unchanged', 'Regressed', 'Not measured']);
    expect(
      within(region)
        .getAllByRole('definition')
        .map((value) => value.textContent)
    ).toEqual(['6', '8', '1', '3']);
  });

  it('splits the bar by verdict', () => {
    const { container } = render(<EvaluationSummaryCard evaluations={evaluations} />);

    const parts = [...container.querySelectorAll<HTMLElement>('.analytics-split-part')];
    expect(parts.map((part) => part.style.flexGrow)).toEqual(['6', '8', '1', '3']);
  });

  it('names the setting that turns evaluations on when nothing was evaluated', () => {
    render(
      <EvaluationSummaryCard
        evaluations={{ improved: 0, unchanged: 0, regressed: 0, not_measured: 0 }}
      />
    );

    expect(screen.getByText(NO_EVALUATIONS_TITLE)).toBeDefined();
    expect(screen.getByText(NO_EVALUATIONS_DESCRIPTION).textContent).toContain(
      'ZONE_TASK_EVALUATION'
    );
    expect(screen.queryByRole('term')).toBeNull();
  });

  it('shows the counts once only tasks without a measure were evaluated', () => {
    render(
      <EvaluationSummaryCard
        evaluations={{ improved: 0, unchanged: 0, regressed: 0, not_measured: 2 }}
      />
    );

    expect(screen.queryByText(NO_EVALUATIONS_TITLE)).toBeNull();
  });
});
