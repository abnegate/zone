import { describe, expect, it } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { Figures } from './Figures';

describe('Figures', () => {
  it('pairs each label with its value', () => {
    render(<Figures label="Activity" figures={[{ label: 'Opened', value: '4' }]} />);

    const list = screen.getByLabelText('Activity');
    expect(within(list).getByRole('term').textContent).toBe('Opened');
    expect(within(list).getByRole('definition').textContent).toBe('4');
  });
});
