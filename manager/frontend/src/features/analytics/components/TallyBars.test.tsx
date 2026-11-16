import { describe, expect, it } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { overview } from '../../../test/analytics';
import { qualityLabel, qualityTone, Tone, taskStatusLabel, taskStatusTone } from '../labels';
import { TallyBars } from './TallyBars';

const { tasks, pull_requests: pullRequests } = overview();

function rows(name: string): HTMLElement[] {
  return within(screen.getByRole('list', { name })).getAllByRole('listitem');
}

describe('TallyBars', () => {
  it('labels task statuses the way the board does, in the fixed order', () => {
    render(
      <TallyBars
        label="By status"
        tallies={tasks.statuses}
        labelOf={taskStatusLabel}
        toneOf={taskStatusTone}
      />
    );

    const labels = rows('By status').map(
      (row) => row.querySelector('.analytics-meter-label')?.textContent
    );
    expect(labels).toEqual([
      'Created',
      'Queued',
      'In progress',
      'Blocked',
      'Paused',
      'In review',
      'Completed',
      'Cancelled',
    ]);
    expect(rows('By status')[6].getAttribute('data-tone')).toBe('success');
    expect(screen.getByRole('heading', { name: 'By status' })).toBeDefined();
  });

  it('keeps a status with nothing in it as an empty row', () => {
    render(
      <TallyBars
        label="By status"
        tallies={tasks.statuses}
        labelOf={taskStatusLabel}
        toneOf={taskStatusTone}
      />
    );

    const paused = rows('By status')[4];
    expect(paused.querySelector('.analytics-meter-value')?.textContent).toBe('0');
    expect((paused.querySelector('.analytics-meter-fill') as HTMLElement).style.inlineSize).toBe(
      '0%'
    );
  });

  it('tints pull request quality bands from poor to exemplary', () => {
    render(
      <TallyBars
        label="Review quality"
        tallies={pullRequests.quality}
        labelOf={qualityLabel}
        toneOf={qualityTone}
      />
    );

    expect(rows('Review quality').map((row) => row.getAttribute('data-tone'))).toEqual([
      'destructive',
      'warning',
      'info',
      'success',
    ]);
  });

  it('humanizes a name it has no label for', () => {
    render(
      <TallyBars
        label="By origin"
        tallies={[{ name: 'webhook_call', count: 2 }]}
        labelOf={taskStatusLabel}
        toneOf={() => Tone.Accent}
      />
    );

    expect(screen.getByText('Webhook call')).toBeDefined();
  });
});
