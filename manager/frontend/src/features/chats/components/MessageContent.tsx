import type { ReactNode } from 'react';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { Citation } from '../types';
import { resolvesToCitation } from '../utils/links';
import { AuthenticatedImage } from './AuthenticatedImage';

/// Which anchors in a reply a reader is allowed to follow. No default: every
/// call site has to say what it is rendering, because getting this wrong is
/// what turns an invented URL into a click.
export type MessageLinks = 'all' | 'citations' | 'none';

interface MessageContentProps {
  content: string;
  links: MessageLinks;
  citations?: Citation[];
  compact?: boolean;
}

export const UNSOURCED_LINK_NOTE = 'Link disabled: not among the sources for this reply';

const EXTERNAL = /^https?:\/\//i;

function Anchor({ href, title, children }: { href?: string; title?: string; children: ReactNode }) {
  const external = Boolean(href && EXTERNAL.test(href));
  return (
    <a
      href={href}
      title={title}
      {...(external ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
    >
      {children}
    </a>
  );
}

/// De-linked, never deleted. Dropping the text would let an invented source
/// read as an ordinary unsourced sentence; keeping the anchor is the hole.
function UnsourcedLink({ children }: { children: ReactNode }) {
  return (
    <span
      className="message-md-link-unsourced"
      data-testid="unsourced-link"
      title={UNSOURCED_LINK_NOTE}
    >
      {children}
      <span className="sr-only"> ({UNSOURCED_LINK_NOTE})</span>
    </span>
  );
}

// Assistant replies are markdown. react-markdown renders no raw HTML unless a
// rehype plugin enables it, so model output cannot inject markup here. Anchors
// are the exception: GitHub-flavoured autolinking turns any bare URL the model
// invents into one, so they are resolved against the reply's own sources.
export function MessageContent({ content, links, citations, compact }: MessageContentProps) {
  const sources = citations ?? [];
  /// Web search mints no citations yet, so resolving against an empty list
  /// would de-link every legitimate link. Once it mints them, drop the
  /// `&& sources.length > 0` and the guard becomes unconditional.
  const resolving = links === 'citations' && sources.length > 0;

  return (
    <div className={compact ? 'message-markdown message-markdown--compact' : 'message-markdown'}>
      <Markdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, title, children }) => {
            if (links === 'none') return <>{children}</>;
            if (resolving && !resolvesToCitation(href, sources)) {
              return <UnsourcedLink>{children}</UnsourcedLink>;
            }
            return (
              <Anchor href={href} title={title}>
                {children}
              </Anchor>
            );
          },
          img: ({ src, alt, title }) =>
            src ? (
              <AuthenticatedImage
                src={src}
                alt={alt ?? ''}
                title={title}
                className="message-md-image"
              />
            ) : null,
          ...(compact ? { blockquote: ({ children }) => children } : {}),
        }}
      >
        {content}
      </Markdown>
    </div>
  );
}
