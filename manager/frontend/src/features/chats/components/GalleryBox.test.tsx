import { afterEach, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { GalleryItem } from '../utils/gallery';
import { GalleryBox } from './GalleryBox';

const ITEMS: GalleryItem[] = [
  {
    key: 'message-1:0',
    messageId: 'message-1',
    name: 'sunset.png',
    mime: 'image/png',
    url: 'data:image/png;base64,iVBORw0KGgo=',
    kind: 'image',
  },
  {
    key: 'message-2:0',
    messageId: 'message-2',
    name: 'harbour.png',
    mime: 'image/png',
    url: 'data:image/png;base64,iVBORw0KGgoAAAA=',
    kind: 'image',
  },
  {
    key: 'message-3:0',
    messageId: 'message-3',
    name: 'theme.mp3',
    mime: 'audio/mpeg',
    url: 'https://cdn.example/theme.mp3',
    kind: 'audio',
  },
];

const watched: Array<{ element: Element; report: IntersectionObserverCallback }> = [];

class WatchingObserver {
  constructor(private readonly report: IntersectionObserverCallback) {}

  observe(element: Element) {
    watched.push({ element, report: this.report });
  }

  disconnect() {
    return;
  }
}

function scrollIntoView(element: Element) {
  act(() => {
    for (const entry of watched.filter((candidate) => candidate.element === element)) {
      entry.report(
        [{ isIntersecting: true, target: element } as IntersectionObserverEntry],
        {} as IntersectionObserver
      );
    }
  });
}

const originalObserver = globalThis.IntersectionObserver;

function renderGallery() {
  const onShowMessage = mock();
  render(<GalleryBox items={ITEMS} userId="user-1" onShowMessage={onShowMessage} />);
  return { onShowMessage };
}

async function opened(name: string): Promise<HTMLElement> {
  fireEvent.click(screen.getByRole('button', { name: `Open ${name}` }));
  return screen.findByRole('dialog');
}

describe('GalleryBox', () => {
  beforeEach(() => {
    localStorage.clear();
    watched.length = 0;
    globalThis.IntersectionObserver =
      WatchingObserver as unknown as typeof globalThis.IntersectionObserver;
  });

  afterEach(() => {
    globalThis.IntersectionObserver = originalObserver;
    mock.restore();
  });

  it('lays every item out as a tile, loading an image only once its tile nears view', () => {
    renderGallery();

    expect(screen.getByRole('button', { name: 'Gallery, 3' })).toBeInTheDocument();
    for (const item of ITEMS) {
      expect(screen.getByRole('button', { name: `Open ${item.name}` })).toBeInTheDocument();
    }
    const tile = screen.getByRole('button', { name: 'Open sunset.png' });
    expect(tile.querySelector('img')).toBeNull();

    scrollIntoView(tile);

    const thumbnail = tile.querySelector('img.chat-gallery-thumb');
    expect(thumbnail).toHaveAttribute('src', ITEMS[0].url);
    expect(thumbnail).toHaveAttribute('loading', 'lazy');
    expect(
      screen.getByRole('button', { name: 'Open harbour.png' }).querySelector('img')
    ).toBeNull();
  });

  it('opens an item full size, steps through the rest with the arrow keys and closes on Escape', async () => {
    renderGallery();

    const dialog = await opened('sunset.png');
    expect(within(dialog).getByRole('heading', { name: 'sunset.png' })).toBeInTheDocument();
    expect(within(dialog).getByText('1 of 3')).toBeInTheDocument();
    expect(within(dialog).getByRole('img', { name: 'sunset.png' })).toHaveAttribute(
      'src',
      ITEMS[0].url
    );

    fireEvent.keyDown(dialog, { key: 'ArrowRight' });
    expect(within(dialog).getByText('2 of 3')).toBeInTheDocument();
    expect(within(dialog).getByRole('heading', { name: 'harbour.png' })).toBeInTheDocument();

    fireEvent.keyDown(dialog, { key: 'ArrowLeft' });
    fireEvent.keyDown(dialog, { key: 'ArrowLeft' });
    expect(within(dialog).getByText('3 of 3')).toBeInTheDocument();

    fireEvent.keyDown(dialog, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('steps with its buttons too', async () => {
    renderGallery();

    const dialog = await opened('harbour.png');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Next item' }));
    expect(within(dialog).getByText('3 of 3')).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Previous item' }));
    expect(within(dialog).getByText('2 of 3')).toBeInTheDocument();
  });

  it('takes the reader back to the message an item came from', async () => {
    const { onShowMessage } = renderGallery();

    const dialog = await opened('harbour.png');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Show in chat' }));

    expect(onShowMessage).toHaveBeenCalledWith('message-2');
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('downloads an item under its own name', async () => {
    const click = spyOn(window.HTMLAnchorElement.prototype, 'click').mockImplementation(
      () => undefined
    );
    renderGallery();

    const dialog = await opened('sunset.png');
    await act(async () => {
      fireEvent.click(within(dialog).getByRole('button', { name: 'Download' }));
    });

    expect(click).toHaveBeenCalledTimes(1);
    const anchor = click.mock.contexts[0] as unknown as HTMLAnchorElement;
    expect(anchor.download).toBe('sunset.png');
    expect(anchor.href).toBe(ITEMS[0].url);
  });
});
