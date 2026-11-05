import { afterEach, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import type { ChatPullRequests } from '../hooks/useChatPullRequests';
import type { ChatPullRequest } from '../types';
import type { GalleryItem } from '../utils/gallery';
import { PANEL_WIDTH } from '../utils/panel';
import { ChatPanel } from './ChatPanel';

const PULL_REQUEST: ChatPullRequest = {
  id: 'pr-1',
  provider: 'github',
  owner: 'acme',
  repo: 'widgets',
  number: 12,
  url: 'https://github.com/acme/widgets/pull/12',
  title: 'Add widgets',
  dismissed: false,
  created_at: '2026-11-05T10:00:00Z',
  status: null,
  actionable: false,
};

const IMAGE: GalleryItem = {
  key: 'message-1:0',
  messageId: 'message-1',
  name: 'sunset.png',
  mime: 'image/png',
  url: 'data:image/png;base64,iVBORw0KGgo=',
  kind: 'image',
};

const WIDTH_KEY = 'zone:chat-panel:user-1:width';
const SEPARATOR = 'Resize changes and media';

function changes(pullRequests: ChatPullRequest[] = []): ChatPullRequests {
  return {
    pullRequests,
    dismissed: null,
    pending: new Set(),
    error: null,
    refresh: mock(async () => undefined),
    change: mock(async () => undefined),
    undo: mock(async () => undefined),
  };
}

function renderPanel(
  options: { pullRequests?: ChatPullRequest[]; media?: GalleryItem[]; userId?: string } = {}
) {
  const onShowMessage = mock();
  const view = render(
    <ChatPanel
      chatId="chat-1"
      userId={options.userId ?? 'user-1'}
      changes={changes(options.pullRequests)}
      media={options.media ?? []}
      onShowMessage={onShowMessage}
    />
  );
  return { ...view, onShowMessage };
}

function separator(): HTMLElement {
  return screen.getByRole('separator', { name: SEPARATOR });
}

describe('ChatPanel', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    mock.restore();
  });

  it('takes no room in a chat with neither pull requests nor media', () => {
    const { container } = renderPanel();

    expect(container.innerHTML).toBe('');
  });

  it('shows the gallery only when the chat holds media', () => {
    renderPanel({ pullRequests: [PULL_REQUEST] });

    expect(screen.getByRole('complementary', { name: 'Changes and media' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Changes, 1' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Gallery, 1' })).toBeNull();
  });

  it('shows the changes only when the chat opened a pull request', () => {
    renderPanel({ media: [IMAGE] });

    expect(screen.getByRole('button', { name: 'Gallery, 1' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open sunset.png' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Changes, 1' })).toBeNull();
  });

  it('is a vertical separator the keyboard can resize, and remembers the width', () => {
    const { unmount } = renderPanel({ media: [IMAGE] });

    expect(separator()).toHaveAttribute('aria-orientation', 'vertical');
    expect(separator()).toHaveAttribute('aria-valuenow', String(PANEL_WIDTH.fallback));
    expect(separator()).toHaveAttribute('tabindex', '0');

    fireEvent.keyDown(separator(), { key: 'ArrowLeft' });
    const wider = PANEL_WIDTH.fallback + PANEL_WIDTH.step;
    expect(separator()).toHaveAttribute('aria-valuenow', String(wider));
    expect(localStorage.getItem(WIDTH_KEY)).toBe(String(wider));

    fireEvent.keyDown(separator(), { key: 'ArrowRight', shiftKey: true });
    const narrower = wider - PANEL_WIDTH.largeStep;
    expect(localStorage.getItem(WIDTH_KEY)).toBe(String(narrower));

    fireEvent.keyDown(separator(), { key: 'Home' });
    expect(localStorage.getItem(WIDTH_KEY)).toBe(String(PANEL_WIDTH.min));

    unmount();
    renderPanel({ media: [IMAGE] });
    expect(separator()).toHaveAttribute('aria-valuenow', String(PANEL_WIDTH.min));
  });

  it('keeps each viewer to their own width', () => {
    localStorage.setItem(WIDTH_KEY, '400');

    renderPanel({ media: [IMAGE], userId: 'user-2' });

    expect(separator()).toHaveAttribute('aria-valuenow', String(PANEL_WIDTH.fallback));
  });

  it('resizes as its edge is dragged, and saves the width once it is let go', () => {
    renderPanel({ media: [IMAGE] });

    fireEvent.pointerDown(separator(), { button: 0, clientX: 600, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 560, pointerId: 1 });
    expect(separator()).toHaveAttribute('aria-valuenow', String(PANEL_WIDTH.fallback + 40));
    expect(localStorage.getItem(WIDTH_KEY)).toBeNull();

    fireEvent.pointerUp(window, { pointerId: 1 });
    expect(localStorage.getItem(WIDTH_KEY)).toBe(String(PANEL_WIDTH.fallback + 40));
  });

  it('never grows past its bounds', () => {
    localStorage.setItem(WIDTH_KEY, '5000');
    renderPanel({ media: [IMAGE] });

    expect(Number(separator().getAttribute('aria-valuenow'))).toBeLessThanOrEqual(PANEL_WIDTH.max);
  });

  it('still works when the browser refuses to store anything', () => {
    spyOn(localStorage, 'getItem').mockImplementation(() => {
      throw new Error('denied');
    });
    spyOn(localStorage, 'setItem').mockImplementation(() => {
      throw new Error('denied');
    });
    renderPanel({ media: [IMAGE] });

    fireEvent.keyDown(separator(), { key: 'ArrowLeft' });

    expect(separator()).toHaveAttribute(
      'aria-valuenow',
      String(PANEL_WIDTH.fallback + PANEL_WIDTH.step)
    );
  });

  it('hides to a rail and remembers that it was hidden', () => {
    const { unmount } = renderPanel({ media: [IMAGE] });

    fireEvent.click(screen.getByRole('button', { name: 'Hide changes and media' }));

    expect(screen.queryByRole('separator')).toBeNull();
    expect(localStorage.getItem('zone:chat-panel:user-1:collapsed')).toBe('true');
    unmount();
    renderPanel({ media: [IMAGE] });
    const rail = screen.getByRole('button', { name: /Show changes and media/ });
    expect(rail).toHaveAttribute('aria-expanded', 'false');

    fireEvent.click(rail);
    expect(separator()).toBeInTheDocument();
    expect(localStorage.getItem('zone:chat-panel:user-1:collapsed')).toBe('false');
  });

  it('opens as a sheet from its toggle, for a screen too narrow to hold it', async () => {
    renderPanel({ pullRequests: [PULL_REQUEST], media: [IMAGE] });

    const toggle = screen.getByRole('button', { name: /^Changes and media/ });
    expect(toggle).toHaveTextContent('1 changes, 1 media');
    fireEvent.click(toggle);

    const sheet = await screen.findByRole('dialog', { name: 'Changes and media' });
    expect(sheet).toHaveClass('chat-panel-sheet');
  });
});
