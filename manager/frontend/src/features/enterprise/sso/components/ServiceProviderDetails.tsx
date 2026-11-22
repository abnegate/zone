import type { ServiceProviderResponse } from '../types';
import { CopyValue } from './CopyValue';

export const SERVICE_PROVIDER_TITLE = 'SAML service provider';

export const SERVICE_PROVIDER_HINT =
  'Give these to your identity provider when you add Zone as a SAML application. Many providers can read them all from the metadata URL.';

interface ServiceProviderDetailsProps {
  serviceProvider: ServiceProviderResponse;
}

export function ServiceProviderDetails({ serviceProvider }: ServiceProviderDetailsProps) {
  return (
    <div className="settings-card sso-service-provider">
      <h3 className="sso-card-title">{SERVICE_PROVIDER_TITLE}</h3>
      <CopyValue label="Entity ID" name="entity ID" value={serviceProvider.entity_id} />
      <CopyValue label="ACS URL" name="ACS URL" value={serviceProvider.acs_url} />
      <CopyValue label="Metadata URL" name="metadata URL" value={serviceProvider.metadata_url} />
      <p className="sso-hint">{SERVICE_PROVIDER_HINT}</p>
    </div>
  );
}
