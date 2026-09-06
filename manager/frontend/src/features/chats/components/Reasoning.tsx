import { MessageContent } from './MessageContent';

export function Reasoning({ content, open }: { content: string; open?: boolean }) {
  const text = content.trim();
  if (!text) {
    return null;
  }

  return (
    <details className="message-reasoning" data-testid="reasoning" open={open || undefined}>
      <summary>Reasoning</summary>
      <div className="message-reasoning-body">
        <MessageContent content={text} />
      </div>
    </details>
  );
}
