import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { Citation } from '../types';
import type { ChatSource, ChatSources, SourceRequest } from '../utils/sources';
import { SourcesBox } from './SourcesBox';

const COLLAPSED_KEY = 'zone:chat-panel:user-1:sources';
const PULL_REQUEST = 'https://github.com/abnegate/zone/pull/160';
const OTHER = 'https://github.com/abnegate/zone/pull/161';

function source(url: string, messageId: string, overrides: Partial<Citation> = {}): ChatSource {
  return {
    key: url,
    messageId,
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

const EARLIER = source(PULL_REQUEST, 'message-1', {
  complete: false,
  outcome: 'incomplete',
  note: 'Checks could not be read.',
});
const LATER = source(PULL_REQUEST, 'message-2', {
  outcome: 'success',
  observed_at: '2026-11-05T09:40:00Z',
  note: null,
});
const ELSEWHERE = source(OTHER, 'message-2', { kind: 'github_build' });

const SOURCES: ChatSources = {
  all: [LATER, ELSEWHERE],
  replies: new Map([
    ['message-1', [EARLIER]],
    ['message-2', [LATER, ELSEWHERE]],
  ]),
};

function box(shown: SourceRequest | null, onShown = mock()) {
  return <SourcesBox sources={SOURCES} userId="user-1" shown={shown} onShown={onShown} />;
}

function renderBox(shown: SourceRequest | null = null) {
  const onShown = mock();
  const view = render(box(shown, onShown));
  return { ...view, onShown };
}

describe('SourcesBox', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    mock.restore();
  });

  it("lists the chat's sources with kind, revision, evidence, time and note", () => {
    renderBox();

    const toggle = screen.getByRole('button', { name: 'Sources, 2' });
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    const [first, second] = screen.getAllByTestId('citation');
    expect(
      within(first).getByRole('link', { name: new RegExp(`Source at ${PULL_REQUEST}`) })
    ).toHaveAttribute('href', PULL_REQUEST);
    expect(first).toHaveTextContent('GitHub issue');
    expect(first).toHaveTextContent('f4c546c');
    expect(first).toHaveTextContent('Passing');
    expect(second).toHaveTextContent('GitHub build');
    expect(second).toHaveTextContent('Checks are still running.');
    expect(screen.queryByText('Cited in this reply')).toBeNull();
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

  it("shows a reply its own evidence, not a later reply's observation of the same source", () => {
    const { onShown } = renderBox({ messageId: 'message-1', key: null });

    expect(onShown).toHaveBeenCalledTimes(1);
    expect(screen.getByText('Cited in this reply')).toBeInTheDocument();
    const [only, ...rest] = screen.getAllByTestId('citation');
    expect(rest).toEqual([]);
    expect(only).toHaveTextContent('Incomplete evidence');
    expect(only).toHaveTextContent('Checks could not be read.');
    expect(only).not.toHaveTextContent('Passing');
    expect(screen.getByRole('button', { name: 'Sources, 1' })).toBeInTheDocument();
  });

  it('goes back to every source in the chat', () => {
    renderBox({ messageId: 'message-1', key: null });

    fireEvent.click(screen.getByRole('button', { name: 'All 2 sources' }));

    expect(screen.queryByText('Cited in this reply')).toBeNull();
    expect(screen.getAllByTestId('citation')).toHaveLength(2);
  });

  it('opens when collapsed and points at the source a marker names', async () => {
    localStorage.setItem(COLLAPSED_KEY, 'true');
    renderBox({ messageId: 'message-2', key: OTHER });

    expect(screen.getByRole('button', { name: 'Sources, 2' })).toHaveAttribute(
      'aria-expanded',
      'true'
    );
    expect(localStorage.getItem(COLLAPSED_KEY)).toBe('true');
    const [first, second] = screen.getAllByTestId('citation');
    expect(second).toHaveClass('is-highlighted');
    expect(first).not.toHaveClass('is-highlighted');
    await waitFor(() =>
      expect(document.activeElement).toBe(second.querySelector('.citation-link'))
    );
  });

  it("focuses the reply's first source when no marker was named", async () => {
    renderBox({ messageId: 'message-2', key: null });

    const [first] = screen.getAllByTestId('citation');
    expect(document.querySelector('.is-highlighted')).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(first.querySelector('.citation-link')));
  });

  it('shows the whole chat until a reply asks for its sources', () => {
    const { onShown } = renderBox();

    expect(onShown).not.toHaveBeenCalled();
    expect(document.querySelector('.is-highlighted')).toBeNull();
  });
});
