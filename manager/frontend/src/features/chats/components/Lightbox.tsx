import { Button, Dialog, DialogContent, DialogDescription, DialogTitle } from '@zone/ui';
import { type KeyboardEvent, useState } from 'react';
import { downloadMedia, type GalleryItem } from '../utils/gallery';
import { LightboxMedia } from './LightboxMedia';

export interface LightboxProps {
  items: readonly GalleryItem[];
  index: number | null;
  onIndexChange: (index: number | null) => void;
  onShowMessage: (messageId: string) => void;
}

/** One gallery item at full size, stepped through with the arrow keys. */
export function Lightbox({ items, index, onIndexChange, onShowMessage }: LightboxProps) {
  const [failed, setFailed] = useState<string | null>(null);
  const item = index === null ? null : (items[index] ?? null);
  const count = items.length;

  const step = (by: number) => {
    if (index === null || count === 0) return;
    setFailed(null);
    onIndexChange((index + by + count) % count);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target instanceof HTMLMediaElement) return;
    if (event.key === 'ArrowRight') {
      event.preventDefault();
      step(1);
    } else if (event.key === 'ArrowLeft') {
      event.preventDefault();
      step(-1);
    }
  };

  const download = async (target: GalleryItem) => {
    setFailed(null);
    try {
      await downloadMedia(target);
    } catch {
      setFailed(`${target.name} could not be downloaded.`);
    }
  };

  return (
    <Dialog
      open={item !== null}
      onOpenChange={(open) => {
        if (!open) {
          setFailed(null);
          onIndexChange(null);
        }
      }}
    >
      {item && index !== null && (
        <DialogContent className="chat-lightbox" onKeyDown={onKeyDown}>
          <DialogTitle className="chat-lightbox-title">{item.name}</DialogTitle>
          <DialogDescription className="chat-lightbox-position">
            {index + 1} of {count}
          </DialogDescription>
          <div className="chat-lightbox-stage">
            {count > 1 && (
              <button
                type="button"
                className="chat-lightbox-step is-previous"
                aria-label="Previous item"
                onClick={() => step(-1)}
              >
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  aria-hidden="true"
                >
                  <path d="m15 6-6 6 6 6" />
                </svg>
              </button>
            )}
            <div className="chat-lightbox-media">
              <LightboxMedia item={item} />
            </div>
            {count > 1 && (
              <button
                type="button"
                className="chat-lightbox-step is-next"
                aria-label="Next item"
                onClick={() => step(1)}
              >
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  aria-hidden="true"
                >
                  <path d="m9 6 6 6-6 6" />
                </svg>
              </button>
            )}
          </div>
          <div className="chat-lightbox-actions">
            <Button variant="secondary" size="sm" onClick={() => void download(item)}>
              Download
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                onIndexChange(null);
                onShowMessage(item.messageId);
              }}
            >
              Show in chat
            </Button>
          </div>
          {failed && (
            <p className="chat-lightbox-error" role="alert">
              {failed}
            </p>
          )}
        </DialogContent>
      )}
    </Dialog>
  );
}
