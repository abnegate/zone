import { MessageContent } from './MessageContent';

export function Reasoning({ content }: { content: string }) {
  const text = content.trim();
  if (!text) {
    return null;
  }

  return (
    <details className="message-reasoning">
      <summary>Reasoning</summary>
      <div className="message-reasoning-body">
        <MessageContent content={text} />
      </div>
    </details>
  );
}
