import type { ReactNode } from 'react';
import type { AiProvider } from '../workspace/types';
import {
  awsRegions,
  type ProviderConfigured,
  type ProviderCredentials,
  providerOptions,
} from './options';

interface AiProviderFieldsProps {
  provider: AiProvider;
  onProviderChange: (provider: AiProvider) => void;
  credentials: ProviderCredentials;
  configured: ProviderConfigured;
  onChange: <K extends keyof ProviderCredentials>(key: K, value: ProviderCredentials[K]) => void;
}

const MASK = '••••••••';

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

export function AiProviderFields({
  provider,
  onProviderChange,
  credentials,
  configured,
  onChange,
}: AiProviderFieldsProps) {
  return (
    <div className="form-grid">
      <Field id="ai-provider" label="AI Provider" full>
        <select
          id="ai-provider"
          value={provider}
          onChange={(event) => onProviderChange(event.target.value as AiProvider)}
          className="form-select"
        >
          {providerOptions.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </Field>

      {provider === 'self_hosted' && (
        <>
          <Field id="litellm-host" label="LiteLLM Host">
            <input
              type="text"
              id="litellm-host"
              value={credentials.litellmHost}
              onChange={(event) => onChange('litellmHost', event.target.value)}
              placeholder="http://localhost:11434"
              className="form-input"
            />
          </Field>
          <Field id="litellm-key" label="LiteLLM API Key" configured={configured.litellm} optional>
            <input
              type="password"
              id="litellm-key"
              value={credentials.litellmKey}
              onChange={(event) => onChange('litellmKey', event.target.value)}
              placeholder={configured.litellm ? MASK : 'Enter API key'}
              className="form-input"
            />
          </Field>
        </>
      )}

      {provider === 'openai' && (
        <>
          <Field id="openai-key" label="OpenAI API Key" configured={configured.openai}>
            <input
              type="password"
              id="openai-key"
              value={credentials.openaiApiKey}
              onChange={(event) => onChange('openaiApiKey', event.target.value)}
              placeholder={configured.openai ? MASK : 'sk-...'}
              className="form-input"
            />
          </Field>
          <Field id="openai-base-url" label="Base URL" optional>
            <input
              type="text"
              id="openai-base-url"
              value={credentials.openaiBaseUrl}
              onChange={(event) => onChange('openaiBaseUrl', event.target.value)}
              placeholder="https://api.openai.com/v1"
              className="form-input"
            />
          </Field>
        </>
      )}

      {provider === 'anthropic' && (
        <>
          <Field id="anthropic-key" label="Anthropic API Key" configured={configured.anthropic}>
            <input
              type="password"
              id="anthropic-key"
              value={credentials.anthropicApiKey}
              onChange={(event) => onChange('anthropicApiKey', event.target.value)}
              placeholder={configured.anthropic ? MASK : 'sk-ant-...'}
              className="form-input"
            />
          </Field>
          <Field id="anthropic-base-url" label="Base URL" optional>
            <input
              type="text"
              id="anthropic-base-url"
              value={credentials.anthropicBaseUrl}
              onChange={(event) => onChange('anthropicBaseUrl', event.target.value)}
              placeholder="https://api.anthropic.com"
              className="form-input"
            />
          </Field>
          <div className="alert alert-warning">
            Anthropic does not provide embedding models. Use a different provider for embeddings.
          </div>
        </>
      )}

      {provider === 'bedrock' && (
        <>
          <Field id="bedrock-region" label="AWS Region">
            <select
              id="bedrock-region"
              value={credentials.bedrockRegion}
              onChange={(event) => onChange('bedrockRegion', event.target.value)}
              className="form-select"
            >
              {awsRegions.map((region) => (
                <option key={region} value={region}>
                  {region}
                </option>
              ))}
            </select>
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
        </>
      )}
    </div>
  );
}
