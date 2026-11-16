import type { ChatProviders } from '../provider';
import { offerDenial } from '../utils/providerOptions';

export function ProviderError({ providers }: { providers: ChatProviders | null }) {
  const selected = providers?.selected;
  if (!selected || selected.usable) return null;
  return (
    <p className="chat-provider-error" role="alert" data-testid="chat-provider-error">
      {offerDenial(selected) ?? selected.reason ?? `${selected.label} can't be used for this chat.`}
    </p>
  );
}
