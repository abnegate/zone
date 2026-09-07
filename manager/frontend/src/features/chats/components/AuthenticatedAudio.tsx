import { type AudioHTMLAttributes, useEffect, useState } from 'react';
import { fetchProtectedImage, isProtectedArtifactUrl } from '../api/protectedImages';

interface AuthenticatedAudioProps
  extends Omit<AudioHTMLAttributes<HTMLAudioElement>, 'src' | 'aria-label'> {
  src: string;
  label: string;
  accessToken?: string | null;
}

interface LoadedAudio {
  source: string;
  objectUrl: string;
}

export function AuthenticatedAudio({
  src,
  label,
  accessToken,
  ...audioProps
}: AuthenticatedAudioProps) {
  const protectedArtifact = isProtectedArtifactUrl(src);
  const [loadedAudio, setLoadedAudio] = useState<LoadedAudio | null>(null);
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
        setLoadedAudio({ source: src, objectUrl });
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
      <span className="message-image-error" role="alert" aria-label="Audio unavailable">
        Audio unavailable
      </span>
    );
  }

  const displaySrc = protectedArtifact
    ? loadedAudio?.source === src
      ? loadedAudio.objectUrl
      : null
    : src;

  if (!displaySrc) {
    return (
      <span className="message-image-loading" role="status" aria-label="Loading audio">
        Loading audio…
      </span>
    );
  }

  return (
    <audio
      {...audioProps}
      className={['message-audio', audioProps.className].filter(Boolean).join(' ')}
      src={displaySrc}
      controls
      preload="metadata"
      aria-label={label}
    />
  );
}
