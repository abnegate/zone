import type { GalleryItem } from '../utils/gallery';
import { AuthenticatedAudio } from './AuthenticatedAudio';
import { AuthenticatedImage } from './AuthenticatedImage';
import { AuthenticatedVideo } from './AuthenticatedVideo';

export function LightboxMedia({ item }: { item: GalleryItem }) {
  switch (item.kind) {
    case 'image':
      return (
        <AuthenticatedImage
          key={item.url}
          src={item.url}
          alt={item.name}
          linked={false}
          className="chat-lightbox-image"
        />
      );
    case 'video':
      return (
        <AuthenticatedVideo
          key={item.url}
          src={item.url}
          label={item.name}
          className="chat-lightbox-video"
        />
      );
    case 'audio':
      return <AuthenticatedAudio key={item.url} src={item.url} label={item.name} />;
  }
}
