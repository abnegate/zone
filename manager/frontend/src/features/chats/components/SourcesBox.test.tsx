import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { Citation } from '../types';
import type { ChatSource, SourceRequest } from '../utils/sources';
import { SourcesBox } from './SourcesBox';

const COLLAPSED_KEY = 'zone:chat-panel:user-1:sources';

function source(url: string, overrides: Partial<Citation> = {}): ChatSource {
  return {
    key: url,
    messageId: 'message-1',
    citation: {
      kind: 'github_issue',
      title: `Source at ${url}`,
      url,
      revision: 'f4c546c2f5270f38f84d1a9121a3f5c6461d1be2',
      observed_at: '2026-11-05T09:14:00Z',
      complete: true,
      outcome: 'pending',
      provenance: 'server_execution',
      note: 'Checks are still running.',
      ...overrides,
    },
  };
}

const SOURCES = [
  source('https://github.com/abnegate/zone/pull/161'),
  source('https://github.com/abnegate/zone/pull/160', {
    kind: 'github_build',
    complete: false,
    outcome: 'incomplete',
  }),
];

function renderBox(shown: SourceRequest | null = null, onShown = mock()) {
  const view = render(
    <SourcesBox sources={SOURCES} userId="user-1" shown={shown} onShown={onShown} />
  );
  return { ...view, onShown };
}

describe('SourcesBox', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    mock.restore();
  });

  it('lists each source with its kind, revision, evidence, time and note', () => {
    renderBox();

    const toggle = screen.getByRole('button', { name: 'Sources, 2' });
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    const [first, second] = screen.getAllByTestId('citation');
    expect(
      within(first).getByRole('link', {
        name: /Source at https:\/\/github.com\/abnegate\/zone\/pull\/161/,
      })
    ).toHaveAttribute('href', 'https://github.com/abnegate/zone/pull/161');
    expect(first).toHaveTextContent('GitHub issue');
    expect(first).toHaveTextContent('f4c546c');
    expect(first).toHaveTextContent('Pending');
    expect(first).toHaveTextContent('Checks are still running.');
    expect(second).toHaveTextContent('GitHub build');
    expect(second).toHaveTextContent('Incomplete evidence');
  });

  it('collapses and remembers that it was collapsed', () => {
    const { unmount } = renderBox();

    fireEvent.click(screen.getByRole('button', { name: 'Sources, 2' }));

    expect(localStorage.getItem(COLLAPSED_KEY)).toBe('true');
    expect(document.querySelector('.citation-list')?.parentElement).toHaveAttribute('hidden');
    unmount();
    renderBox();
    expect(screen.getByRole('button', { name: 'Sources, 2' })).toHaveAttribute(
      'aria-expanded',
      'false'
    );
  });

  it('opens and points at the sources a reply asks to show, even when collapsed', async () => {
    localStorage.setItem(COLLAPSED_KEY, 'true');
    const { onShown } = renderBox({ keys: ['https://github.com/abnegate/zone/pull/160'] });

    expect(onShown).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('button', { name: 'Sources, 2' })).toHaveAttribute(
      'aria-expanded',
      'true'
    );
    const [first, second] = screen.getAllByTestId('citation');
    expect(second).toHaveClass('is-highlighted');
    expect(first).not.toHaveClass('is-highlighted');
    await waitFor(() =>
      expect(document.activeElement).toBe(second.querySelector('.citation-link'))
    );
  });

  it('shows nothing extra until a reply asks for its sources', () => {
    const { onShown } = renderBox();

    expect(onShown).not.toHaveBeenCalled();
    expect(document.querySelector('.is-highlighted')).toBeNull();
  });
});
