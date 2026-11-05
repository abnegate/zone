import { useRef } from 'react';
import { useVisible } from '../hooks/useVisible';
import type { GalleryItem } from '../utils/gallery';
import { AuthenticatedImage } from './AuthenticatedImage';

export interface GalleryTileProps {
  item: GalleryItem;
  onOpen: () => void;
}

const KIND_ICONS: Record<'video' | 'audio', string> = {
  video: 'M8 5v14l11-7z',
  audio: 'M9 18V5l12-2v13M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0Zm12-2a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z',
};

/** One thumbnail in the gallery. An image loads only once the tile nears view. */
export function GalleryTile({ item, onOpen }: GalleryTileProps) {
  const tile = useRef<HTMLButtonElement>(null);
  const visible = useVisible(tile);

  return (
    <li className="chat-gallery-item">
      <button
        ref={tile}
        type="button"
        className={`chat-gallery-tile is-${item.kind}`}
        aria-label={`Open ${item.name}`}
        title={item.name}
        onClick={onOpen}
      >
        {item.kind === 'image' ? (
          visible ? (
            <AuthenticatedImage
              src={item.url}
              alt=""
              linked={false}
              compact
              loading="lazy"
              decoding="async"
              className="chat-gallery-thumb"
              fallbackClassName="chat-gallery-thumb-fallback"
            />
          ) : (
            <span className="chat-gallery-thumb-fallback" aria-hidden="true" />
          )
        ) : (
          <span className="chat-gallery-placeholder" aria-hidden="true">
            <svg
              viewBox="0 0 24 24"
              fill={item.kind === 'video' ? 'currentColor' : 'none'}
              stroke="currentColor"
              strokeWidth="2"
            >
              <path d={KIND_ICONS[item.kind]} />
            </svg>
            <span className="chat-gallery-placeholder-name">{item.name}</span>
          </span>
        )}
      </button>
    </li>
  );
}
