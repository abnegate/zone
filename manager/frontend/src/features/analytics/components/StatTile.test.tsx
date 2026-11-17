import { describe, expect, it } from 'bun:test';
import { join } from 'node:path';
import { render, screen } from '@testing-library/react';
import { read, rule, token } from '../../../test/css';
import { StatTile } from './StatTile';

const page = read(join(import.meta.dir, '../pages/AnalyticsPage.css'));
const kit = read(join(import.meta.dir, '../../../../../../packages/ui/src/styles/variables.css'));

describe('StatTile', () => {
  it('shows its label, value and detail on one tile', () => {
    render(<StatTile label="Runs" value="40" detail="8 failed" />);

    const tile = screen.getByTestId('analytics-tile');
    expect(tile.querySelector('.analytics-tile-label')?.textContent).toBe('Runs');
    expect(tile.textContent).toContain('40');
    expect(tile.textContent).toContain('8 failed');
  });

  it('leaves the detail out when there is none', () => {
    render(<StatTile label="Runs" value="0" />);

    expect(screen.getByTestId('analytics-tile').querySelector('.analytics-tile-detail')).toBeNull();
  });

  it('sets the label at 13 on a 20px line', () => {
    const label = rule(page, '.analytics-tile-label');
    expect(label).toContain('font-size: var(--ui-text-sm)');
    expect(label).toContain('line-height: var(--ui-space-5)');
    expect(token(kit, 'ui-text-sm')).toStartWith('0.8125rem');
    expect(token(kit, 'ui-space-5')).toStartWith('1.25rem');
  });
});
