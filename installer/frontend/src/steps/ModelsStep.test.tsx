import { fireEvent, render, screen } from '@testing-library/react';
import type { InstallerConfig } from '../types';
import { ModelsStep } from './ModelsStep';

const createMockConfig = (overrides?: Partial<InstallerConfig>): InstallerConfig => ({
  DOMAIN_HOST_WEBUI: 'test.localhost',
  SECURITY_BASICAUTH_REALM: '',
  SECURITY_LITELLM_MASTER_KEY: '',
  SECURITY_LITELLM_SALT_KEY: '',
  SECURITY_SEARXNG_SECRET_KEY: '',
  SECURITY_MANAGER_API_KEY: '',
  POSTGRES_PASSWORD: '',
  SECURITY_HTTP_REDIRECT: 'true',
  SECURITY_GENERATE_CERTIFICATE: 'true',
  WEBUI_AUTH: 'false',
  WEBUI_ENABLE_SIGNUP: 'false',
  WEBUI_DEFAULT_LOCALE: 'en-US',
  SEARCH_ENABLE_WEB_SEARCH: 'true',
  SEARCH_RESULT_COUNT: '5',
  SEARCH_CONCURRENT_REQUESTS: '8',
  SEARCH_SEARXNG_INSTANCE_NAME: '',
  VPN_SERVICE_PROVIDER: '',
  VPN_TYPE: 'openvpn',
  VPN_OPENVPN_USER: '',
  VPN_OPENVPN_PASSWORD: '',
  VPN_WIREGUARD_PRIVATE_KEY: '',
  VPN_WIREGUARD_ADDRESSES: '',
  VPN_SERVER_COUNTRIES: '',
  VPN_SERVER_CITIES: '',
  VPN_SERVER_REGIONS: '',
  MONITORING_ENABLED: 'false',
  MONITORING_GRAFANA_ADMIN_USER: 'admin',
  MONITORING_GRAFANA_ADMIN_PASSWORD: '',
  MONITORING_RETENTION_TIME: '15d',
  ALERT_ENABLED: 'false',
  ALERT_EMAIL_RECIPIENTS: '',
  ALERT_SMTP_HOST: '',
  ALERT_SMTP_PORT: '587',
  ALERT_SMTP_USER: '',
  ALERT_SMTP_PASSWORD: '',
  ALERT_SMTP_FROM_ADDRESS: '',
  ALERT_SMTP_FROM_NAME: '',
  ADVANCED_LITELLM_WORKERS: '4',
  ADVANCED_LITELLM_REQUEST_TIMEOUT: '600',
  ADVANCED_TZ: 'UTC',
  ADVANCED_ACME_EMAIL: '',
  SECURITY_BASIC_AUTH_USERS_FILE: '',
  OLLAMA_HOST: '',
  OLLAMA_KEEP_ALIVE: '',
  OLLAMA_MAX_LOADED_MODELS: '',
  // AI Provider fields
  AI_PROVIDER: 'self_hosted',
  AI_LITELLM_HOST: '',
  AI_LITELLM_KEY: '',
  AI_OPENAI_API_KEY: '',
  AI_OPENAI_BASE_URL: '',
  AI_ANTHROPIC_API_KEY: '',
  AI_ANTHROPIC_BASE_URL: '',
  AI_BEDROCK_REGION: 'us-east-1',
  AI_BEDROCK_ACCESS_KEY: '',
  AI_BEDROCK_SECRET_KEY: '',
  AI_BEDROCK_USE_IAM_ROLE: 'false',
  AI_MODEL_FAST: 'llama3.1:8b',
  AI_MODEL_REASONING: 'deepseek-r1:7b',
  AI_MODEL_EMBEDDING: 'nomic-embed-text',
  ...overrides,
});

describe('ModelsStep', () => {
  const onChange = jest.fn();
  const getFieldError = jest.fn().mockReturnValue(undefined);

  beforeEach(() => {
    jest.clearAllMocks();
  });

  describe('Header and Provider Selection', () => {
    it('renders step header', () => {
      render(
        <ModelsStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      expect(screen.getByText('AI Provider Configuration')).toBeInTheDocument();
      expect(screen.getByText('Choose your AI provider and configure models')).toBeInTheDocument();
    });

    it('renders AI provider select with current value', () => {
      render(
        <ModelsStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      const select = screen.getByLabelText(/ai provider/i);
      expect(select).toHaveValue('self_hosted');
    });

    it('calls onChange when provider is changed', () => {
      render(
        <ModelsStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      fireEvent.change(screen.getByLabelText(/ai provider/i), {
        target: { value: 'openai' },
      });

      expect(onChange).toHaveBeenCalledWith('AI_PROVIDER', 'openai');
    });

    it('displays all provider options', () => {
      render(
        <ModelsStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      const select = screen.getByLabelText(/ai provider/i);
      const options = select.querySelectorAll('option');

      expect(options.length).toBe(4);
      expect(options[0].value).toBe('self_hosted');
      expect(options[1].value).toBe('openai');
      expect(options[2].value).toBe('anthropic');
      expect(options[3].value).toBe('bedrock');
    });
  });

  describe('Self-Hosted Provider', () => {
    it('renders LiteLLM configuration for self_hosted provider', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'self_hosted' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByText('LiteLLM Configuration')).toBeInTheDocument();
      expect(screen.getByLabelText(/litellm host/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/litellm api key/i)).toBeInTheDocument();
    });

    it('calls onChange when LiteLLM host is changed', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'self_hosted' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.change(screen.getByLabelText(/litellm host/i), {
        target: { value: 'http://localhost:4000' },
      });

      expect(onChange).toHaveBeenCalledWith('AI_LITELLM_HOST', 'http://localhost:4000');
    });

    it('displays info about model download', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'self_hosted' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByText(/models will download on first start/i)).toBeInTheDocument();
    });

    it('displays self-hosted model options', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'self_hosted' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      const fastSelect = screen.getByLabelText(/fast model/i);
      expect(fastSelect.querySelectorAll('option').length).toBe(4);

      const reasoningSelect = screen.getByLabelText(/reasoning model/i);
      expect(reasoningSelect.querySelectorAll('option').length).toBe(4);

      const embeddingSelect = screen.getByLabelText(/embedding model/i);
      expect(embeddingSelect.querySelectorAll('option').length).toBe(2);
    });
  });

  describe('OpenAI Provider', () => {
    it('renders OpenAI configuration when openai provider selected', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'openai' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByText('OpenAI Configuration')).toBeInTheDocument();
      expect(screen.getByLabelText(/openai api key/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/base url/i)).toBeInTheDocument();
    });

    it('calls onChange when OpenAI API key is changed', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'openai' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.change(screen.getByLabelText(/openai api key/i), {
        target: { value: 'sk-test123' },
      });

      expect(onChange).toHaveBeenCalledWith('AI_OPENAI_API_KEY', 'sk-test123');
    });

    it('displays billing info for OpenAI', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'openai' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByText(/api usage will be billed/i)).toBeInTheDocument();
    });

    it('displays OpenAI model options', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'openai', AI_MODEL_FAST: 'gpt-4o-mini' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      const fastSelect = screen.getByLabelText(/fast model/i);
      expect(fastSelect.querySelectorAll('option').length).toBe(3);
    });
  });

  describe('Anthropic Provider', () => {
    it('renders Anthropic configuration when anthropic provider selected', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'anthropic' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByText('Anthropic Configuration')).toBeInTheDocument();
      expect(screen.getByLabelText(/anthropic api key/i)).toBeInTheDocument();
    });

    it('shows warning about no embedding models', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'anthropic' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByText(/does not provide embedding models/i)).toBeInTheDocument();
    });

    it('shows text input for embedding model instead of select', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'anthropic' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      const embeddingInput = screen.getByLabelText(/embedding model \(external\)/i);
      expect(embeddingInput.tagName).toBe('INPUT');
    });
  });

  describe('AWS Bedrock Provider', () => {
    it('renders Bedrock configuration when bedrock provider selected', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'bedrock' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByText('AWS Bedrock Configuration')).toBeInTheDocument();
      expect(screen.getByLabelText(/aws region/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/use iam role/i)).toBeInTheDocument();
    });

    it('shows credential fields when IAM role is not used', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'bedrock', AI_BEDROCK_USE_IAM_ROLE: 'false' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByLabelText(/aws access key id/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/aws secret access key/i)).toBeInTheDocument();
    });

    it('hides credential fields when IAM role is used', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'bedrock', AI_BEDROCK_USE_IAM_ROLE: 'true' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.queryByLabelText(/aws access key id/i)).not.toBeInTheDocument();
      expect(screen.queryByLabelText(/aws secret access key/i)).not.toBeInTheDocument();
    });

    it('calls onChange when IAM role checkbox is toggled', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'bedrock', AI_BEDROCK_USE_IAM_ROLE: 'false' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.click(screen.getByLabelText(/use iam role/i));

      expect(onChange).toHaveBeenCalledWith('AI_BEDROCK_USE_IAM_ROLE', 'true');
    });

    it('displays Bedrock info message', () => {
      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'bedrock' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByText(/aws bedrock usage is billed/i)).toBeInTheDocument();
    });
  });

  describe('Model Selection', () => {
    it('renders model selection section', () => {
      render(
        <ModelsStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      expect(screen.getByText('Model Selection')).toBeInTheDocument();
    });

    it('renders fast model select with current value', () => {
      render(
        <ModelsStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      const select = screen.getByLabelText(/fast model/i);
      expect(select).toHaveValue('llama3.1:8b');
    });

    it('calls onChange when fast model is changed', () => {
      render(
        <ModelsStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      fireEvent.change(screen.getByLabelText(/fast model/i), {
        target: { value: 'llama3.2:3b' },
      });

      expect(onChange).toHaveBeenCalledWith('AI_MODEL_FAST', 'llama3.2:3b');
    });

    it('renders reasoning model select with current value', () => {
      render(
        <ModelsStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      const select = screen.getByLabelText(/reasoning model/i);
      expect(select).toHaveValue('deepseek-r1:7b');
    });

    it('calls onChange when reasoning model is changed', () => {
      render(
        <ModelsStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      fireEvent.change(screen.getByLabelText(/reasoning model/i), {
        target: { value: 'deepseek-r1:32b' },
      });

      expect(onChange).toHaveBeenCalledWith('AI_MODEL_REASONING', 'deepseek-r1:32b');
    });

    it('renders embedding model select with current value', () => {
      render(
        <ModelsStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      const select = screen.getByLabelText(/embedding model/i);
      expect(select).toHaveValue('nomic-embed-text');
    });

    it('calls onChange when embedding model is changed', () => {
      render(
        <ModelsStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      fireEvent.change(screen.getByLabelText(/embedding model/i), {
        target: { value: 'mxbai-embed-large' },
      });

      expect(onChange).toHaveBeenCalledWith('AI_MODEL_EMBEDDING', 'mxbai-embed-large');
    });
  });

  describe('Error Display', () => {
    it('displays field errors', () => {
      const getFieldErrorWithError = jest.fn((field: string) =>
        field === 'AI_OPENAI_API_KEY' ? 'API key is required' : undefined
      );

      render(
        <ModelsStep
          config={createMockConfig({ AI_PROVIDER: 'openai' })}
          onChange={onChange}
          getFieldError={getFieldErrorWithError}
        />
      );

      expect(screen.getByText('API key is required')).toBeInTheDocument();
    });
  });
});
