import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { Generation, RESPONDING } from './Generation';

function visibleText(element: Element): string {
  const copy = element.cloneNode(true) as Element;
  for (const hidden of copy.querySelectorAll('.sr-only, [aria-hidden="true"]')) {
    hidden.remove();
  }
  return copy.textContent ?? '';
}

describe('Generation', () => {
  it('shows only the spinner and the elapsed time while a turn has no status', () => {
    const { container } = render(<Generation status={null} started={null} />);

    const indicator = container.querySelector('.message-status');
    expect(indicator).not.toBeNull();
    expect(indicator?.querySelector('.generation-spinner')).not.toBeNull();
    expect(screen.getByRole('timer')).toHaveTextContent('0:00');
    expect(visibleText(indicator as Element)).toBe('0:00');
  });

  it('still tells a screen reader the turn is responding', () => {
    render(<Generation status={null} started={null} />);

    const status = screen.getByRole('status');
    expect(status).toHaveTextContent(RESPONDING);
    expect(status).toHaveClass('sr-only');
  });

  it('shows a status the server reports beside the time', () => {
    const { container } = render(<Generation status="Generating image…" started={null} />);

    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('Generating image…');
    expect(status).not.toHaveClass('sr-only');
    expect(visibleText(container.querySelector('.message-status') as Element)).toBe(
      'Generating image…0:00'
    );
  });

  it('counts from when the turn started rather than from when it was drawn', () => {
    render(<Generation status={null} started={Date.now() - 95_000} />);

    expect(screen.getByRole('timer')).toHaveTextContent('1:35');
  });

  it('reads zero rather than backwards when the start is ahead of this clock', () => {
    render(<Generation status={null} started={Date.now() + 5_000} />);

    expect(screen.getByRole('timer')).toHaveTextContent('0:00');
  });
});
