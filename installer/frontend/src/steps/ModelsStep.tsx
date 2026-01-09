import type React from 'react';
import { Checkbox, InfoBox, Input, Select } from '../components';
import type { AiProvider, InstallerConfig } from '../types';

interface ModelsStepProps {
  config: InstallerConfig;
  onChange: (key: keyof InstallerConfig, value: string) => void;
  getFieldError: (field: string) => string | undefined;
}

const providerOptions: { value: AiProvider; label: string }[] = [
  { value: 'self_hosted', label: 'Self-Hosted (Ollama via LiteLLM)' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'bedrock', label: 'AWS Bedrock' },
];

// Model options by provider
const modelOptions = {
  self_hosted: {
    fast: [
      { value: 'llama3.2:3b', label: 'llama3.2:3b - 3GB (Very Fast)' },
      { value: 'llama3.1:8b', label: 'llama3.1:8b - 4.7GB (Recommended)' },
      { value: 'qwen2.5:7b', label: 'qwen2.5:7b - 4.4GB' },
      { value: 'mistral:7b', label: 'mistral:7b - 4.1GB' },
    ],
    reasoning: [
      { value: 'deepseek-r1:7b', label: 'deepseek-r1:7b - 4.9GB' },
      { value: 'deepseek-r1:14b', label: 'deepseek-r1:14b - 8.9GB' },
      { value: 'deepseek-r1:32b', label: 'deepseek-r1:32b - 20GB (Best)' },
      { value: 'llama3.1:70b', label: 'llama3.1:70b - 40GB' },
    ],
    embedding: [
      { value: 'nomic-embed-text', label: 'nomic-embed-text - 274MB' },
      { value: 'mxbai-embed-large', label: 'mxbai-embed-large - 669MB' },
    ],
  },
  openai: {
    fast: [
      { value: 'gpt-4o-mini', label: 'GPT-4o Mini (Recommended)' },
      { value: 'gpt-4o', label: 'GPT-4o' },
      { value: 'gpt-4-turbo', label: 'GPT-4 Turbo' },
    ],
    reasoning: [
      { value: 'gpt-4o', label: 'GPT-4o (Recommended)' },
      { value: 'o1', label: 'o1' },
      { value: 'o1-mini', label: 'o1-mini' },
    ],
    embedding: [
      { value: 'text-embedding-3-small', label: 'text-embedding-3-small (Recommended)' },
      { value: 'text-embedding-3-large', label: 'text-embedding-3-large' },
      { value: 'text-embedding-ada-002', label: 'text-embedding-ada-002' },
    ],
  },
  anthropic: {
    fast: [
      { value: 'claude-3-haiku-20240307', label: 'Claude 3 Haiku (Recommended)' },
      { value: 'claude-sonnet-4-20250514', label: 'Claude Sonnet 4' },
    ],
    reasoning: [
      { value: 'claude-sonnet-4-20250514', label: 'Claude Sonnet 4 (Recommended)' },
      { value: 'claude-opus-4-20250514', label: 'Claude Opus 4' },
    ],
    embedding: [] as { value: string; label: string }[],
  },
  bedrock: {
    fast: [
      { value: 'anthropic.claude-3-haiku-20240307-v1:0', label: 'Claude 3 Haiku (Recommended)' },
      { value: 'amazon.nova-lite-v1:0', label: 'Amazon Nova Lite' },
      { value: 'amazon.nova-micro-v1:0', label: 'Amazon Nova Micro' },
    ],
    reasoning: [
      {
        value: 'anthropic.claude-3-5-sonnet-20241022-v2:0',
        label: 'Claude 3.5 Sonnet (Recommended)',
      },
      { value: 'amazon.nova-pro-v1:0', label: 'Amazon Nova Pro' },
      { value: 'anthropic.claude-3-opus-20240229-v1:0', label: 'Claude 3 Opus' },
    ],
    embedding: [
      { value: 'amazon.titan-embed-text-v2:0', label: 'Titan Embeddings V2 (Recommended)' },
      { value: 'amazon.titan-embed-text-v1', label: 'Titan Embeddings V1' },
      { value: 'cohere.embed-english-v3', label: 'Cohere Embed English V3' },
    ],
  },
};

const awsRegions = [
  { value: 'us-east-1', label: 'US East (N. Virginia)' },
  { value: 'us-west-2', label: 'US West (Oregon)' },
  { value: 'eu-west-1', label: 'Europe (Ireland)' },
  { value: 'eu-central-1', label: 'Europe (Frankfurt)' },
  { value: 'ap-northeast-1', label: 'Asia Pacific (Tokyo)' },
  { value: 'ap-southeast-1', label: 'Asia Pacific (Singapore)' },
  { value: 'ap-southeast-2', label: 'Asia Pacific (Sydney)' },
];

export function ModelsStep({ config, onChange, getFieldError }: ModelsStepProps) {
  const provider = config.AI_PROVIDER as AiProvider;
  const providerModels = modelOptions[provider];
  const useIamRole = config.AI_BEDROCK_USE_IAM_ROLE === 'true';
  const hasEmbeddingModels = providerModels.embedding.length > 0;

  return (
    <div className="step-content">
      <div className="step-header">
        <h2>AI Provider Configuration</h2>
        <p>Choose your AI provider and configure models</p>
      </div>

      <Select
        label="AI Provider"
        options={providerOptions}
        value={config.AI_PROVIDER}
        onChange={(e: React.ChangeEvent<HTMLSelectElement>) =>
          onChange('AI_PROVIDER', e.target.value)
        }
        helpText="Select your AI provider. Self-hosted uses local Ollama models."
      />

      {/* Self-hosted (Ollama via LiteLLM) settings */}
      {provider === 'self_hosted' && (
        <>
          <h3 className="section-header">LiteLLM Configuration</h3>
          <Input
            label="LiteLLM Host"
            type="text"
            value={config.AI_LITELLM_HOST}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              onChange('AI_LITELLM_HOST', e.target.value)
            }
            placeholder="http://ollama:11434"
            helpText="URL of your Ollama/LiteLLM server"
            error={getFieldError('AI_LITELLM_HOST')}
          />
          <Input
            label="LiteLLM API Key (Optional)"
            type="password"
            value={config.AI_LITELLM_KEY}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              onChange('AI_LITELLM_KEY', e.target.value)
            }
            helpText="API key for LiteLLM if authentication is enabled"
            error={getFieldError('AI_LITELLM_KEY')}
          />
        </>
      )}

      {/* OpenAI settings */}
      {provider === 'openai' && (
        <>
          <h3 className="section-header">OpenAI Configuration</h3>
          <Input
            label="OpenAI API Key"
            type="password"
            value={config.AI_OPENAI_API_KEY}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              onChange('AI_OPENAI_API_KEY', e.target.value)
            }
            placeholder="sk-..."
            helpText="Your OpenAI API key from platform.openai.com"
            error={getFieldError('AI_OPENAI_API_KEY')}
          />
          <Input
            label="Base URL (Optional)"
            type="text"
            value={config.AI_OPENAI_BASE_URL}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              onChange('AI_OPENAI_BASE_URL', e.target.value)
            }
            placeholder="https://api.openai.com/v1"
            helpText="Custom base URL for OpenAI-compatible APIs"
            error={getFieldError('AI_OPENAI_BASE_URL')}
          />
        </>
      )}

      {/* Anthropic settings */}
      {provider === 'anthropic' && (
        <>
          <h3 className="section-header">Anthropic Configuration</h3>
          <Input
            label="Anthropic API Key"
            type="password"
            value={config.AI_ANTHROPIC_API_KEY}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              onChange('AI_ANTHROPIC_API_KEY', e.target.value)
            }
            placeholder="sk-ant-..."
            helpText="Your Anthropic API key from console.anthropic.com"
            error={getFieldError('AI_ANTHROPIC_API_KEY')}
          />
          <Input
            label="Base URL (Optional)"
            type="text"
            value={config.AI_ANTHROPIC_BASE_URL}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              onChange('AI_ANTHROPIC_BASE_URL', e.target.value)
            }
            placeholder="https://api.anthropic.com"
            helpText="Custom base URL for Anthropic-compatible APIs"
            error={getFieldError('AI_ANTHROPIC_BASE_URL')}
          />
          <InfoBox variant="warning">
            Anthropic does not provide embedding models. You will need to use a different provider
            for embeddings (e.g., OpenAI or a self-hosted model).
          </InfoBox>
        </>
      )}

      {/* AWS Bedrock settings */}
      {provider === 'bedrock' && (
        <>
          <h3 className="section-header">AWS Bedrock Configuration</h3>
          <Select
            label="AWS Region"
            options={awsRegions}
            value={config.AI_BEDROCK_REGION}
            onChange={(e: React.ChangeEvent<HTMLSelectElement>) =>
              onChange('AI_BEDROCK_REGION', e.target.value)
            }
            helpText="Select the AWS region for Bedrock"
          />
          <Checkbox
            label="Use IAM Role"
            checked={useIamRole}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              onChange('AI_BEDROCK_USE_IAM_ROLE', e.target.checked ? 'true' : 'false')
            }
            helpText="Use IAM role from instance metadata (EC2/ECS). Uncheck to provide explicit credentials."
          />
          {!useIamRole && (
            <>
              <Input
                label="AWS Access Key ID"
                type="text"
                value={config.AI_BEDROCK_ACCESS_KEY}
                onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
                  onChange('AI_BEDROCK_ACCESS_KEY', e.target.value)
                }
                placeholder="AKIA..."
                helpText="Your AWS access key ID"
                error={getFieldError('AI_BEDROCK_ACCESS_KEY')}
              />
              <Input
                label="AWS Secret Access Key"
                type="password"
                value={config.AI_BEDROCK_SECRET_KEY}
                onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
                  onChange('AI_BEDROCK_SECRET_KEY', e.target.value)
                }
                helpText="Your AWS secret access key"
                error={getFieldError('AI_BEDROCK_SECRET_KEY')}
              />
            </>
          )}
        </>
      )}

      <h3 className="section-header">Model Selection</h3>

      <Select
        label="Fast Model"
        options={providerModels.fast}
        value={config.AI_MODEL_FAST}
        onChange={(e: React.ChangeEvent<HTMLSelectElement>) =>
          onChange('AI_MODEL_FAST', e.target.value)
        }
        helpText="For general queries and quick responses"
      />

      <Select
        label="Reasoning Model"
        options={providerModels.reasoning}
        value={config.AI_MODEL_REASONING}
        onChange={(e: React.ChangeEvent<HTMLSelectElement>) =>
          onChange('AI_MODEL_REASONING', e.target.value)
        }
        helpText="For complex analysis and detailed reasoning"
      />

      {hasEmbeddingModels ? (
        <Select
          label="Embedding Model"
          options={providerModels.embedding}
          value={config.AI_MODEL_EMBEDDING}
          onChange={(e: React.ChangeEvent<HTMLSelectElement>) =>
            onChange('AI_MODEL_EMBEDDING', e.target.value)
          }
          helpText="For semantic routing and search"
        />
      ) : (
        <Input
          label="Embedding Model (External)"
          type="text"
          value={config.AI_MODEL_EMBEDDING}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
            onChange('AI_MODEL_EMBEDDING', e.target.value)
          }
          placeholder="text-embedding-3-small"
          helpText="Enter an embedding model from another provider (e.g., OpenAI)"
          error={getFieldError('AI_MODEL_EMBEDDING')}
        />
      )}

      {provider === 'self_hosted' && (
        <InfoBox variant="info">
          Models will download on first start. Total size varies (typically 10-50GB).
        </InfoBox>
      )}

      {(provider === 'openai' || provider === 'anthropic') && (
        <InfoBox variant="info">
          API usage will be billed according to your provider's pricing. Monitor usage in your
          provider's dashboard.
        </InfoBox>
      )}

      {provider === 'bedrock' && (
        <InfoBox variant="info">
          AWS Bedrock usage is billed through your AWS account. Ensure you have model access enabled
          in the AWS console.
        </InfoBox>
      )}
    </div>
  );
}
