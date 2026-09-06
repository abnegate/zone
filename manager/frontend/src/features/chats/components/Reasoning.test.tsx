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
  });
});
