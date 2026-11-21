import { Input, Label } from '@zone/ui';
import { Switch } from '../../components/Switch';
import { MetadataSource, type ProviderDraft } from '../draft';
import type { ProviderResponse } from '../types';
import { SsoChoice } from './SsoChoice';

export const METADATA_SOURCE_LABEL = 'Metadata';

export const URL_SOURCE_LABEL = 'Read from a URL';

export const XML_SOURCE_LABEL = 'Paste XML';

export const METADATA_URL_LABEL = 'Metadata URL';

export const METADATA_XML_LABEL = 'Metadata XML';

export const METADATA_URL_HINT = 'Zone reads the metadata from this address when you save.';

export const METADATA_XML_HINT = 'Paste the whole metadata document from your identity provider.';

export const KEEP_METADATA_PLACEHOLDER = 'Leave blank to keep the current metadata';

export const REFRESH_METADATA_LABEL = 'Read the metadata again';

export const REFRESH_METADATA_DESCRIPTION =
  'Fetch the metadata URL when you save, to pick up new signing certificates from your identity provider.';

const METADATA_ROWS = 6;

const SOURCES = [
  { value: MetadataSource.Url, label: URL_SOURCE_LABEL },
  { value: MetadataSource.Xml, label: XML_SOURCE_LABEL },
] as const;

interface SamlFieldsProps {
  base: string;
  draft: ProviderDraft;
  provider?: ProviderResponse;
  pending: boolean;
  error: string | undefined;
  onChange<K extends keyof ProviderDraft>(key: K, value: ProviderDraft[K]): void;
}

export function SamlFields({ base, draft, provider, pending, error, onChange }: SamlFieldsProps) {
  const xmlId = `${base}-metadata-xml`;
  const xmlNoteId = `${xmlId}-${error ? 'error' : 'help'}`;
  const refreshable = provider?.metadata_url != null && draft.metadataSource === MetadataSource.Url;

  return (
    <>
      <SsoChoice
        legend={METADATA_SOURCE_LABEL}
        value={draft.metadataSource}
        choices={SOURCES}
        onChange={(source) => onChange('metadataSource', source)}
      />
      {draft.metadataSource === MetadataSource.Url ? (
        <Input
          id={`${base}-metadata-url`}
          label={METADATA_URL_LABEL}
          type="url"
          inputMode="url"
          value={draft.metadataUrl}
          autoComplete="off"
          spellCheck={false}
          placeholder="https://acme.okta.com/app/exk1a2b3c4/sso/saml/metadata"
          helpText={METADATA_URL_HINT}
          error={error}
          onChange={(event) => onChange('metadataUrl', event.target.value)}
        />
      ) : (
        <div className="ui-input-wrapper">
          <Label htmlFor={xmlId}>{METADATA_XML_LABEL}</Label>
          <textarea
            id={xmlId}
            className="sso-metadata-xml"
            rows={METADATA_ROWS}
            value={draft.metadata}
            spellCheck={false}
            autoComplete="off"
            placeholder={provider ? KEEP_METADATA_PLACEHOLDER : '<EntityDescriptor …>'}
            aria-invalid={!!error}
            aria-describedby={xmlNoteId}
            onChange={(event) => onChange('metadata', event.target.value)}
          />
          {error ? (
            <p id={xmlNoteId} className="ui-input-error-text" role="alert">
              {error}
            </p>
          ) : (
            <p id={xmlNoteId} className="ui-input-help-text">
              {METADATA_XML_HINT}
            </p>
          )}
        </div>
      )}
      {refreshable && (
        <Switch
          label={REFRESH_METADATA_LABEL}
          description={REFRESH_METADATA_DESCRIPTION}
          checked={draft.refreshMetadata}
          disabled={pending}
          onChange={(checked) => onChange('refreshMetadata', checked)}
        />
      )}
    </>
  );
}
