import { describe, expect, it } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { ContextUsageSchema } from '../schemas';
import type { ContextUsage as Usage } from '../types';
import { ContextUsage } from './ContextUsage';

const usage: Usage = {
  model: 'local-model',
  used: 2400,
  limit: 10000,
  reserved: 1000,
  threshold: 8000,
  remaining: 5600,
  estimated: true,
  incomplete: false,
  source: 'runtime',
  status: 'compacted',
  revision: 2,
  compacted_messages: 8,
  updated_at: '2026-09-06T00:00:00Z',
  breakdown: {
    instructions: 400,
    conversation: 1000,
    tools: 200,
    results: 300,
    summary: 400,
    attachments: 0,
    overhead: 100,
  },
};
describe('ContextUsage', () => {
  it('uses total capacity for percentage and exposes budget separately with keyboard dismissal', () => {
    render(<ContextUsage usage={usage} />);
    const button = screen.getByRole('button', { name: /Context/ });
    expect(button.textContent).toContain('24%');
    expect(button.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(button);
    expect(screen.getByRole('region', { name: 'Context usage details' })).toBeTruthy();
    expect(screen.getByText('8 messages summarized.', { exact: false })).toBeTruthy();
    expect(screen.getByText('Reserved for response')).toBeTruthy();
    fireEvent.keyDown(button, { key: 'Escape' });
    expect(button.getAttribute('aria-expanded')).toBe('false');
    expect(document.activeElement).toBe(button);
  });
  it('never renders a precise percentage for unknown image costs or zero capacity', () => {
    const { rerender } = render(
      <ContextUsage
        usage={{ ...usage, incomplete: true, breakdown: { ...usage.breakdown, attachments: null } }}
      />
    );
    expect(screen.getByRole('button').textContent).not.toContain('%');
    fireEvent.click(screen.getByRole('button'));
    expect(screen.getByText('Estimate incomplete: some input costs are unknown.')).toBeTruthy();
    rerender(<ContextUsage usage={{ ...usage, limit: 0 }} />);
    expect(screen.getByRole('button').textContent).not.toContain('%');
  });
  it('shows blocked/compacting state and older server fallback without disabling sending', () => {
    const { rerender } = render(
      <ContextUsage usage={{ ...usage, status: 'blocked', used: 12000 }} />
    );
    expect(screen.getByRole('button').textContent).toContain('120%');
    expect(screen.getByText('Needs attention')).toBeTruthy();
    rerender(<ContextUsage usage={{ ...usage, status: 'compacting' }} />);
    expect(screen.getByText('Compacting…')).toBeTruthy();
    rerender(<ContextUsage usage={{ ...usage, status: 'ready', remaining: 0 }} />);
    expect(screen.getByText('Will compact before sending')).toBeTruthy();
    rerender(<ContextUsage usage={null} />);
    fireEvent.click(screen.getByRole('button'));
    expect(screen.getByText(/This server has not supplied/)).toBeTruthy();
  });
  it('rejects malformed numbers and unmarked unknown attachment costs', () => {
    for (const used of [-1, Number.NaN, Number.POSITIVE_INFINITY, Number.MAX_SAFE_INTEGER + 1]) {
      expect(ContextUsageSchema.safeParse({ ...usage, used }).success).toBe(false);
    }
    expect(
      ContextUsageSchema.safeParse({
        ...usage,
        breakdown: { ...usage.breakdown, attachments: null },
      }).success
    ).toBe(false);
  });
});

it('explains paused compaction without claiming a fitting context is full', () => {
  render(
    <ContextUsage
      usage={{
        ...usage,
        used: 2400,
        status: 'blocked',
        reason: 'The summary could not be prepared. History is unchanged.',
      }}
    />
  );
  expect(screen.getByRole('button').textContent).toContain('Needs attention');
  expect(screen.queryByText('Context full')).toBeNull();
  fireEvent.click(screen.getByRole('button'));
  expect(screen.getByText('The summary could not be prepared. History is unchanged.')).toBeTruthy();
});
