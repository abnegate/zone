import type { ChatProviders } from '../provider';
import { offerDenial } from '../utils/providerOptions';

function reasonOf(providers: ChatProviders | null): string | null {
  if (!providers) return null;
  const { selected } = providers;
  if (!selected) return offerDenial(providers.default);
  if (selected.usable) return null;
  return (
    offerDenial(selected) ?? selected.reason ?? `${selected.label} can't be used for this chat.`
  );
}

export function ProviderError({ providers }: { providers: ChatProviders | null }) {
  const reason = reasonOf(providers);
  if (reason === null) return null;
  return (
    <p className="chat-provider-error" role="alert" data-testid="chat-provider-error">
      {reason}
    </p>
  );
}
