import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

interface MessageContentProps {
  content: string;
}

// Assistant replies are markdown. react-markdown renders no raw HTML unless a
// rehype plugin enables it, so model output cannot inject markup here.
export function MessageContent({ content }: MessageContentProps) {
  return (
    <div className="message-markdown">
      <Markdown remarkPlugins={[remarkGfm]}>{content}</Markdown>
    </div>
  );
}
