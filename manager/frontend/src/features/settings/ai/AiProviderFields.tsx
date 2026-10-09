import { Select } from '@zone/ui';
import type { ReactNode } from 'react';
import type { AiProvider, AiSettings, OrganizationKeys } from '../workspace/types';
import {
  awsRegions,
  missesOrganizationKey,
  needsKeyAgain,
  type ProviderConfigured,
  type ProviderCredentials,
  providerOptions,
  routingOf,
} from './options';
import type { SettingsLevel } from './types';

interface AiProviderFieldsProps {
  level: SettingsLevel;
  provider: AiProvider;
  onProviderChange: (provider: AiProvider) => void;
  credentials: ProviderCredentials;
  configured: ProviderConfigured;
  onChange: <K extends keyof ProviderCredentials>(key: K, value: ProviderCredentials[K]) => void;
  saved?: AiSettings | null;
  organizationKeys?: OrganizationKeys;
}

const MASK = '••••••••';
const ENDPOINT_HINT_ID = 'ai-endpoint-hint';
const REKEY_HINT_ID = 'ai-rekey-hint';

function Field({
  id,
  label,
  configured,
  optional,
  children,
  full,
}: {
  id: string;
  label: string;
  configured?: boolean;
  optional?: boolean;
  full?: boolean;
  children: ReactNode;
}) {
  return (
    <div className={full ? 'form-group form-group--full' : 'form-group'}>
      <label htmlFor={id}>
        {label}
        {optional && <span className="label-optional">optional</span>}
        {configured && <span className="credential-set">(configured)</span>}
      </label>
      {children}
    </div>
  );
}

export const UNROUTED_NOTICE =
  'Saved before completions were routed; save to start sending completions here.';
export const VERSION_HINT =
  "A URL naming only a host gets /v1 added. End it with / to use the host's root, or give a path to use it as saved.";
export const AUTOMATIC_HINT =
  'Automatic needs a Fast or Reasoning model when completions go to a saved endpoint.';
export const REKEY_HINT = 'Changing the URL needs the key again';
export const KEYLESS_WORKSPACE_WARNING =
  "This workspace host has no key of its own, and the organization's key never goes to it. Enter a key if the host needs one.";

interface EndpointKeyProps {
  id: string;
  label: string;
  value: string;
  placeholder: string;
  configured: boolean;
  optional?: boolean;
  rekey: boolean;
  onChange: (value: string) => void;
}

function EndpointKey({
  id,
  label,
  value,
  placeholder,
  configured,
  optional,
  rekey,
  onChange,
}: EndpointKeyProps) {
  return (
    <Field id={id} label={label} configured={configured} optional={optional && !rekey}>
      <input
        type="password"
        id={id}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={configured ? MASK : placeholder}
        required={rekey}
        aria-describedby={rekey ? REKEY_HINT_ID : undefined}
        className="form-input"
      />
      {rekey && (
        <p id={REKEY_HINT_ID} className="form-hint">
          {REKEY_HINT}
        </p>
      )}
    </Field>
  );
}

interface EndpointHintsProps {
  level: SettingsLevel;
  provider: AiProvider;
  credentials: ProviderCredentials;
  configured: ProviderConfigured;
  saved: AiSettings | null;
  organizationKeys: OrganizationKeys | undefined;
}

function EndpointHints({
  level,
  provider,
  credentials,
  configured,
  saved,
  organizationKeys,
}: EndpointHintsProps) {
  const routing = routingOf(saved, provider);
  const keyless =
    level === 'workspace' &&
    organizationKeys !== undefined &&
    missesOrganizationKey(provider, credentials, configured, organizationKeys);
  return (
    <div id={ENDPOINT_HINT_ID} className="form-group form-group--full">
      {routing === 'pending' && (
        <div className="alert alert-warning" role="status">
          {UNROUTED_NOTICE}
        </div>
      )}
      {routing === 'routed' && (
        <p className="form-hint">
          Chats, task runs and background work in this {level} send completions here.
        </p>
      )}
      <p className="form-hint">{VERSION_HINT}</p>
      <p className="form-hint">{AUTOMATIC_HINT}</p>
      {provider === 'anthropic' && (
        <p className="form-hint">Completions go through Anthropic's OpenAI-compatible endpoint.</p>
      )}
      {level === 'workspace' && (
        <p className="form-hint">
          A workspace host needs its own key; it never receives the organization's.
        </p>
      )}
      {keyless && (
        <div className="alert alert-warning" role="status">
          {KEYLESS_WORKSPACE_WARNING}
        </div>
      )}
    </div>
  );
}

export function AiProviderFields({
  level,
  provider,
  onProviderChange,
  credentials,
  configured,
  onChange,
  saved = null,
  organizationKeys,
}: AiProviderFieldsProps) {
  const rekey = needsKeyAgain(provider, credentials, configured, saved);
  const hints = (
    <EndpointHints
      level={level}
      provider={provider}
      credentials={credentials}
      configured={configured}
      saved={saved}
      organizationKeys={organizationKeys}
    />
  );
  return (
    <div className="form-grid">
      <Field id="ai-provider" label="AI Provider" full>
        <Select
          compact
          id="ai-provider"
          value={provider}
          onValueChange={(next) => onProviderChange(next as AiProvider)}
          options={providerOptions}
        />
      </Field>

      {provider === 'self_hosted' && (
        <>
          <Field id="litellm-host" label="LiteLLM Host">
            <input
              type="text"
              id="litellm-host"
              value={credentials.litellmHost}
              onChange={(event) => onChange('litellmHost', event.target.value)}
              placeholder="http://litellm:4000"
              aria-describedby={ENDPOINT_HINT_ID}
              className="form-input"
            />
          </Field>
          <EndpointKey
            id="litellm-key"
            label="LiteLLM API Key"
            value={credentials.litellmKey}
            placeholder="Enter API key"
            configured={configured.litellm}
            optional
            rekey={rekey}
            onChange={(value) => onChange('litellmKey', value)}
          />
          {hints}
        </>
      )}

      {provider === 'openai' && (
        <>
          <EndpointKey
            id="openai-key"
            label="OpenAI API Key"
            value={credentials.openaiApiKey}
            placeholder="sk-..."
            configured={configured.openai}
            rekey={rekey}
            onChange={(value) => onChange('openaiApiKey', value)}
          />
          <Field id="openai-base-url" label="Base URL" optional>
            <input
              type="text"
              id="openai-base-url"
              value={credentials.openaiBaseUrl}
              onChange={(event) => onChange('openaiBaseUrl', event.target.value)}
              placeholder="https://api.openai.com/v1"
              aria-describedby={ENDPOINT_HINT_ID}
              className="form-input"
            />
          </Field>
          {hints}
        </>
      )}

      {provider === 'anthropic' && (
        <>
          <EndpointKey
            id="anthropic-key"
            label="Anthropic API Key"
            value={credentials.anthropicApiKey}
            placeholder="sk-ant-..."
            configured={configured.anthropic}
            rekey={rekey}
            onChange={(value) => onChange('anthropicApiKey', value)}
          />
          <Field id="anthropic-base-url" label="Base URL" optional>
            <input
              type="text"
              id="anthropic-base-url"
              value={credentials.anthropicBaseUrl}
              onChange={(event) => onChange('anthropicBaseUrl', event.target.value)}
              placeholder="https://api.anthropic.com/v1"
              aria-describedby={ENDPOINT_HINT_ID}
              className="form-input"
            />
          </Field>
          {hints}
          <div className="alert alert-warning">
            Anthropic does not provide embedding models. Use a different provider for embeddings.
          </div>
        </>
      )}

      {provider === 'bedrock' && (
        <>
          <Field id="bedrock-region" label="AWS Region">
            <Select
              compact
              id="bedrock-region"
              value={credentials.bedrockRegion}
              onValueChange={(next) => onChange('bedrockRegion', next)}
              options={awsRegions.map((region) => ({ value: region, label: region }))}
            />
          </Field>
          <div className="form-group">
            <span className="form-label">Authentication</span>
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={credentials.bedrockUseIamRole}
                onChange={(event) => onChange('bedrockUseIamRole', event.target.checked)}
              />
              Use IAM Role (EC2 instance profile / ECS task role)
            </label>
          </div>
          {!credentials.bedrockUseIamRole && (
            <>
              <Field id="bedrock-access-key" label="Access Key ID" configured={configured.bedrock}>
                <input
                  type="password"
                  id="bedrock-access-key"
                  value={credentials.bedrockAccessKey}
                  onChange={(event) => onChange('bedrockAccessKey', event.target.value)}
                  placeholder={configured.bedrock ? MASK : 'AKIA...'}
                  className="form-input"
                />
              </Field>
              <Field id="bedrock-secret-key" label="Secret Access Key">
                <input
                  type="password"
                  id="bedrock-secret-key"
                  value={credentials.bedrockSecretKey}
                  onChange={(event) => onChange('bedrockSecretKey', event.target.value)}
                  placeholder={configured.bedrock ? MASK : 'Secret key'}
                  className="form-input"
                />
              </Field>
            </>
          )}
          <div className="alert alert-warning">
            Bedrock completions still use the server's default endpoint for now.
          </div>
        </>
      )}

      <Field id="runpod-key" label="Runpod" configured={configured.runpod} optional>
        <input
          type="password"
          id="runpod-key"
          value={credentials.runpodApiKey}
          onChange={(event) => onChange('runpodApiKey', event.target.value)}
          placeholder={configured.runpod ? MASK : 'Enter API key'}
          className="form-input"
        />
      </Field>
    </div>
  );
}
