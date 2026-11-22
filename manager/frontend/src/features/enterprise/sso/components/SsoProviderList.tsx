import { Badge, Button } from '@zone/ui';
import { protocolBadge } from '../messages';
import { Protocol } from '../schemas';
import type { ProviderResponse } from '../types';

interface SsoProviderListProps {
  providers: readonly ProviderResponse[];
  editable: boolean;
  onAdd(): void;
  onEdit(provider: ProviderResponse): void;
  onDelete(provider: ProviderResponse): void;
}

function domainsText(domains: readonly string[]): string {
  return domains.length === 0 ? 'Any email domain' : `Domains: ${domains.join(', ')}`;
}

function verificationText(provider: ProviderResponse): string {
  if (provider.protocol === Protocol.Saml) {
    return 'Never joins accounts by email';
  }
  return provider.require_verified_email ? 'Verified email required' : 'Provider emails trusted';
}

export function SsoProviderList({
  providers,
  editable,
  onAdd,
  onEdit,
  onDelete,
}: SsoProviderListProps) {
  return (
    <div className="settings-card sso-providers">
      <div className="sso-card-header">
        <h3 className="sso-card-title">Identity providers</h3>
        {editable && (
          <Button type="button" size="sm" onClick={onAdd}>
            Add provider
          </Button>
        )}
      </div>
      {providers.length === 0 ? (
        <p className="sso-hint">
          {editable
            ? 'No identity providers yet. Add one to show its button on the sign-in page.'
            : 'No identity providers are set up.'}
        </p>
      ) : (
        <ul className="sso-provider-list">
          {providers.map((provider) => (
            <li key={provider.id} className="sso-provider" aria-label={provider.display_name}>
              <div className="sso-provider-identity">
                <div className="sso-provider-heading">
                  <span className="sso-provider-name">{provider.display_name}</span>
                  <Badge variant="outline">{protocolBadge(provider.protocol)}</Badge>
                  <Badge variant={provider.enabled ? 'success' : 'neutral'}>
                    {provider.enabled ? 'Enabled' : 'Disabled'}
                  </Badge>
                </div>
                <span className="sso-provider-issuer">{provider.issuer}</span>
                <span className="sso-provider-meta">
                  {domainsText(provider.allowed_domains)} · {verificationText(provider)}
                </span>
              </div>
              {editable && (
                <div className="sso-provider-actions">
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    aria-label={`Edit ${provider.display_name}`}
                    onClick={() => onEdit(provider)}
                  >
                    Edit
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    aria-label={`Delete ${provider.display_name}`}
                    onClick={() => onDelete(provider)}
                  >
                    Delete
                  </Button>
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
