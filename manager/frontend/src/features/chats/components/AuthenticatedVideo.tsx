import { useEffect, useState, type VideoHTMLAttributes } from 'react';
import { fetchProtectedImage, isProtectedArtifactUrl } from '../api/protectedImages';

interface AuthenticatedVideoProps
  extends Omit<VideoHTMLAttributes<HTMLVideoElement>, 'src' | 'aria-label'> {
  src: string;
  label: string;
  accessToken?: string | null;
}

interface LoadedVideo {
  source: string;
  objectUrl: string;
}

export function AuthenticatedVideo({
  src,
  label,
  accessToken,
  ...videoProps
}: AuthenticatedVideoProps) {
  const protectedArtifact = isProtectedArtifactUrl(src);
  const [loadedVideo, setLoadedVideo] = useState<LoadedVideo | null>(null);
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
        setLoadedVideo({ source: src, objectUrl });
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
      <span className="message-image-error" role="alert" aria-label="Video unavailable">
        Video unavailable
      </span>
    );
  }

  const displaySrc = protectedArtifact
    ? loadedVideo?.source === src
      ? loadedVideo.objectUrl
      : null
    : src;

  if (!displaySrc) {
    return (
      <span className="message-image-loading" role="status" aria-label="Loading video">
        Loading video…
      </span>
    );
  }

  return (
    <video
      {...videoProps}
      className={['message-video', videoProps.className].filter(Boolean).join(' ')}
      src={displaySrc}
      controls
      playsInline
      preload="metadata"
      aria-label={label}
    />
  );
}
