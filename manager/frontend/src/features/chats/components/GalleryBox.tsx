import { useState } from 'react';
import type { GalleryItem } from '../utils/gallery';
import { panelKey } from '../utils/panel';
import { GalleryTile } from './GalleryTile';
import { Lightbox } from './Lightbox';
import { PanelBox } from './PanelBox';

export interface GalleryBoxProps {
  items: readonly GalleryItem[];
  userId: string | null;
  onShowMessage: (messageId: string) => void;
}

/** Every image, video and audio file in the chat, opened full size on click. */
export function GalleryBox({ items, userId, onShowMessage }: GalleryBoxProps) {
  const [open, setOpen] = useState<number | null>(null);

  return (
    <PanelBox
      title="Gallery"
      count={items.length}
      storageKey={panelKey(userId, 'gallery')}
      className="chat-gallery"
    >
      <ul className="chat-gallery-grid">
        {items.map((item, index) => (
          <GalleryTile key={item.key} item={item} onOpen={() => setOpen(index)} />
        ))}
      </ul>
      <Lightbox items={items} index={open} onIndexChange={setOpen} onShowMessage={onShowMessage} />
    </PanelBox>
  );
}
