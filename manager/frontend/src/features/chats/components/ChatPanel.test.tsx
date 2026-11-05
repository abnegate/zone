import { afterEach, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ChatPullRequests } from '../hooks/useChatPullRequests';
import type { ChatPullRequest } from '../types';
import type { GalleryItem } from '../utils/gallery';
import { PANEL_WIDTH } from '../utils/panel';
import type { ChatSource, ChatSources, SourceRequest } from '../utils/sources';
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

const CITED: ChatSources = {
  all: [SOURCE],
  replies: new Map([[SOURCE.messageId, [SOURCE]]]),
};

const NO_SOURCES: ChatSources = { all: [], replies: new Map() };

const COLLAPSED_KEY = 'zone:chat-panel:user-1:collapsed';
const WIDTH_KEY = 'zone:chat-panel:user-1:width';
const SEPARATOR = 'Resize changes, media and sources';
const ORDER_KEY = 'zone:chat-panel:user-1:order';
const SLOT_HEIGHT = 120;

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
  sources?: ChatSources;
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
      sources={options.sources ?? NO_SOURCES}
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

function phone() {
  const narrow = document.createElement('style');
  narrow.textContent = '.chat-panel { display: none; }';
  document.head.append(narrow);
  return () => narrow.remove();
}

function request(): SourceRequest {
  return { messageId: SOURCE.messageId, key: SOURCE.key };
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
    renderPanel({ sources: CITED });

    const aside = screen.getByRole('complementary', { name: 'Changes, media and sources' });
    expect(within(aside).getByRole('button', { name: 'Sources, 1' })).toBeInTheDocument();
    expect(within(aside).getByText(SOURCE.citation.title)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Changes, \d/ })).toBeNull();
    expect(screen.queryByRole('button', { name: /^Gallery, \d/ })).toBeNull();
    expect(
      screen.getByRole('button', { name: 'Changes, media and sources: 1 source' })
    ).toHaveTextContent(/^1 source$/);
  });

  it('comes back from its rail to show the sources a reply points to', () => {
    localStorage.setItem(COLLAPSED_KEY, 'true');
    const { rerender, onSourcesShown } = renderPanel({ sources: CITED });
    expect(screen.queryByRole('separator')).toBeNull();

    rerender(panel({ sources: CITED, shownSources: request() }, mock(), onSourcesShown));

    expect(separator()).toBeInTheDocument();
    expect(onSourcesShown).toHaveBeenCalledTimes(1);
    expect(screen.getAllByTestId('citation')[0]).toHaveClass('is-highlighted');
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it("keeps the viewer's choice to hide the panel when it opens to show sources", () => {
    localStorage.setItem(COLLAPSED_KEY, 'true');
    const { rerender, unmount } = renderPanel({ sources: CITED });

    rerender(panel({ sources: CITED, shownSources: request() }));

    expect(separator()).toBeInTheDocument();
    expect(localStorage.getItem(COLLAPSED_KEY)).toBe('true');
    unmount();
    renderPanel({ sources: CITED });
    expect(screen.queryByRole('separator')).toBeNull();
  });

  it('opens the sheet where the stylesheet hides the panel, to show the sources a reply points to', async () => {
    const restore = phone();
    try {
      const { rerender, onSourcesShown } = renderPanel({ sources: CITED });

      rerender(panel({ sources: CITED, shownSources: request() }, mock(), onSourcesShown));

      const sheet = await screen.findByRole('dialog', { name: 'Changes, media and sources' });
      await waitFor(() => expect(onSourcesShown).toHaveBeenCalledTimes(1));
      expect(within(sheet).getByTestId('citation')).toHaveClass('is-highlighted');
      expect(localStorage.getItem(COLLAPSED_KEY)).toBeNull();
    } finally {
      restore();
    }
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

  it('returns focus to whatever opened the sheet when it closes', async () => {
    const restore = phone();
    const opener = document.createElement('button');
    document.body.append(opener);
    try {
      const { rerender } = renderPanel({ sources: CITED });
      opener.focus();

      rerender(panel({ sources: CITED, shownSources: request() }));
      const sheet = await screen.findByRole('dialog', { name: 'Changes, media and sources' });
      fireEvent.keyDown(sheet, { key: 'Escape' });

      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
      await waitFor(() => expect(document.activeElement).toBe(opener));
    } finally {
      opener.remove();
      restore();
    }
  });

  it('keeps a box in the panel in step with the same box collapsed in the sheet', async () => {
    renderPanel({ sources: CITED });
    const inline = within(
      screen.getByRole('complementary', { name: 'Changes, media and sources' })
    ).getByRole('button', { name: 'Sources, 1' });
    expect(inline).toHaveAttribute('aria-expanded', 'true');

    fireEvent.click(screen.getByRole('button', { name: /^Changes, media and sources: / }));
    const sheet = await screen.findByRole('dialog', { name: 'Changes, media and sources' });
    fireEvent.click(within(sheet).getByRole('button', { name: 'Sources, 1' }));

    expect(inline).toHaveAttribute('aria-expanded', 'false');
  });

  it('opens as a sheet from its toggle, for a screen too narrow to hold it', async () => {
    renderPanel({ pullRequests: [PULL_REQUEST], media: [IMAGE] });

    const toggle = screen.getByRole('button', {
      name: 'Changes, media and sources: 1 change, 1 media item',
    });
    expect(toggle).toHaveTextContent(/^1 change, 1 media item$/);
    fireEvent.click(toggle);

    const sheet = await screen.findByRole('dialog', { name: 'Changes, media and sources' });
    expect(sheet).toHaveClass('chat-panel-sheet');
  });

  describe('box order', () => {
    const everything = { pullRequests: [PULL_REQUEST], media: [IMAGE], sources: CITED };
    let measure: ReturnType<typeof spyOn>;

    beforeEach(() => {
      const original = HTMLElement.prototype.getBoundingClientRect;
      measure = spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
        this: HTMLElement
      ) {
        const slot = this.closest('.chat-panel-drag-overlay')
          ? document.querySelector<HTMLElement>('.chat-panel-slot[data-dragging]')
          : this.closest<HTMLElement>('.chat-panel-slot');
        if (!slot?.parentElement) return original.call(this);
        const index = Array.from(slot.parentElement.children).indexOf(slot);
        const top = index * (SLOT_HEIGHT + 12);
        const bottom = top + SLOT_HEIGHT;
        return {
          x: 0,
          y: top,
          top,
          bottom,
          left: 0,
          right: 300,
          width: 300,
          height: SLOT_HEIGHT,
        } as DOMRect;
      });
    });

    afterEach(() => {
      measure.mockRestore();
    });

    function shownOrder(within: HTMLElement = inPanel()): string[] {
      return Array.from(within.querySelectorAll<HTMLElement>('.chat-panel-slot')).map(
        (slot) => slot.dataset.box ?? ''
      );
    }

    function inPanel(): HTMLElement {
      return screen.getByRole('complementary', { name: 'Changes, media and sources' });
    }

    function handle(title: string): HTMLElement {
      return within(inPanel()).getByRole('button', { name: `Move ${title}` });
    }

    async function press(target: HTMLElement, code: string) {
      await act(async () => {
        fireEvent.keyDown(target, { code, key: code === 'Space' ? ' ' : code });
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    }

    function announced(text: string | RegExp) {
      return waitFor(() => expect(screen.getByText(text)).toBeInTheDocument());
    }

    it('shows the boxes in their default order, each with a grip to move it by', () => {
      renderPanel(everything);

      expect(shownOrder()).toEqual(['changes', 'sources', 'gallery']);
      const grip = handle('Sources');
      expect(grip).toHaveAttribute('aria-roledescription', 'drag handle');
      expect(grip).toHaveAttribute('tabindex', '0');
      expect(grip.getAttribute('aria-describedby')).toBeTruthy();
      expect(
        document.getElementById(grip.getAttribute('aria-describedby') ?? '')
      ).toHaveTextContent(/press space or enter to pick it up/);
    });

    it('moves a box with the keyboard, says where it goes, and remembers the order', async () => {
      const { unmount } = renderPanel(everything);
      const grip = handle('Gallery');
      grip.focus();

      await press(grip, 'Space');
      await announced('Picked up Gallery, at position 3 of 3.');
      await press(grip, 'ArrowUp');
      await announced('Gallery would move to position 2 of 3.');
      await waitFor(() =>
        expect(inPanel().querySelector('[data-box="sources"]')).toHaveAttribute(
          'data-drop',
          'before'
        )
      );
      await press(grip, 'ArrowUp');
      await announced('Gallery would move to position 1 of 3.');
      await press(grip, 'Space');

      await announced('Gallery dropped at position 1 of 3.');
      expect(shownOrder()).toEqual(['gallery', 'changes', 'sources']);
      expect(inPanel().querySelector('[data-drop]')).toBeNull();
      expect(JSON.parse(localStorage.getItem(ORDER_KEY) ?? '[]')).toEqual([
        'gallery',
        'changes',
        'sources',
      ]);

      unmount();
      renderPanel(everything);
      expect(shownOrder()).toEqual(['gallery', 'changes', 'sources']);
    });

    it('puts a box back where it was when the move is cancelled', async () => {
      renderPanel(everything);
      const grip = handle('Changes');
      grip.focus();

      await press(grip, 'Space');
      await press(grip, 'ArrowDown');
      await announced('Changes would move to position 2 of 3.');
      await press(grip, 'Escape');

      await announced('Moving Changes cancelled. It stays at position 1 of 3.');
      expect(shownOrder()).toEqual(['changes', 'sources', 'gallery']);
      expect(localStorage.getItem(ORDER_KEY)).toBeNull();
    });

    it('keeps each viewer to their own order, and adds a box they never placed after theirs', () => {
      localStorage.setItem(ORDER_KEY, JSON.stringify(['gallery', 'retired']));

      const { unmount } = renderPanel(everything);
      expect(shownOrder()).toEqual(['gallery', 'changes', 'sources']);

      unmount();
      renderPanel({ ...everything, userId: 'user-2' });
      expect(shownOrder()).toEqual(['changes', 'sources', 'gallery']);
    });

    it('keeps a box with nothing to show in its place while the others move', async () => {
      const { rerender } = renderPanel({ pullRequests: [PULL_REQUEST], media: [IMAGE] });
      expect(shownOrder()).toEqual(['changes', 'gallery']);
      const grip = handle('Gallery');
      grip.focus();

      await press(grip, 'Space');
      await press(grip, 'ArrowUp');
      await announced('Gallery would move to position 1 of 2.');
      await press(grip, 'Space');
      await announced('Gallery dropped at position 1 of 2.');

      expect(JSON.parse(localStorage.getItem(ORDER_KEY) ?? '[]')).toEqual([
        'gallery',
        'sources',
        'changes',
      ]);
      rerender(panel(everything));
      expect(shownOrder()).toEqual(['gallery', 'sources', 'changes']);
    });

    it('still collapses a box from its header, apart from its grip', () => {
      renderPanel(everything);
      const header = within(inPanel()).getByRole('button', { name: 'Gallery, 1' });

      fireEvent.click(header);

      expect(header).toHaveAttribute('aria-expanded', 'false');
      fireEvent.click(handle('Gallery'));
      expect(header).toHaveAttribute('aria-expanded', 'false');
    });

    it('has nothing to move a lone box by', () => {
      renderPanel({ media: [IMAGE] });

      expect(within(inPanel()).queryByRole('button', { name: /^Move / })).toBeNull();
    });

    it('cancels a move in the sheet with escape, and leaves the sheet open', async () => {
      renderPanel(everything);
      fireEvent.click(screen.getByRole('button', { name: /^Changes, media and sources: / }));
      const sheet = await screen.findByRole('dialog', { name: 'Changes, media and sources' });
      const grip = within(sheet).getByRole('button', { name: 'Move Changes' });
      grip.focus();

      await press(grip, 'Space');
      await press(grip, 'ArrowDown');
      await press(grip, 'Escape');

      await announced('Moving Changes cancelled. It stays at position 1 of 3.');
      expect(screen.getByRole('dialog', { name: 'Changes, media and sources' })).toBe(sheet);
      expect(shownOrder(sheet)).toEqual(['changes', 'sources', 'gallery']);

      await press(sheet, 'Escape');
      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    });

    it('keeps every move when the browser refuses to store the order', async () => {
      spyOn(localStorage, 'setItem').mockImplementation(() => {
        throw new Error('denied');
      });
      const { rerender } = renderPanel(everything);
      const move = async (title: string, place: string) => {
        const grip = handle(title);
        grip.focus();
        await press(grip, 'Space');
        await press(grip, 'ArrowUp');
        await press(grip, 'Space');
        await announced(`${title} dropped at ${place}.`);
      };

      await move('Sources', 'position 1 of 3');
      rerender(panel({ pullRequests: [PULL_REQUEST], media: [IMAGE] }));
      await move('Gallery', 'position 1 of 2');
      rerender(panel(everything));

      expect(shownOrder()).toEqual(['sources', 'gallery', 'changes']);
    });

    it('keeps the grip out of the box heading', () => {
      renderPanel(everything);

      for (const heading of within(inPanel()).getAllByRole('heading', { level: 4 })) {
        expect(within(heading).queryByRole('button', { name: /^Move / })).toBeNull();
      }
      expect(handle('Gallery').closest('h4')).toBeNull();
    });

    it('announces moves without leaving a status on the page', async () => {
      renderPanel(everything);
      expect(screen.queryByRole('status')).toBeNull();
      const grip = handle('Gallery');
      grip.focus();

      await press(grip, 'Space');
      await announced('Picked up Gallery, at position 3 of 3.');

      expect(screen.queryByRole('status')).toBeNull();
      expect(screen.getByText('Picked up Gallery, at position 3 of 3.')).toHaveAttribute(
        'aria-live',
        'assertive'
      );
      await press(grip, 'Escape');
    });

    it('follows an order saved in another tab, and a cleared one', () => {
      renderPanel(everything);

      localStorage.setItem(ORDER_KEY, JSON.stringify(['gallery', 'sources', 'changes']));
      act(() => {
        window.dispatchEvent(new StorageEvent('storage', { key: ORDER_KEY }));
      });
      expect(shownOrder()).toEqual(['gallery', 'sources', 'changes']);

      localStorage.clear();
      act(() => {
        window.dispatchEvent(new StorageEvent('storage', { key: null }));
      });
      expect(shownOrder()).toEqual(['changes', 'sources', 'gallery']);
    });

    it('lets escape close a sheet that was shut while a box was lifted', async () => {
      renderPanel(everything);
      const toggle = screen.getByRole('button', { name: /^Changes, media and sources: / });
      fireEvent.click(toggle);
      const sheet = await screen.findByRole('dialog', { name: 'Changes, media and sources' });
      const grip = within(sheet).getByRole('button', { name: 'Move Changes' });
      grip.focus();
      await press(grip, 'Space');
      await announced('Picked up Changes, at position 1 of 3.');

      fireEvent.click(within(sheet).getByRole('button', { name: 'Close' }));
      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
      fireEvent.click(toggle);
      const reopened = await screen.findByRole('dialog', { name: 'Changes, media and sources' });
      await press(reopened, 'Escape');

      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    });

    it('shows the order the viewer chose in the sheet as well', async () => {
      localStorage.setItem(ORDER_KEY, JSON.stringify(['sources', 'gallery', 'changes']));
      renderPanel(everything);

      fireEvent.click(screen.getByRole('button', { name: /^Changes, media and sources: / }));
      const sheet = await screen.findByRole('dialog', { name: 'Changes, media and sources' });

      expect(shownOrder(sheet)).toEqual(['sources', 'gallery', 'changes']);
      expect(within(sheet).getByRole('button', { name: 'Move Sources' })).toBeInTheDocument();
    });
  });
});
