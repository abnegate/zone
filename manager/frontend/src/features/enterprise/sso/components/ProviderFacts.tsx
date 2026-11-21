import { protocolName } from '../messages';
import { Protocol } from '../schemas';
import type { ProviderResponse } from '../types';

interface ProviderFactsProps {
  provider: ProviderResponse;
}

export function ProviderFacts({ provider }: ProviderFactsProps) {
  const saml = provider.protocol === Protocol.Saml;

  return (
    <dl className="sso-facts">
      <div className="sso-fact">
        <dt>Protocol</dt>
        <dd>{protocolName(provider.protocol)}</dd>
      </div>
      {saml && (
        <>
          <div className="sso-fact">
            <dt>Entity ID</dt>
            <dd className="sso-fact-value">{provider.issuer}</dd>
          </div>
          <div className="sso-fact">
            <dt>SSO URL</dt>
            <dd className="sso-fact-value">{provider.sso_url}</dd>
          </div>
          <div className="sso-fact">
            <dt>Certificate fingerprints</dt>
            {provider.certificate_fingerprints.map((fingerprint) => (
              <dd key={fingerprint} className="sso-fact-value">
                {fingerprint}
              </dd>
            ))}
          </div>
        </>
      )}
    </dl>
  );
}
