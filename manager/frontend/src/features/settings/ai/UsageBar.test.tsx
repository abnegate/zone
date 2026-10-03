import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { UsageWindow } from './schemas';
import { UsageBar } from './UsageBar';

const now = Date.parse('2026-09-23T04:00:00Z');
const fiveHour: UsageWindow = {
  name: '5h',
  used_percent: 62,
  used: null,
  limit: null,
  resets_at: '2026-09-23T06:10:00Z',
};

function row(window: UsageWindow) {
  const { container } = render(<UsageBar window={window} now={now} />);
  return {
    meter: screen.getByRole('meter', { name: `${window.name} usage` }),
    row: container.querySelector('.usage-bar') as HTMLElement,
  };
}

describe('UsageBar', () => {
  it('reads a window in aiusg order: name, bar, share used, counts, reset', () => {
    const { meter, row: shown } = row(fiveHour);

    expect(meter).toHaveAttribute('aria-valuemin', '0');
    expect(meter).toHaveAttribute('aria-valuemax', '100');
    expect(meter).toHaveAttribute('aria-valuenow', '62');
    expect(meter).toHaveAttribute('aria-valuetext', '62% used, resets in 2h 10m');
    expect(Array.from(shown.children, (child) => child.className)).toEqual([
      'usage-bar-name',
      'usage-bar-track',
      'usage-bar-percent',
      'usage-bar-counts',
      'usage-bar-resets',
    ]);
    expect(shown.textContent).toBe('5h62%resets in 2h 10m');
    expect(shown).toHaveAttribute('data-severity', 'fine');
    expect((meter.firstElementChild as HTMLElement).style.width).toBe('62%');
  });

  it('shows the counts of a window that reports them', () => {
    const { meter, row: shown } = row({ ...fiveHour, used: 31, limit: 50 });

    expect(shown.querySelector('.usage-bar-counts')).toHaveTextContent('31/50');
    expect(meter).toHaveAttribute('aria-valuetext', '62% used, 31/50, resets in 2h 10m');
  });

  it('tints a window by how much of it is spent', () => {
    for (const [percent, severity] of [
      [80, 'warning'],
      [95, 'critical'],
      [100, 'exhausted'],
    ] as const) {
      const { container, unmount } = render(
        <UsageBar window={{ ...fiveHour, used_percent: percent }} now={now} />
      );
      expect(container.querySelector('.usage-bar')).toHaveAttribute('data-severity', severity);
      unmount();
    }
  });

  it('keeps a spent window full rather than overflowing', () => {
    const { meter } = row({ ...fiveHour, used_percent: 140 });

    expect(meter).toHaveAttribute('aria-valuenow', '100');
    expect((meter.firstElementChild as HTMLElement).style.width).toBe('100%');
    expect(screen.getByText('140%')).toBeInTheDocument();
  });

  it('says so when the share used is unknown, and leaves out a reset nobody reported', () => {
    const { meter, row: shown } = row({ ...fiveHour, used_percent: null, resets_at: null });

    expect(meter).toHaveAttribute('aria-valuenow', '0');
    expect(meter).toHaveAttribute('aria-valuetext', 'usage unknown');
    expect(shown).toHaveAttribute('data-severity', 'unknown');
    expect(shown.textContent).toBe('5h?%');
  });
});
