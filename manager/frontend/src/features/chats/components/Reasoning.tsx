import { MessageContent } from './MessageContent';

export function Reasoning({ content, open = false }: { content: string; open?: boolean }) {
  const text = content.trim();
  if (!text) {
    return null;
  }

  return (
    <div className="message-reasoning" data-testid="reasoning" hidden={!open}>
      <MessageContent content={text} links="none" compact />
    </div>
  );
}
