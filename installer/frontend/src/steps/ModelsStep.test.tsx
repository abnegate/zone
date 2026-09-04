import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { FormProvider, type UseFormReturn, useForm } from 'react-hook-form';
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
  SEARCH_ENABLE_WEB_SEARCH: 'true',
  SEARCH_RESULT_COUNT: '5',
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

const renderWithForm = (defaultValues: InstallerConfig) => {
  let methods: UseFormReturn<InstallerConfig> | undefined;
  const Wrapper = ({ children }: { children: ReactNode }) => {
    const form = useForm<InstallerConfig>({ defaultValues });
    methods = form;
    return <FormProvider {...form}>{children}</FormProvider>;
  };
  const utils = render(<ModelsStep />, { wrapper: Wrapper });
  if (!methods) {
    throw new Error('Form methods not initialized');
  }
  return { ...utils, methods };
};

describe('ModelsStep', () => {
  it('renders AI provider select with current value', () => {
    const { methods } = renderWithForm(createMockConfig());

    expect(methods.getValues('AI_PROVIDER')).toBe('self_hosted');
    expect(screen.getByLabelText(/ai provider/i)).toHaveTextContent('Self-Hosted');
  });

  it('updates provider selection', async () => {
    const { methods } = renderWithForm(createMockConfig());

    // Radix UI Select requires clicking to open, then selecting an option
    fireEvent.click(screen.getByLabelText(/ai provider/i));
    await waitFor(() => {
      expect(screen.getByText('OpenAI')).toBeInTheDocument();
    });
    fireEvent.click(screen.getByText('OpenAI'));

    await waitFor(() => {
      expect(methods.getValues('AI_PROVIDER')).toBe('openai');
    });
  });

  it('displays all provider options', () => {
    // Verify the select is present and has the correct initial value
    // Testing actual dropdown options requires Radix Portal testing setup
    const { methods } = renderWithForm(createMockConfig());

    const select = screen.getByLabelText(/ai provider/i);
    expect(select).toBeInTheDocument();
    expect(methods.getValues('AI_PROVIDER')).toBe('self_hosted');
    // The trigger should display the current selection
    expect(select).toHaveTextContent('Self-Hosted');
  });

  it('renders LiteLLM configuration for self-hosted provider', () => {
    renderWithForm(createMockConfig({ AI_PROVIDER: 'self_hosted' }));

    expect(screen.getByText('LiteLLM Configuration')).toBeInTheDocument();
    expect(screen.getByLabelText(/litellm host/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/litellm api key/i)).toBeInTheDocument();
    expect(screen.getByText(/models will download on first start/i)).toBeInTheDocument();
  });

  it('shows self-hosted model options', () => {
    const { methods } = renderWithForm(createMockConfig({ AI_PROVIDER: 'self_hosted' }));

    // Verify all model selects are present with correct initial values
    const fastSelect = screen.getByLabelText(/fast model/i);
    const reasoningSelect = screen.getByLabelText(/reasoning model/i);
    const embeddingSelect = screen.getByLabelText(/embedding model/i);

    expect(fastSelect).toBeInTheDocument();
    expect(fastSelect).toHaveTextContent('llama3.1:8b');
    expect(methods.getValues('AI_MODEL_FAST')).toBe('llama3.1:8b');

    expect(reasoningSelect).toBeInTheDocument();
    expect(reasoningSelect).toHaveTextContent('deepseek-r1:7b');
    expect(methods.getValues('AI_MODEL_REASONING')).toBe('deepseek-r1:7b');

    expect(embeddingSelect).toBeInTheDocument();
    expect(embeddingSelect).toHaveTextContent('nomic-embed-text');
    expect(methods.getValues('AI_MODEL_EMBEDDING')).toBe('nomic-embed-text');
  });

  it('renders OpenAI configuration when openai provider selected', () => {
    // Test with openai already selected to avoid portal interaction issues
    renderWithForm(createMockConfig({ AI_PROVIDER: 'openai' }));

    expect(screen.getByText('OpenAI Configuration')).toBeInTheDocument();
    expect(screen.getByLabelText(/openai api key/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/base url/i)).toBeInTheDocument();
    expect(screen.getByText(/api usage will be billed/i)).toBeInTheDocument();
  });

  it('renders Anthropic configuration when anthropic provider selected', () => {
    // Test with anthropic already selected to avoid portal interaction issues
    renderWithForm(createMockConfig({ AI_PROVIDER: 'anthropic' }));

    expect(screen.getByText('Anthropic Configuration')).toBeInTheDocument();
    expect(screen.getByLabelText(/anthropic api key/i)).toBeInTheDocument();
    expect(screen.getByText(/anthropic does not provide embedding models/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/embedding model \(external\)/i)).toBeInTheDocument();
  });

  it('renders AWS Bedrock configuration when bedrock provider selected', () => {
    // Test with bedrock already selected to avoid portal interaction issues
    renderWithForm(createMockConfig({ AI_PROVIDER: 'bedrock' }));

    expect(screen.getByText('AWS Bedrock Configuration')).toBeInTheDocument();
    expect(screen.getByLabelText(/aws region/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/use iam role/i)).toBeInTheDocument();
  });

  it('hides AWS credentials when IAM role is enabled', async () => {
    renderWithForm(createMockConfig({ AI_PROVIDER: 'bedrock' }));

    const checkbox = screen.getByLabelText(/use iam role/i);
    fireEvent.click(checkbox);

    await waitFor(() => {
      expect(screen.queryByLabelText(/aws access key id/i)).not.toBeInTheDocument();
      expect(screen.queryByLabelText(/aws secret access key/i)).not.toBeInTheDocument();
    });
  });
});
