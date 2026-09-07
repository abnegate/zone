import { useEffect, useState, type VideoHTMLAttributes } from 'react';
import { fetchSignedArtifactUrl, isProtectedArtifactUrl } from '../api/protectedImages';

interface AuthenticatedVideoProps
  extends Omit<VideoHTMLAttributes<HTMLVideoElement>, 'src' | 'aria-label'> {
  src: string;
  label: string;
  accessToken?: string | null;
}

interface SignedVideo {
  source: string;
  signedUrl: string;
}

export function AuthenticatedVideo({
  src,
  label,
  accessToken,
  ...videoProps
}: AuthenticatedVideoProps) {
  const protectedArtifact = isProtectedArtifactUrl(src);
  const [signedVideo, setSignedVideo] = useState<SignedVideo | null>(null);
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
        setSignedVideo({ source: src, signedUrl });
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
      <span className="message-image-error" role="alert" aria-label="Video unavailable">
        Video unavailable
      </span>
    );
  }

  const displaySrc = protectedArtifact
    ? signedVideo?.source === src
      ? signedVideo.signedUrl
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
