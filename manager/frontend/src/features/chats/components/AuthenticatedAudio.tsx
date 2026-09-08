import { type AudioHTMLAttributes, useEffect, useState } from 'react';
import { fetchSignedArtifactUrl, isProtectedArtifactUrl } from '../api/protectedImages';

interface AuthenticatedAudioProps
  extends Omit<AudioHTMLAttributes<HTMLAudioElement>, 'src' | 'aria-label'> {
  src: string;
  label: string;
  accessToken?: string | null;
}

interface SignedAudio {
  source: string;
  signedUrl: string;
}

export function AuthenticatedAudio({
  src,
  label,
  accessToken,
  ...audioProps
}: AuthenticatedAudioProps) {
  const protectedArtifact = isProtectedArtifactUrl(src);
  const [signedAudio, setSignedAudio] = useState<SignedAudio | null>(null);
  const [failedSource, setFailedSource] = useState<string | null>(null);

  useEffect(() => {
    if (!protectedArtifact) {
      return;
    }

    const controller = new AbortController();

    // A signed URL rather than a blob: the element must fetch the media itself
    // for the browser to issue the range requests that make it seekable.
    fetchSignedArtifactUrl(src, controller.signal, accessToken)
      .then((signedUrl) => {
        if (controller.signal.aborted) {
          return;
        }
        setSignedAudio({ source: src, signedUrl });
        setFailedSource(null);
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          setFailedSource(src);
        }
      });

    return () => {
      controller.abort();
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
    ? signedAudio?.source === src
      ? signedAudio.signedUrl
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
