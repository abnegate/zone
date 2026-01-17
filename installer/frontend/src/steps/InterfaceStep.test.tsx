import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { FormProvider, useForm, type UseFormReturn } from 'react-hook-form';
import type { InstallerConfig } from '../types';
import { InterfaceStep } from './InterfaceStep';

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
  ...overrides,
});

const renderWithForm = (defaultValues: InstallerConfig) => {
  let methods: UseFormReturn<InstallerConfig> | undefined;
  const Wrapper = ({ children }: { children: ReactNode }) => {
    const form = useForm<InstallerConfig>({ defaultValues });
    methods = form;
    return <FormProvider {...form}>{children}</FormProvider>;
  };
  const utils = render(<InterfaceStep />, { wrapper: Wrapper });
  if (!methods) {
    throw new Error('Form methods not initialized');
  }
  return { ...utils, methods };
};

describe('InterfaceStep', () => {
  it('renders authentication checkbox unchecked by default', () => {
    renderWithForm(createMockConfig());

    const checkbox = screen.getByLabelText(/enable built-in authentication/i);
    expect(checkbox).not.toBeChecked();
  });

  it('renders authentication checkbox checked when enabled', () => {
    renderWithForm(createMockConfig({ WEBUI_AUTH: 'true' }));

    const checkbox = screen.getByLabelText(/enable built-in authentication/i);
    expect(checkbox).toBeChecked();
  });

  it('toggles authentication setting', () => {
    const { methods } = renderWithForm(createMockConfig());

    fireEvent.click(screen.getByLabelText(/enable built-in authentication/i));
    expect(methods.getValues('WEBUI_AUTH')).toBe('true');

    fireEvent.click(screen.getByLabelText(/enable built-in authentication/i));
    expect(methods.getValues('WEBUI_AUTH')).toBe('false');
  });

  it('renders signup checkbox unchecked by default', () => {
    renderWithForm(createMockConfig());

    const checkbox = screen.getByLabelText(/allow user signups/i);
    expect(checkbox).not.toBeChecked();
  });

  it('toggles signup setting', () => {
    const { methods } = renderWithForm(createMockConfig());

    fireEvent.click(screen.getByLabelText(/allow user signups/i));
    expect(methods.getValues('WEBUI_ENABLE_SIGNUP')).toBe('true');

    fireEvent.click(screen.getByLabelText(/allow user signups/i));
    expect(methods.getValues('WEBUI_ENABLE_SIGNUP')).toBe('false');
  });

  it('renders language select with default value', () => {
    const { methods } = renderWithForm(createMockConfig());

    expect(methods.getValues('WEBUI_DEFAULT_LOCALE')).toBe('en-US');
    expect(screen.getByLabelText(/default language/i)).toHaveTextContent('English (US)');
  });

  it('updates language selection', async () => {
    const { methods } = renderWithForm(createMockConfig());

    // Radix UI Select requires clicking to open, then selecting an option
    fireEvent.click(screen.getByLabelText(/default language/i));
    await waitFor(() => {
      expect(screen.getByText('French')).toBeInTheDocument();
    });
    fireEvent.click(screen.getByText('French'));

    await waitFor(() => {
      expect(methods.getValues('WEBUI_DEFAULT_LOCALE')).toBe('fr-FR');
    });
  });
});
