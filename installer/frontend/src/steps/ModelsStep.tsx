import type React from 'react';
import { Controller, useFormContext } from 'react-hook-form';
import {
  AlertDescription,
  AlertTitle,
  Checkbox,
  InfoBox,
  Input,
  SectionHeader,
  Select,
} from '../components';
import type { AiProvider, InstallerConfig } from '../types';

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

export function ModelsStep() {
  const {
    register,
    setValue,
    watch,
    control,
    formState: { errors },
  } = useFormContext<InstallerConfig>();
  const provider = (watch('AI_PROVIDER') ?? 'self_hosted') as AiProvider;
  const providerModels = modelOptions[provider];
  const useIamRole = watch('AI_BEDROCK_USE_IAM_ROLE') === 'true';
  const hasEmbeddingModels = providerModels.embedding.length > 0;

  return (
    <div className="space-y-6">
      <Controller
        control={control}
        name="AI_PROVIDER"
        render={({ field }) => (
          <Select
            label="AI Provider"
            options={providerOptions}
            helpText="Select your AI provider. Self-hosted uses local Ollama models."
            value={field.value}
            onValueChange={field.onChange}
            name={field.name}
          />
        )}
      />

      {provider === 'self_hosted' && (
        <div className="space-y-4">
          <SectionHeader title="LiteLLM Configuration" />
          <Input
            label="LiteLLM Host"
            type="text"
            placeholder="http://ollama:11434"
            helpText="URL of your Ollama/LiteLLM server"
            error={errors.AI_LITELLM_HOST?.message}
            {...register('AI_LITELLM_HOST')}
          />
          <Input
            label="LiteLLM API Key (Optional)"
            type="password"
            helpText="API key for LiteLLM if authentication is enabled"
            error={errors.AI_LITELLM_KEY?.message}
            {...register('AI_LITELLM_KEY')}
          />
        </div>
      )}

      {provider === 'openai' && (
        <div className="space-y-4">
          <SectionHeader title="OpenAI Configuration" />
          <Input
            label="OpenAI API Key"
            type="password"
            placeholder="sk-..."
            helpText="Your OpenAI API key from platform.openai.com"
            error={errors.AI_OPENAI_API_KEY?.message}
            {...register('AI_OPENAI_API_KEY')}
          />
          <Input
            label="Base URL (Optional)"
            type="text"
            placeholder="https://api.openai.com/v1"
            helpText="Custom base URL for OpenAI-compatible APIs"
            error={errors.AI_OPENAI_BASE_URL?.message}
            {...register('AI_OPENAI_BASE_URL')}
          />
        </div>
      )}

      {provider === 'anthropic' && (
        <div className="space-y-4">
          <SectionHeader title="Anthropic Configuration" />
          <Input
            label="Anthropic API Key"
            type="password"
            placeholder="sk-ant-..."
            helpText="Your Anthropic API key from console.anthropic.com"
            error={errors.AI_ANTHROPIC_API_KEY?.message}
            {...register('AI_ANTHROPIC_API_KEY')}
          />
          <Input
            label="Base URL (Optional)"
            type="text"
            placeholder="https://api.anthropic.com"
            helpText="Custom base URL for Anthropic-compatible APIs"
            error={errors.AI_ANTHROPIC_BASE_URL?.message}
            {...register('AI_ANTHROPIC_BASE_URL')}
          />
          <InfoBox variant="warning">
            <AlertTitle>Embeddings</AlertTitle>
            <AlertDescription>
              Anthropic does not provide embedding models. Use a different provider for embeddings
              (e.g., OpenAI or a self-hosted model).
            </AlertDescription>
          </InfoBox>
        </div>
      )}

      {provider === 'bedrock' && (
        <div className="space-y-4">
          <SectionHeader title="AWS Bedrock Configuration" />
          <Controller
            control={control}
            name="AI_BEDROCK_REGION"
            render={({ field }) => (
              <Select
                label="AWS Region"
                options={awsRegions}
                helpText="Select the AWS region for Bedrock"
                value={field.value}
                onValueChange={field.onChange}
                name={field.name}
              />
            )}
          />
          <Checkbox
            label="Use IAM Role"
            checked={useIamRole}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              setValue('AI_BEDROCK_USE_IAM_ROLE', e.target.checked ? 'true' : 'false', {
                shouldDirty: true,
                shouldValidate: true,
              })
            }
            helpText="Use IAM role from instance metadata (EC2/ECS). Uncheck to provide explicit credentials."
          />
          {!useIamRole && (
            <>
              <Input
                label="AWS Access Key ID"
                type="text"
                placeholder="AKIA..."
                helpText="Your AWS access key ID"
                error={errors.AI_BEDROCK_ACCESS_KEY?.message}
                {...register('AI_BEDROCK_ACCESS_KEY')}
              />
              <Input
                label="AWS Secret Access Key"
                type="password"
                helpText="Your AWS secret access key"
                error={errors.AI_BEDROCK_SECRET_KEY?.message}
                {...register('AI_BEDROCK_SECRET_KEY')}
              />
            </>
          )}
        </div>
      )}

      <div className="space-y-4">
        <SectionHeader title="Model Selection" />
        <Controller
          control={control}
          name="AI_MODEL_FAST"
          render={({ field }) => (
            <Select
              label="Fast Model"
              options={providerModels.fast}
              helpText="For general queries and quick responses"
              value={field.value}
              onValueChange={field.onChange}
              name={field.name}
            />
          )}
        />
        <Controller
          control={control}
          name="AI_MODEL_REASONING"
          render={({ field }) => (
            <Select
              label="Reasoning Model"
              options={providerModels.reasoning}
              helpText="For complex analysis and detailed reasoning"
              value={field.value}
              onValueChange={field.onChange}
              name={field.name}
            />
          )}
        />
        {hasEmbeddingModels ? (
          <Controller
            control={control}
            name="AI_MODEL_EMBEDDING"
            render={({ field }) => (
              <Select
                label="Embedding Model"
                options={providerModels.embedding}
                helpText="For semantic routing and search"
                value={field.value}
                onValueChange={field.onChange}
                name={field.name}
              />
            )}
          />
        ) : (
          <Input
            label="Embedding Model (External)"
            type="text"
            placeholder="text-embedding-3-small"
            helpText="Enter an embedding model from another provider (e.g., OpenAI)"
            error={errors.AI_MODEL_EMBEDDING?.message}
            {...register('AI_MODEL_EMBEDDING')}
          />
        )}
      </div>

      {provider === 'self_hosted' && (
        <InfoBox variant="info">
          <AlertDescription>
            Models will download on first start. Total size varies (typically 10-50GB).
          </AlertDescription>
        </InfoBox>
      )}

      {(provider === 'openai' || provider === 'anthropic') && (
        <InfoBox variant="info">
          <AlertDescription>
            API usage will be billed according to your provider's pricing. Monitor usage in your
            provider's dashboard.
          </AlertDescription>
        </InfoBox>
      )}

      {provider === 'bedrock' && (
        <InfoBox variant="info">
          <AlertDescription>
            AWS Bedrock usage is billed through your AWS account. Ensure you have model access
            enabled in the AWS console.
          </AlertDescription>
        </InfoBox>
      )}
    </div>
  );
}
