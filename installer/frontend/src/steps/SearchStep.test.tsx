import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { FormProvider, type UseFormReturn, useForm } from 'react-hook-form';
import type { InstallerConfig } from '../types';
import { SearchStep } from './SearchStep';

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
  AI_PROVIDER: 'self_hosted',
  AI_LITELLM_HOST: 'http://ollama:11434',
  AI_LITELLM_KEY: '',
  AI_OPENAI_API_KEY: '',
  AI_OPENAI_BASE_URL: '',
  AI_ANTHROPIC_API_KEY: '',
  AI_ANTHROPIC_BASE_URL: '',
  AI_BEDROCK_REGION: 'us-east-1',
  AI_BEDROCK_ACCESS_KEY: '',
  AI_BEDROCK_SECRET_KEY: '',
  AI_BEDROCK_USE_IAM_ROLE: 'false',
  AI_MODEL_FAST: '',
  AI_MODEL_REASONING: '',
  AI_MODEL_EMBEDDING: '',
  WEBUI_AUTH: 'false',
  WEBUI_ENABLE_SIGNUP: 'false',
  WEBUI_DEFAULT_LOCALE: 'en-US',
  SEARCH_ENABLE_WEB_SEARCH: 'true',
  SEARCH_RESULT_COUNT: '5',
  SEARCH_CONCURRENT_REQUESTS: '8',
  SEARCH_SEARXNG_INSTANCE_NAME: 'my-searx',
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
  ...overrides,
});

const renderWithForm = (defaultValues: InstallerConfig) => {
  let methods: UseFormReturn<InstallerConfig> | undefined;
  const Wrapper = ({ children }: { children: ReactNode }) => {
    const form = useForm<InstallerConfig>({ defaultValues });
    methods = form;
    return <FormProvider {...form}>{children}</FormProvider>;
  };
  const utils = render(<SearchStep />, { wrapper: Wrapper });
  if (!methods) {
    throw new Error('Form methods not initialized');
  }
  return { ...utils, methods };
};

describe('SearchStep', () => {
  it('renders web search checkbox checked when enabled', () => {
    renderWithForm(createMockConfig());

    const checkbox = screen.getByLabelText(/enable web search/i);
    expect(checkbox).toBeChecked();
  });

  it('renders web search checkbox unchecked when disabled', () => {
    renderWithForm(createMockConfig({ SEARCH_ENABLE_WEB_SEARCH: 'false' }));

    const checkbox = screen.getByLabelText(/enable web search/i);
    expect(checkbox).not.toBeChecked();
  });

  it('toggles web search setting', () => {
    const { methods } = renderWithForm(createMockConfig());

    fireEvent.click(screen.getByLabelText(/enable web search/i));
    expect(methods.getValues('SEARCH_ENABLE_WEB_SEARCH')).toBe('false');

    fireEvent.click(screen.getByLabelText(/enable web search/i));
    expect(methods.getValues('SEARCH_ENABLE_WEB_SEARCH')).toBe('true');
  });

  it('renders results per query input with current value', () => {
    renderWithForm(createMockConfig());

    const input = screen.getByLabelText(/results per query/i);
    expect(input).toHaveValue(5);
  });

  it('updates results per query', () => {
    const { methods } = renderWithForm(createMockConfig());

    fireEvent.change(screen.getByLabelText(/results per query/i), {
      target: { value: '10' },
    });

    expect(methods.getValues('SEARCH_RESULT_COUNT')).toBe('10');
  });

  it('renders concurrent requests input with current value', () => {
    renderWithForm(createMockConfig());

    const input = screen.getByLabelText(/concurrent requests/i);
    expect(input).toHaveValue(8);
  });

  it('updates concurrent requests', () => {
    const { methods } = renderWithForm(createMockConfig());

    fireEvent.change(screen.getByLabelText(/concurrent requests/i), {
      target: { value: '16' },
    });

    expect(methods.getValues('SEARCH_CONCURRENT_REQUESTS')).toBe('16');
  });

  it('renders instance name input with current value', () => {
    renderWithForm(createMockConfig());

    const input = screen.getByLabelText(/search instance name/i);
    expect(input).toHaveValue('my-searx');
  });

  it('updates instance name', () => {
    const { methods } = renderWithForm(createMockConfig());

    fireEvent.change(screen.getByLabelText(/search instance name/i), {
      target: { value: 'new-instance' },
    });

    expect(methods.getValues('SEARCH_SEARXNG_INSTANCE_NAME')).toBe('new-instance');
  });

  it('displays info box about VPN requirement', () => {
    renderWithForm(createMockConfig());

    expect(screen.getByText(/web search requires vpn/i)).toBeInTheDocument();
  });

  it('displays error for results per query when provided', async () => {
    const { methods } = renderWithForm(createMockConfig());

    methods.setError('SEARCH_RESULT_COUNT', {
      type: 'manual',
      message: 'Must be between 1 and 20',
    });

    await waitFor(() => {
      expect(screen.getByText('Must be between 1 and 20')).toBeInTheDocument();
    });
  });
});
