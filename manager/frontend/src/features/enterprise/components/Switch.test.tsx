import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { Switch } from './Switch';

function Harness({ initial = false, disabled = false }: { initial?: boolean; disabled?: boolean }) {
  const [checked, setChecked] = useState(initial);
  return (
    <Switch
      label="Enabled"
      description="Show this provider on the sign-in page."
      hint="Extra context."
      checked={checked}
      disabled={disabled}
      onChange={setChecked}
    />
  );
}

function control(): HTMLElement {
  return screen.getByRole('switch', { name: 'Enabled' });
}

describe('Switch', () => {
  it('is a switch named by its label that reports its state with aria-checked', () => {
    render(<Harness initial />);

    expect(control().tagName).toBe('BUTTON');
    expect(control()).toHaveAttribute('type', 'button');
    expect(control()).toHaveAttribute('aria-checked', 'true');
  });

  it('describes itself with its description and hint', () => {
    render(<Harness />);

    const described = (control().getAttribute('aria-describedby') ?? '')
      .split(' ')
      .map((id) => document.getElementById(id)?.textContent);
    expect(described).toEqual(['Show this provider on the sign-in page.', 'Extra context.']);
  });

  it('toggles on click', () => {
    render(<Harness />);

    fireEvent.click(control());
    expect(control()).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(control());
    expect(control()).toHaveAttribute('aria-checked', 'false');
  });

  for (const key of [' ', 'Enter']) {
    it(`toggles with ${key === ' ' ? 'Space' : key} and stops the browser from clicking too`, () => {
      render(<Harness />);

      const event = fireEvent.keyDown(control(), { key });

      expect(event).toBe(false);
      expect(control()).toHaveAttribute('aria-checked', 'true');
    });
  }

  it('ignores a held key repeating', () => {
    render(<Harness />);

    fireEvent.keyDown(control(), { key: ' ' });
    fireEvent.keyDown(control(), { key: ' ', repeat: true });

    expect(control()).toHaveAttribute('aria-checked', 'true');
  });

  it('leaves other keys to the browser', () => {
    render(<Harness />);

    expect(fireEvent.keyDown(control(), { key: 'Tab' })).toBe(true);
    expect(control()).toHaveAttribute('aria-checked', 'false');
  });

  it('takes a fuller accessible name when the visible label alone is ambiguous', () => {
    render(<Switch label="Enabled" ariaLabel="Enabled for Graylog" checked onChange={mock()} />);

    expect(screen.getByRole('switch', { name: 'Enabled for Graylog' })).toHaveAttribute(
      'aria-checked',
      'true'
    );
    expect(screen.queryByRole('switch', { name: 'Enabled' })).toBeNull();
  });

  it('does not change while disabled', () => {
    const change = mock();
    render(<Switch label="Enabled" checked={false} disabled onChange={change} />);

    fireEvent.keyDown(control(), { key: 'Enter' });

    expect(control()).toBeDisabled();
    expect(change).not.toHaveBeenCalled();
  });
});
