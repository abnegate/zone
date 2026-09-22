import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import PageBar from './PageBar';

describe('PageBar', () => {
  it('renders the title as the page heading', () => {
    render(<PageBar title="Models" />);
    expect(screen.getByRole('heading', { level: 1, name: 'Models' })).toBeInTheDocument();
  });

  it('keeps title, subtitle and actions on one bar', () => {
    render(
      <PageBar title="Models" subtitle="Manage local models">
        <button type="button">Install</button>
      </PageBar>
    );
    const bar = screen.getByRole('banner');
    expect(bar).toHaveClass('page-bar');
    expect(bar.querySelector('.page-bar-subtitle')).toHaveTextContent('Manage local models');
    const actions = bar.querySelector('.page-bar-actions');
    expect(actions?.contains(screen.getByRole('button', { name: 'Install' }))).toBe(true);
  });

  it('omits the subtitle and actions slots when they are empty', () => {
    render(<PageBar title="Models" />);
    expect(document.querySelector('.page-bar-subtitle')).toBeNull();
    expect(document.querySelector('.page-bar-actions')).toBeNull();
  });
});
