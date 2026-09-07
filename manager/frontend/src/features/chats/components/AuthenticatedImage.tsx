import { type ImgHTMLAttributes, type MouseEvent, useEffect, useState } from 'react';
import { fetchProtectedImage, isProtectedArtifactUrl } from '../api/protectedImages';

interface AuthenticatedImageProps extends Omit<ImgHTMLAttributes<HTMLImageElement>, 'src' | 'alt'> {
  src: string;
  alt: string;
  accessToken?: string | null;
  openLabel?: string;
  linkClassName?: string;
  linked?: boolean;
  compact?: boolean;
}

interface LoadedImage {
  source: string;
  objectUrl: string;
}

export function AuthenticatedImage({
  src,
  alt,
  accessToken,
  openLabel = `Open ${alt || 'image'} full size`,
  linkClassName,
  linked = true,
  compact = false,
  ...imageProps
}: AuthenticatedImageProps) {
  const protectedArtifact = isProtectedArtifactUrl(src);
  const [loadedImage, setLoadedImage] = useState<LoadedImage | null>(null);
  const [failedSource, setFailedSource] = useState<string | null>(null);

  useEffect(() => {
    if (!protectedArtifact) {
      return;
    }

    const controller = new AbortController();
    let objectUrl: string | null = null;

    fetchProtectedImage(src, controller.signal, accessToken)
      .then((blob) => {
        if (controller.signal.aborted) {
          return;
        }
        objectUrl = URL.createObjectURL(blob);
        if (controller.signal.aborted) {
          URL.revokeObjectURL(objectUrl);
          objectUrl = null;
          return;
        }
        setLoadedImage({ source: src, objectUrl });
        setFailedSource(null);
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          setFailedSource(src);
        }
      });

    return () => {
      controller.abort();
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
      }
    };
  }, [accessToken, protectedArtifact, src]);

  if (protectedArtifact && failedSource === src) {
    return (
      <span
        className={compact ? 'attachment-chip-thumb-fallback' : 'message-image-error'}
        role="alert"
        aria-label="Image unavailable"
      >
        {compact ? '' : 'Image unavailable'}
      </span>
    );
  }

  const displaySrc = protectedArtifact
    ? loadedImage?.source === src
      ? loadedImage.objectUrl
      : null
    : src;

  if (!displaySrc) {
    return (
      <span
        className={compact ? 'attachment-chip-thumb-fallback' : 'message-image-loading'}
        role="status"
        aria-label="Loading image"
      >
        {compact ? '' : 'Loading image…'}
      </span>
    );
  }

  const image = <img {...imageProps} src={displaySrc} alt={alt} />;
  if (!linked) {
    return image;
  }

  const openFullSize = async (event: MouseEvent<HTMLAnchorElement>) => {
    if (!protectedArtifact) {
      return;
    }
    event.preventDefault();
    const blob = await fetchProtectedImage(src, undefined, accessToken);
    const url = URL.createObjectURL(blob);
    const opened = window.open(url, '_blank', 'noopener,noreferrer');
    if (!opened) {
      URL.revokeObjectURL(url);
    }
  };

  return (
    <a
      className={linkClassName}
      href={protectedArtifact ? src : displaySrc}
      target="_blank"
      rel="noreferrer"
      aria-label={openLabel}
      onClick={openFullSize}
    >
      {image}
    </a>
  );
}
