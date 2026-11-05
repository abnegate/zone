import { fetchProtectedImage, isProtectedArtifactUrl } from '../../../api/protectedImages';
import type { Message } from '../types';

export type MediaKind = 'image' | 'video' | 'audio';

export interface GalleryItem {
  key: string;
  messageId: string;
  name: string;
  mime: string;
  url: string;
  kind: MediaKind;
}

const MEDIA_KINDS: readonly MediaKind[] = ['image', 'video', 'audio'];

function mediaKind(mime: string): MediaKind | null {
  return MEDIA_KINDS.find((kind) => mime.startsWith(`${kind}/`)) ?? null;
}

/**
 * Every image, video and audio file sent to or made in the chat, oldest
 * first. A file attached again, as a starting image is, keeps the place it
 * first appeared in.
 */
export function galleryItems(messages: readonly Message[]): GalleryItem[] {
  const seen = new Set<string>();
  const items: GalleryItem[] = [];
  for (const message of messages) {
    (message.metadata?.attachments ?? []).forEach((attachment, index) => {
      const kind = mediaKind(attachment.mime);
      if (!kind || !attachment.url || seen.has(attachment.url)) return;
      seen.add(attachment.url);
      items.push({
        key: `${message.id}:${index}`,
        messageId: message.id,
        name: attachment.name || `${kind} ${items.length + 1}`,
        mime: attachment.mime,
        url: attachment.url,
        kind,
      });
    });
  }
  return items;
}

/** Saves the item under its own name, fetching a protected one with the session. */
export async function downloadMedia(item: GalleryItem): Promise<void> {
  const protectedArtifact = isProtectedArtifactUrl(item.url);
  const href = protectedArtifact
    ? URL.createObjectURL(await fetchProtectedImage(item.url))
    : item.url;
  const anchor = document.createElement('a');
  anchor.href = href;
  anchor.download = item.name;
  anchor.rel = 'noopener';
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  if (protectedArtifact) {
    setTimeout(() => URL.revokeObjectURL(href), 0);
  }
}
