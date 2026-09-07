import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { AuthenticatedImage } from './AuthenticatedImage';

interface MessageContentProps {
  content: string;
  compact?: boolean;
}

// Assistant replies are markdown. react-markdown renders no raw HTML unless a
// rehype plugin enables it, so model output cannot inject markup here.
export function MessageContent({ content, compact }: MessageContentProps) {
  return (
    <div className={compact ? 'message-markdown message-markdown--compact' : 'message-markdown'}>
      <Markdown
        remarkPlugins={[remarkGfm]}
        components={{
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
