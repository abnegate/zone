import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { NavDrawerProvider } from '../../context/NavDrawerContext';
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

  it('leads the bar with the drawer trigger inside the app frame', () => {
    render(
      <NavDrawerProvider>
        <PageBar title="Models" />
      </NavDrawerProvider>
    );
    const trigger = screen.getByRole('button', { name: 'Toggle menu' });
    expect(trigger.closest('.page-bar-lead')).toBeInTheDocument();
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(trigger);
    expect(screen.getByRole('button', { name: 'Toggle menu' })).toHaveAttribute(
      'aria-expanded',
      'true'
    );
  });

  it('has no leading slot outside the frame, where there is no drawer', () => {
    render(<PageBar title="Models" />);
    expect(document.querySelector('.page-bar-lead')).toBeNull();
  });

  it('leads with back instead of the drawer on a screen reached from a list', () => {
    const onClick = mock();
    render(
      <NavDrawerProvider>
        <PageBar title="A chat" back={{ label: 'Back to chats', onClick }} />
      </NavDrawerProvider>
    );
    expect(screen.queryByRole('button', { name: 'Toggle menu' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Back to chats' }));
    expect(onClick).toHaveBeenCalled();
  });
});
