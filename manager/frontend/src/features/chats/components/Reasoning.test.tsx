import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { Reasoning } from './Reasoning';

describe('Reasoning', () => {
  it('hides empty thinking and shows a disclosure for model reasoning', () => {
    const { container } = render(<Reasoning content="  " />);
    expect(container.firstChild).toBeNull();

    render(<Reasoning content="The capital is Paris." />);
    expect(screen.getByText('Reasoning')).toBeInTheDocument();
    expect(screen.getByText('The capital is Paris.')).toBeInTheDocument();
    expect(screen.getByTestId('reasoning')).not.toHaveAttribute('open');
  });

  it('can start expanded so live thinking is visible', () => {
    render(<Reasoning content="Inspect the file first." open />);
    expect(screen.getByText('Inspect the file first.')).toBeInTheDocument();
    expect(screen.getByTestId('reasoning')).toHaveAttribute('open');
  });

  it('does not nest a markdown quote inside the thinking disclosure', () => {
    const { container } = render(<Reasoning content={'> Search the workspace first.'} open />);

    expect(screen.getByText('Search the workspace first.')).toBeInTheDocument();
    expect(container.querySelector('blockquote')).toBeNull();
    expect(container.querySelector('.message-markdown--compact')).not.toBeNull();
  });

  it('renders reasoning links inert, with no rejection marker', () => {
    const { container } = render(
      <Reasoning content={'Try https://example.com/a and [the doc](https://example.com/b).'} open />
    );

    expect(container.querySelector('a')).toBeNull();
    expect(screen.queryByTestId('unsourced-link')).toBeNull();
    expect(container.textContent).toContain('https://example.com/a');
    expect(screen.getByText(/the doc/)).toBeInTheDocument();
  });
});
