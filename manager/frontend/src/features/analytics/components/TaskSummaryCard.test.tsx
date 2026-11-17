import { describe, expect, it } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { overview } from '../../../test/analytics';
import { TASKS_TITLE, TaskSummaryCard } from './TaskSummaryCard';

describe('TaskSummaryCard', () => {
  it('shows created, shipped and reopened with statuses and origins', () => {
    render(<TaskSummaryCard tasks={overview().tasks} />);

    const region = screen.getByRole('region', { name: TASKS_TITLE });
    const activity = within(region).getByLabelText('Task activity');
    expect(
      within(activity)
        .getAllByRole('definition')
        .map((value) => value.textContent)
    ).toEqual(['35', '18', '2']);
    expect(within(region).getAllByRole('listitem')).toHaveLength(11);
    expect(within(region).getByText('Automations')).toBeDefined();
    expect(within(region).getByText('In review')).toBeDefined();
  });
});
