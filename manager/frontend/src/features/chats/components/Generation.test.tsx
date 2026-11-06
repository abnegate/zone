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
    const { container } = render(<Generation status={null} />);

    const indicator = container.querySelector('.message-status');
    expect(indicator).not.toBeNull();
    expect(indicator?.querySelector('.generation-spinner')).not.toBeNull();
    expect(screen.getByRole('timer')).toHaveTextContent('0:00');
    expect(visibleText(indicator as Element)).toBe('0:00');
  });

  it('still tells a screen reader the turn is responding', () => {
    render(<Generation status={null} />);

    const status = screen.getByRole('status');
    expect(status).toHaveTextContent(RESPONDING);
    expect(status).toHaveClass('sr-only');
  });

  it('shows a status the server reports beside the time', () => {
    const { container } = render(<Generation status="Generating image…" />);

    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('Generating image…');
    expect(status).not.toHaveClass('sr-only');
    expect(visibleText(container.querySelector('.message-status') as Element)).toBe(
      'Generating image…0:00'
    );
  });
});
