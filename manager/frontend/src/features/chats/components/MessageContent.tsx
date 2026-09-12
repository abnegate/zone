import type { ReactNode } from 'react';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { Citation } from '../types';
import { citationAnchorId } from '../utils/citations';
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
  breaks?: boolean;
}

export const UNSOURCED_LINK_NOTE = 'Link disabled: not among the sources for this reply';

export const UNRESOLVED_MARKER_NOTE =
  'Source marker unresolved: no source on this reply carries this identifier';

/// A marker reads as bracketed hex, so the source it names is spelled out for
/// anyone listening rather than looking.
export const REFERENCE_NOTE = 'source';

const EXTERNAL = /^https?:\/\//i;

/// Retrieval namespaces a source marker can name. The kind is part of the
/// identifier, not decoration: two namespaces may digest to the same value.
const MARKER_KINDS = ['web', 'doc', 'kb', 'chat'] as const;

/// `[web:a3f21c]` — what the model writes to cite a source it was given.
const MARKER = new RegExp(`\\[(${MARKER_KINDS.join('|')}):([0-9a-fA-F]{6,32})\\]`, 'g');

const REFERENCE_CLASS = 'message-md-citation-ref';
const UNRESOLVED_CLASS = 'message-md-citation-unresolved';

/// Markers resolve by identifier, never by position: a citation list's order is
/// not a contract, and a reordered list would silently re-point every marker.
function identified(citations: readonly Citation[]): Map<string, Citation> {
  const index = new Map<string, Citation>();
  for (const citation of citations) {
    const identifier = citation.identifier?.trim().toLowerCase();
    if (identifier && !index.has(identifier)) index.set(identifier, citation);
  }
  return index;
}

interface MarkdownNode {
  type: string;
  value?: string;
  children?: MarkdownNode[];
  data?: {
    hName?: string;
    hProperties?: Record<string, string>;
  };
}

/// Both renderings keep the marker exactly as written. An unresolved marker is
/// a fabricated attribution, and deleting it would leave a confident sentence
/// with nothing left to check.
function markerNode(
  raw: string,
  kind: string,
  digest: string,
  index: Map<string, Citation>
): MarkdownNode {
  const key = digest.toLowerCase();
  const citation = index.get(`${kind}:${key}`) ?? index.get(key);

  if (!citation?.identifier) {
    return {
      type: 'text',
      value: raw,
      data: {
        hName: 'span',
        hProperties: { className: UNRESOLVED_CLASS, title: UNRESOLVED_MARKER_NOTE },
      },
    };
  }

  return {
    type: 'text',
    value: raw,
    data: {
      hName: 'a',
      hProperties: {
        className: REFERENCE_CLASS,
        href: `#${citationAnchorId(citation.identifier)}`,
        title: citation.title,
      },
    },
  };
}

/// Markers are rewritten on the parsed tree rather than in the raw string, so a
/// fenced block and inline code — separate node types, never text — stay byte
/// for byte what the model wrote.
function citationMarkers(citations: readonly Citation[]) {
  const index = identified(citations);

  const split = (value: string): MarkdownNode[] | null => {
    const nodes: MarkdownNode[] = [];
    let cursor = 0;

    for (const match of value.matchAll(MARKER)) {
      const start = match.index;
      if (start > cursor) nodes.push({ type: 'text', value: value.slice(cursor, start) });
      nodes.push(markerNode(match[0], match[1], match[2], index));
      cursor = start + match[0].length;
    }

    if (nodes.length === 0) return null;
    if (cursor < value.length) nodes.push({ type: 'text', value: value.slice(cursor) });
    return nodes;
  };

  /// A marker inside a link's own label is left as written. Rewriting it there
  /// would nest an anchor inside an anchor, which is invalid markup and gives a
  /// reader two overlapping click targets with no way to reach either reliably.
  const walk = (node: MarkdownNode): void => {
    if (node.type === 'link' || node.type === 'linkReference') return;
    if (!node.children) return;
    const rewritten: MarkdownNode[] = [];

    for (const child of node.children) {
      const parts =
        child.type === 'text' && typeof child.value === 'string' && !child.data
          ? split(child.value)
          : null;
      if (parts) {
        rewritten.push(...parts);
        continue;
      }
      walk(child);
      rewritten.push(child);
    }

    node.children = rewritten;
  };

  return () => (tree: MarkdownNode) => {
    walk(tree);
  };
}

/// Markdown folds a single newline into a space, so text written as separate
/// lines renders as one run-on paragraph. An answer to a question card is one
/// line per question answered, and folded together two answers read as a single
/// sentence with nothing between them.
///
/// Rewritten on the parsed tree for the same reason markers are: a fenced block
/// and inline code are their own node types, never text, so whatever the writer
/// laid out inside them keeps its own lines.
function softBreaks() {
  const split = (value: string): MarkdownNode[] | null => {
    const lines = value.split('\n');
    if (lines.length === 1) return null;

    const nodes: MarkdownNode[] = [];
    for (const [index, line] of lines.entries()) {
      if (index > 0) nodes.push({ type: 'break' });
      if (line) nodes.push({ type: 'text', value: line });
    }
    return nodes;
  };

  const walk = (node: MarkdownNode): void => {
    if (!node.children) return;
    const rewritten: MarkdownNode[] = [];

    for (const child of node.children) {
      const parts =
        child.type === 'text' && typeof child.value === 'string' && !child.data
          ? split(child.value)
          : null;
      if (parts) {
        rewritten.push(...parts);
        continue;
      }
      walk(child);
      rewritten.push(child);
    }

    node.children = rewritten;
  };

  return () => (tree: MarkdownNode) => {
    walk(tree);
  };
}

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
export function MessageContent({
  content,
  links,
  citations,
  compact,
  breaks,
}: MessageContentProps) {
  const sources = citations ?? [];
  /// Every retrieval path now registers what it returned and mints a citation
  /// for it, so an empty list means nothing was retrieved rather than that the
  /// sources have not caught up. A link on such a reply has nothing behind it
  /// and is de-linked like any other unsourced one.
  const resolving = links === 'citations';
  /// Markers are read on assistant replies whether or not the citations have
  /// settled yet, so a streaming marker shows as unresolved and then resolves
  /// in place. Reasoning and user text carry no marker convention.
  const marking = links === 'citations';

  return (
    <div className={compact ? 'message-markdown message-markdown--compact' : 'message-markdown'}>
      <Markdown
        remarkPlugins={[
          remarkGfm,
          ...(marking ? [citationMarkers(sources)] : []),
          ...(breaks ? [softBreaks()] : []),
        ]}
        components={{
          a: ({ href, title, className, children }) => {
            if (className === REFERENCE_CLASS) {
              return (
                <a className={className} href={href} title={title} data-testid="citation-reference">
                  {children}
                  <span className="sr-only">
                    {' '}
                    ({REFERENCE_NOTE}: {title})
                  </span>
                </a>
              );
            }
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
          span: ({ className, title, children }) => {
            if (className !== UNRESOLVED_CLASS)
              return <span className={className}>{children}</span>;
            return (
              <span className={className} title={title} data-testid="unresolved-marker">
                {children}
                <span className="sr-only"> ({UNRESOLVED_MARKER_NOTE})</span>
              </span>
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
