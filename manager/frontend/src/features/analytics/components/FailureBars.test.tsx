import { describe, expect, it } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { overview } from '../../../test/analytics';
import { FAILURES_LABEL, FailureBars, NO_FAILURES } from './FailureBars';

const { categories } = overview().runs.outcomes;

describe('FailureBars', () => {
  it('lists each failure kind with its count and spread', () => {
    render(<FailureBars categories={categories} />);

    const rows = within(screen.getByRole('list', { name: FAILURES_LABEL })).getAllByRole(
      'listitem'
    );
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain('Test failures');
    expect(rows[0].textContent).toContain('5');
    expect(rows[0].textContent).toContain('3 tasks · last');
    expect(rows[1].textContent).toContain('Build');
    expect(rows[0].getAttribute('data-tone')).toBe('destructive');
  });

  it('measures each bar against the commonest kind', () => {
    const { container } = render(<FailureBars categories={categories} />);

    const fills = [...container.querySelectorAll<HTMLElement>('.analytics-meter-fill')].map(
      (fill) => fill.style.inlineSize
    );
    expect(fills).toEqual(['100%', '60%']);
  });

  it('says so when nothing failed', () => {
    render(<FailureBars categories={[]} />);

    expect(screen.getByText(NO_FAILURES)).toBeDefined();
    expect(screen.queryByRole('list')).toBeNull();
  });
});
