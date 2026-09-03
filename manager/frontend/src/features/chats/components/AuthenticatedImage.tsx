import { type ImgHTMLAttributes, useEffect, useState } from 'react';
import { fetchProtectedImage, isProtectedArtifactUrl } from '../api/protectedImages';

interface AuthenticatedImageProps extends Omit<ImgHTMLAttributes<HTMLImageElement>, 'src' | 'alt'> {
  src: string;
  alt: string;
  accessToken?: string | null;
  openLabel?: string;
  linkClassName?: string;
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
      <span className="message-image-error" role="alert">
        Image unavailable
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
      <span className="message-image-loading" role="status">
        Loading image…
      </span>
    );
  }

  return (
    <a
      className={linkClassName}
      href={displaySrc}
      target="_blank"
      rel="noreferrer"
      aria-label={openLabel}
    >
      <img {...imageProps} src={displaySrc} alt={alt} />
    </a>
  );
}
