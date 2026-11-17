import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { emptyOverview, overview } from '../../../test/analytics';
import { NOT_MEASURED } from '../AnalyticsFormat';
import { RunTiles } from './RunTiles';

function tile(label: string): HTMLElement {
  const tiles = screen.getAllByTestId('analytics-tile');
  const found = tiles.find(
    (candidate) => candidate.querySelector('.analytics-tile-label')?.textContent === label
  );
  if (!found) throw new Error(`no ${label} tile`);
  return found;
}

describe('RunTiles', () => {
  it('shows the six headline figures', () => {
    render(<RunTiles overview={overview()} />);

    expect(screen.getAllByTestId('analytics-tile')).toHaveLength(6);
    expect(tile('Success rate').textContent).toContain('75%');
    expect(tile('Success rate').textContent).toContain('Recent 82%');
    expect(tile('Success rate').textContent).toContain('Up 12 points');
    expect(tile('Runs').textContent).toContain('8 failed · 2 cancelled');
    expect(tile('Median completion').textContent).toContain('13m');
    expect(tile('90th percentile').textContent).toContain('1h 4m');
    expect(tile('PR merge rate').textContent).toContain('82%');
    expect(tile('Time to merge').textContent).toContain('1h 35m');
  });

  it('shows nothing measured rather than a zero rate with no runs', () => {
    render(<RunTiles overview={emptyOverview()} />);

    expect(tile('Success rate').querySelector('.analytics-tile-value')?.textContent).toBe(
      NOT_MEASURED
    );
    expect(tile('Success rate').querySelector('.analytics-tile-detail')).toBeNull();
    expect(tile('Median completion').querySelector('.analytics-tile-value')?.textContent).toBe(
      NOT_MEASURED
    );
  });
});
