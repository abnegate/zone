import { describe, expect, it } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { overview } from '../../../test/analytics';
import { NOT_MEASURED } from '../format';
import { NO_MODEL } from '../labels';
import { MODELS_LABEL, ModelBreakdownTable, NO_MODELS } from './ModelBreakdownTable';

const { models } = overview();

describe('ModelBreakdownTable', () => {
  it('shows each model with its rate and completion times', () => {
    render(<ModelBreakdownTable models={models} />);

    const [, first, second] = within(
      screen.getByRole('table', { name: MODELS_LABEL })
    ).getAllByRole('row');
    expect(
      within(first)
        .getAllByRole('cell')
        .map((cell) => cell.textContent)
    ).toEqual(['claude-opus', '30', '83%', '10m', '40m']);
    expect(
      within(second)
        .getAllByRole('cell')
        .map((cell) => cell.textContent)
    ).toEqual([NO_MODEL, '10', '50%', NOT_MEASURED, NOT_MEASURED]);
  });

  it('says so when no model ran', () => {
    render(<ModelBreakdownTable models={[]} />);

    expect(screen.getByText(NO_MODELS)).toBeDefined();
  });
});
