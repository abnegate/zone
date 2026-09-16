import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { MemoryBadge } from './MemoryBadge';

describe('MemoryBadge', () => {
  it('renders nothing when the server set no flag', () => {
    const { container } = render(<MemoryBadge />);

    expect(container.firstChild).toBeNull();
  });

  it('renders nothing when the flag is false', () => {
    const { container } = render(<MemoryBadge used={false} />);

    expect(container.firstChild).toBeNull();
  });

  it('says the memory was read, never that it was used', () => {
    render(<MemoryBadge used />);

    const badge = screen.getByTestId('memory-badge');
    expect(badge).toHaveTextContent('Memory read');
    expect(badge).toHaveAttribute(
      'title',
      'The assistant read your stored memory while writing this reply.'
    );
  });
});
