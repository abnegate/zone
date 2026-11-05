import { afterEach, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ChatPullRequests } from '../hooks/useChatPullRequests';
import type { ChatPullRequest } from '../types';
import type { GalleryItem } from '../utils/gallery';
import { PANEL_WIDTH } from '../utils/panel';
import type { ChatSource, SourceRequest } from '../utils/sources';
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
  writable: true,
  credentialed: false,
};

const IMAGE: GalleryItem = {
  key: 'message-1:0',
  messageId: 'message-1',
  name: 'sunset.png',
  mime: 'image/png',
  url: 'data:image/png;base64,iVBORw0KGgo=',
  kind: 'image',
};

const SOURCE: ChatSource = {
  key: 'https://github.com/abnegate/zone/pull/160',
  messageId: 'message-2',
  citation: {
    kind: 'github_issue',
    title: '#160 feat(chat): merge chat pull requests when ready',
    url: 'https://github.com/abnegate/zone/pull/160',
    revision: 'f4c546c2f5270f38f84d1a9121a3f5c6461d1be2',
    observed_at: '2026-11-05T09:14:00Z',
    complete: true,
    outcome: 'pending',
    provenance: 'server_execution',
  },
};

const WIDTH_KEY = 'zone:chat-panel:user-1:width';
const SEPARATOR = 'Resize changes, media and sources';

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

interface PanelOptions {
  pullRequests?: ChatPullRequest[];
  media?: GalleryItem[];
  sources?: ChatSource[];
  shownSources?: SourceRequest | null;
  userId?: string;
}

function panel(options: PanelOptions, onShowMessage = mock(), onSourcesShown = mock()) {
  return (
    <ChatPanel
      chatId="chat-1"
      userId={options.userId ?? 'user-1'}
      changes={changes(options.pullRequests)}
      media={options.media ?? []}
      sources={options.sources ?? []}
      shownSources={options.shownSources ?? null}
      onSourcesShown={onSourcesShown}
      onShowMessage={onShowMessage}
    />
  );
}

function renderPanel(options: PanelOptions = {}) {
  const onShowMessage = mock();
  const onSourcesShown = mock();
  const view = render(panel(options, onShowMessage, onSourcesShown));
  return { ...view, onShowMessage, onSourcesShown };
}

function phone(matches: boolean) {
  spyOn(window, 'matchMedia').mockImplementation(
    (query: string) =>
      ({
        matches,
        media: query,
        onchange: null,
        addEventListener: () => undefined,
        removeEventListener: () => undefined,
        addListener: () => undefined,
        removeListener: () => undefined,
        dispatchEvent: () => false,
      }) as MediaQueryList
  );
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

  it('shows the sources only when a reply in the chat cites one', () => {
    renderPanel({ sources: [SOURCE] });

    const aside = screen.getByRole('complementary', { name: 'Changes, media and sources' });
    expect(within(aside).getByRole('button', { name: 'Sources, 1' })).toBeInTheDocument();
    expect(within(aside).getByText(SOURCE.citation.title)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Changes, \d/ })).toBeNull();
    expect(screen.queryByRole('button', { name: /^Gallery, \d/ })).toBeNull();
    expect(screen.getByRole('button', { name: /^Changes, media and sources/ })).toHaveTextContent(
      '1 source'
    );
  });

  it('comes back from its rail to show the sources a reply points to', () => {
    localStorage.setItem('zone:chat-panel:user-1:collapsed', 'true');
    phone(false);
    const { rerender, onSourcesShown } = renderPanel({ sources: [SOURCE] });
    expect(screen.queryByRole('separator')).toBeNull();

    rerender(
      panel({ sources: [SOURCE], shownSources: { keys: [SOURCE.key] } }, mock(), onSourcesShown)
    );

    expect(separator()).toBeInTheDocument();
    expect(localStorage.getItem('zone:chat-panel:user-1:collapsed')).toBe('false');
    expect(onSourcesShown).toHaveBeenCalledTimes(1);
    expect(screen.getAllByTestId('citation')[0]).toHaveClass('is-highlighted');
  });

  it('opens the sheet on a phone to show the sources a reply points to', async () => {
    phone(true);
    const { rerender, onSourcesShown } = renderPanel({ sources: [SOURCE] });

    rerender(
      panel({ sources: [SOURCE], shownSources: { keys: [SOURCE.key] } }, mock(), onSourcesShown)
    );

    const sheet = await screen.findByRole('dialog', { name: 'Changes, media and sources' });
    await waitFor(() => expect(onSourcesShown).toHaveBeenCalledTimes(1));
    expect(within(sheet).getByTestId('citation')).toHaveClass('is-highlighted');
  });

  it('takes no room in a chat with no pull requests, media or sources', () => {
    const { container } = renderPanel();

    expect(container.innerHTML).toBe('');
  });

  it('shows the gallery only when the chat holds media', () => {
    renderPanel({ pullRequests: [PULL_REQUEST] });

    expect(
      screen.getByRole('complementary', { name: 'Changes, media and sources' })
    ).toBeInTheDocument();
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

    fireEvent.click(screen.getByRole('button', { name: 'Hide changes, media and sources' }));

    expect(screen.queryByRole('separator')).toBeNull();
    expect(document.activeElement).toBe(
      screen.getByRole('button', { name: /Show changes, media and sources/ })
    );
    expect(localStorage.getItem('zone:chat-panel:user-1:collapsed')).toBe('true');
    unmount();
    renderPanel({ media: [IMAGE] });
    const rail = screen.getByRole('button', { name: /Show changes, media and sources/ });
    expect(rail).toHaveAttribute('aria-expanded', 'false');

    fireEvent.click(rail);
    expect(separator()).toBeInTheDocument();
    expect(document.activeElement).toBe(
      screen.getByRole('button', { name: 'Hide changes, media and sources' })
    );
    expect(localStorage.getItem('zone:chat-panel:user-1:collapsed')).toBe('false');
  });

  it('opens as a sheet from its toggle, for a screen too narrow to hold it', async () => {
    renderPanel({ pullRequests: [PULL_REQUEST], media: [IMAGE] });

    const toggle = screen.getByRole('button', { name: /^Changes, media and sources/ });
    expect(toggle).toHaveTextContent('1 change, 1 media item');
    fireEvent.click(toggle);

    const sheet = await screen.findByRole('dialog', { name: 'Changes, media and sources' });
    expect(sheet).toHaveClass('chat-panel-sheet');
  });
});
