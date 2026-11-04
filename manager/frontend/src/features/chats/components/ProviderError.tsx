import type { ChatProviders } from '../provider';

export function ProviderError({ providers }: { providers: ChatProviders | null }) {
  const selected = providers?.selected;
  if (!selected || selected.usable) return null;
  return (
    <p className="chat-provider-error" role="alert" data-testid="chat-provider-error">
      {selected.reason ?? `${selected.label} can't be used for this chat.`}
    </p>
  );
}
