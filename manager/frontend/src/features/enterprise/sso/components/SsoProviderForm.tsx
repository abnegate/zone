import { Button, Input, Modal } from '@zone/ui';
import { type FormEvent, useId, useState } from 'react';
import { Switch } from '../../components/Switch';
import { UpgradeNotice } from '../../components/UpgradeNotice';
import {
  changed,
  createRequest,
  type DraftErrors,
  draftErrors,
  draftOf,
  emptyDraft,
  type ProviderDraft,
  parseDomains,
  updateRequest,
} from '../draft';
import { useCreateSsoProvider, useUpdateSsoProvider } from '../hooks';
import {
  ProviderField,
  protocolName,
  refusalField,
  refusalMessage,
  UPGRADE_NOTICE,
} from '../messages';
import { PROTOCOLS, Protocol, SsoAdminCode } from '../schemas';
import type { ProviderResponse } from '../types';
import { ProviderFacts } from './ProviderFacts';
import { SamlFields } from './SamlFields';
import { SsoChoice } from './SsoChoice';

export const PROTOCOL_LABEL = 'Protocol';

export const KEEP_SECRET_PLACEHOLDER = 'Leave blank to keep the current secret';

export const EMPTY_DOMAINS_HINT =
  'Anyone your identity provider lets sign in gets a Zone account. Set allowed domains to limit this.';

export const DOMAINS_HINT = 'Only people with an email on these domains can sign in.';

export const SAML_DOMAINS_HINT =
  "Required for SAML, which doesn't say whether an email is verified. Only people with an email on these domains can sign in.";

export const SAML_LINKING_NOTE =
  "Zone never joins a SAML sign-in to an existing Zone account by email, so a SAML provider can't take an account over.";

export const VERIFIED_EMAIL_LABEL = 'Require verified email';

export const VERIFIED_EMAIL_DESCRIPTION =
  "Only accept emails your identity provider marks as verified. Turn this off for Microsoft Entra ID, which does not send that claim, and set allowed domains instead. While it's off, a sign-in never joins an existing Zone account by email, so it can't take one over.";

export const ENABLED_LABEL = 'Enabled';

export const ENABLED_DESCRIPTION = 'Show this provider on the sign-in page.';

const NO_ERRORS: DraftErrors = {};

const PROTOCOL_CHOICES = PROTOCOLS.map((protocol) => ({
  value: protocol,
  label: protocolName(protocol),
}));

interface SsoProviderFormProps {
  provider?: ProviderResponse;
  onClose: () => void;
}

export function SsoProviderForm({ provider, onClose }: SsoProviderFormProps) {
  const base = useId();
  const creation = useCreateSsoProvider();
  const update = useUpdateSsoProvider();
  const [draft, setDraft] = useState<ProviderDraft>(() =>
    provider ? draftOf(provider) : emptyDraft()
  );
  const [attempted, setAttempted] = useState(false);

  const mutation = provider ? update : creation;
  const { pending, refusal, error } = mutation;
  const saml = draft.protocol === Protocol.Saml;
  const errors = attempted ? draftErrors(draft, provider) : NO_ERRORS;
  const unchanged = provider !== undefined && !changed(updateRequest(provider, draft));
  const refusedField = refusal ? refusalField(refusal.code, draft.protocol) : null;
  const licenseRequired = refusal?.code === SsoAdminCode.LicenseRequired;
  const formError =
    licenseRequired || refusedField !== null
      ? null
      : refusal
        ? refusalMessage(refusal.code, refusal.message)
        : error;

  const fieldError = (field: ProviderField): string | undefined => {
    if (refusal && refusedField === field) {
      return refusalMessage(refusal.code, refusal.message);
    }
    return errors[field];
  };

  const change = <K extends keyof ProviderDraft>(key: K, value: ProviderDraft[K]) => {
    setDraft((current) => ({ ...current, [key]: value }));
    if (error) {
      mutation.reset();
    }
  };

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (pending || unchanged) {
      return;
    }
    setAttempted(true);
    if (Object.keys(draftErrors(draft, provider)).length > 0) {
      return;
    }
    if (provider) {
      update.update(provider.id, updateRequest(provider, draft), onClose);
    } else {
      creation.create(createRequest(draft), onClose);
    }
  };

  const domainsEmpty = parseDomains(draft.domains).length === 0;
  const domainsHint = saml ? SAML_DOMAINS_HINT : domainsEmpty ? EMPTY_DOMAINS_HINT : DOMAINS_HINT;

  return (
    <Modal
      isOpen
      onClose={pending ? undefined : onClose}
      title={provider ? `Edit ${provider.display_name}` : 'Add identity provider'}
      size="md"
    >
      <form className="ui-form sso-provider-form" onSubmit={submit} noValidate>
        <fieldset className="sso-provider-fields" disabled={pending}>
          {provider ? (
            <ProviderFacts provider={provider} />
          ) : (
            <SsoChoice
              legend={PROTOCOL_LABEL}
              value={draft.protocol}
              choices={PROTOCOL_CHOICES}
              onChange={(protocol) => change('protocol', protocol)}
            />
          )}
          <Input
            id={`${base}-display-name`}
            label="Display name"
            value={draft.displayName}
            autoComplete="off"
            placeholder="Okta"
            helpText="Shown on the sign-in button."
            error={fieldError(ProviderField.DisplayName)}
            onChange={(event) => change('displayName', event.target.value)}
          />
          {saml ? (
            <SamlFields
              base={base}
              draft={draft}
              provider={provider}
              pending={pending}
              error={fieldError(ProviderField.Metadata)}
              onChange={change}
            />
          ) : (
            <>
              <Input
                id={`${base}-issuer`}
                label="Issuer URL"
                type="url"
                inputMode="url"
                value={draft.issuer}
                autoComplete="off"
                spellCheck={false}
                placeholder="https://acme.okta.com"
                error={fieldError(ProviderField.Issuer)}
                onChange={(event) => change('issuer', event.target.value)}
              />
              <Input
                id={`${base}-client-id`}
                label="Client ID"
                value={draft.clientId}
                autoComplete="off"
                spellCheck={false}
                error={fieldError(ProviderField.ClientId)}
                onChange={(event) => change('clientId', event.target.value)}
              />
              <Input
                id={`${base}-client-secret`}
                label="Client secret"
                type="password"
                value={draft.clientSecret}
                autoComplete="new-password"
                spellCheck={false}
                placeholder={provider ? KEEP_SECRET_PLACEHOLDER : undefined}
                error={fieldError(ProviderField.ClientSecret)}
                onChange={(event) => change('clientSecret', event.target.value)}
              />
            </>
          )}
          <Input
            id={`${base}-domains`}
            label="Allowed domains"
            value={draft.domains}
            autoComplete="off"
            spellCheck={false}
            placeholder="acme.com, acme.co.uk"
            helpText={domainsHint}
            error={fieldError(ProviderField.AllowedDomains)}
            onChange={(event) => change('domains', event.target.value)}
          />
          {saml ? (
            <p className="sso-hint">{SAML_LINKING_NOTE}</p>
          ) : (
            <Switch
              label={VERIFIED_EMAIL_LABEL}
              description={VERIFIED_EMAIL_DESCRIPTION}
              checked={draft.requireVerifiedEmail}
              disabled={pending}
              onChange={(checked) => change('requireVerifiedEmail', checked)}
            />
          )}
          <Switch
            label={ENABLED_LABEL}
            description={ENABLED_DESCRIPTION}
            checked={draft.enabled}
            disabled={pending}
            onChange={(checked) => change('enabled', checked)}
          />
        </fieldset>
        {licenseRequired && <UpgradeNotice message={UPGRADE_NOTICE} className="sso-notice" />}
        {formError && (
          <div className="modal-error" role="alert">
            {formError}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" loading={pending} disabled={unchanged}>
            {provider ? 'Save' : 'Add provider'}
          </Button>
        </div>
      </form>
    </Modal>
  );
}
