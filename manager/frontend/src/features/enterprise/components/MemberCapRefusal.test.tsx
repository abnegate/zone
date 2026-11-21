import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import { memberCapMessage } from '../members';
import { PRICING_URL } from '../pricing';
import { MemberCapRefusal } from './MemberCapRefusal';

describe('MemberCapRefusal', () => {
  it('explains the cap and links to the plans in a new tab', () => {
    render(<MemberCapRefusal className="modal-error" />);

    const alert = screen.getByRole('alert');
    expect(alert).toHaveClass('modal-error');
    expect(alert).toHaveTextContent(memberCapMessage());
    const link = screen.getByRole('link', { name: 'Compare plans (opens in a new tab)' });
    expect(link).toHaveAttribute('href', PRICING_URL);
    expect(link).toHaveAttribute('target', '_blank');
    expect(link.getAttribute('rel')?.split(' ')).toContain('noopener');
  });
});
