import { describe, expect, it } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { overview } from '../../../test/analytics';
import { NO_SOURCE } from '../AnalyticsLabel';
import { NO_SOURCES, SOURCES_LABEL, SourceLeaderboard } from './SourceLeaderboard';

const { sources } = overview();

function bodyRows(): HTMLElement[] {
  const table = screen.getByRole('table', { name: SOURCES_LABEL });
  return within(table).getAllByRole('row').slice(1);
}

describe('SourceLeaderboard', () => {
  it('ranks the busiest source first with its rate and merged pull requests', () => {
    render(<SourceLeaderboard sources={[...sources].reverse()} />);

    const [first, second] = bodyRows();
    expect(within(first).getByText('acme/zone')).toBeDefined();
    expect(within(first).getByText('GitHub')).toBeDefined();
    expect(within(first).getByText('28')).toBeDefined();
    expect(within(first).getByText('79%')).toBeDefined();
    expect(within(first).getByText('9')).toBeDefined();
    expect(within(second).getByText(NO_SOURCE)).toBeDefined();
  });

  it('heads its columns', () => {
    render(<SourceLeaderboard sources={sources} />);

    expect(screen.getAllByRole('columnheader').map((header) => header.textContent)).toEqual([
      'Source',
      'Runs',
      'Success',
      'Merged',
    ]);
  });

  it('says so when no source ran', () => {
    render(<SourceLeaderboard sources={[]} />);

    expect(screen.getByText(NO_SOURCES)).toBeDefined();
    expect(screen.queryByRole('table')).toBeNull();
  });
});
