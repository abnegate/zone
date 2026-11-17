import { describe, expect, it } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { SectionCard } from './SectionCard';

describe('SectionCard', () => {
  it('is a region named by its heading', () => {
    render(
      <SectionCard title="Sources" description="Per source" wide>
        <p>body</p>
      </SectionCard>
    );

    const region = screen.getByRole('region', { name: 'Sources' });
    expect(within(region).getByRole('heading', { level: 2, name: 'Sources' })).toBeDefined();
    expect(region.className).toContain('analytics-section--wide');
    expect(within(region).getByText('Per source')).toBeDefined();
  });
});
