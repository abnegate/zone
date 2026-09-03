import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { FormProvider, useForm, type UseFormReturn } from 'react-hook-form';
import type { InstallerConfig } from '../types';
import { SecurityStep } from './SecurityStep';

const createMockConfig = (overrides?: Partial<InstallerConfig>): InstallerConfig => ({
  DOMAIN_HOST_WEBUI: 'test.localhost',
  SECURITY_BASICAUTH_REALM: 'Zone',
  SECURITY_LITELLM_MASTER_KEY: 'master-key',
  SECURITY_LITELLM_SALT_KEY: 'salt-key',
  SECURITY_SEARXNG_SECRET_KEY: 'searx-key',
  SECURITY_MANAGER_API_KEY: 'api-key',
  POSTGRES_PASSWORD: 'pg-password',
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
  const utils = render(<SecurityStep />, { wrapper: Wrapper });
  if (!methods) {
    throw new Error('Form methods not initialized');
  }
  return { ...utils, methods };
};

describe('SecurityStep', () => {
  it('renders authentication realm input with current value', () => {
    renderWithForm(createMockConfig());

    expect(screen.getByLabelText(/authentication realm/i)).toHaveValue('Zone');
  });

  it('updates authentication realm', () => {
    const { methods } = renderWithForm(createMockConfig());

    fireEvent.change(screen.getByLabelText(/authentication realm/i), {
      target: { value: 'NewRealm' },
    });

    expect(methods.getValues('SECURITY_BASICAUTH_REALM')).toBe('NewRealm');
  });

  it('renders secret inputs with current values', () => {
    renderWithForm(createMockConfig());

    expect(screen.getByLabelText(/litellm master key/i)).toHaveValue('master-key');
    expect(screen.getByLabelText(/litellm salt key/i)).toHaveValue('salt-key');
    expect(screen.getByLabelText(/searxng secret key/i)).toHaveValue('searx-key');
    expect(screen.getByLabelText(/manager api key/i)).toHaveValue('api-key');
    expect(screen.getByLabelText(/postgresql password/i)).toHaveValue('pg-password');
  });

  it('generates all secrets when button is clicked', () => {
    const { methods } = renderWithForm(
      createMockConfig({
        SECURITY_LITELLM_MASTER_KEY: '',
        SECURITY_LITELLM_SALT_KEY: '',
        SECURITY_SEARXNG_SECRET_KEY: '',
        SECURITY_MANAGER_API_KEY: '',
        POSTGRES_PASSWORD: '',
      })
    );

    fireEvent.click(screen.getByRole('button', { name: /generate all secrets/i }));

    expect(methods.getValues('SECURITY_LITELLM_MASTER_KEY')).not.toBe('');
    expect(methods.getValues('SECURITY_LITELLM_SALT_KEY')).not.toBe('');
    expect(methods.getValues('SECURITY_SEARXNG_SECRET_KEY')).not.toBe('');
    expect(methods.getValues('SECURITY_MANAGER_API_KEY')).not.toBe('');
    expect(methods.getValues('POSTGRES_PASSWORD')).not.toBe('');
  });

  it('generates an individual secret when Generate is clicked', () => {
    const { methods } = renderWithForm(createMockConfig({ SECURITY_LITELLM_MASTER_KEY: '' }));

    const generateButtons = screen.getAllByRole('button', { name: /^generate$/i });
    fireEvent.click(generateButtons[0]);

    expect(methods.getValues('SECURITY_LITELLM_MASTER_KEY')).not.toBe('');
  });

  it('renders HTTPS redirect checkbox checked when enabled', () => {
    renderWithForm(createMockConfig());

    const checkbox = screen.getByLabelText(/enable https redirect/i);
    expect(checkbox).toBeChecked();
  });

  it('toggles HTTPS redirect setting', () => {
    const { methods } = renderWithForm(createMockConfig());

    fireEvent.click(screen.getByLabelText(/enable https redirect/i));
    expect(methods.getValues('SECURITY_HTTP_REDIRECT')).toBe('false');

    fireEvent.click(screen.getByLabelText(/enable https redirect/i));
    expect(methods.getValues('SECURITY_HTTP_REDIRECT')).toBe('true');
  });

  it('renders TLS certificate checkbox checked when enabled', () => {
    renderWithForm(createMockConfig());

    const checkbox = screen.getByLabelText(/auto-generate tls certificate/i);
    expect(checkbox).toBeChecked();
  });

  it('toggles TLS certificate setting', () => {
    const { methods } = renderWithForm(createMockConfig());

    fireEvent.click(screen.getByLabelText(/auto-generate tls certificate/i));
    expect(methods.getValues('SECURITY_GENERATE_CERTIFICATE')).toBe('false');

    fireEvent.click(screen.getByLabelText(/auto-generate tls certificate/i));
    expect(methods.getValues('SECURITY_GENERATE_CERTIFICATE')).toBe('true');
  });

  it('shows ACME email info box when TLS is enabled', () => {
    renderWithForm(createMockConfig());

    expect(screen.getByText(/set your acme email in advanced settings/i)).toBeInTheDocument();
  });

  it('hides ACME email info box when TLS is disabled', () => {
    renderWithForm(createMockConfig({ SECURITY_GENERATE_CERTIFICATE: 'false' }));

    expect(screen.queryByText(/set your acme email in advanced settings/i)).not.toBeInTheDocument();
  });

  it('displays warning about empty keys when a secret is missing', () => {
    renderWithForm(createMockConfig({ SECURITY_LITELLM_MASTER_KEY: '' }));

    expect(screen.getByText(/empty keys are insecure/i)).toBeInTheDocument();
  });

  it('hides warning about empty keys when all secrets are provided', () => {
    renderWithForm(createMockConfig());

    expect(screen.queryByText(/empty keys are insecure/i)).not.toBeInTheDocument();
  });

  it('displays Production Settings header', () => {
    renderWithForm(createMockConfig());

    expect(screen.getByText('Production Settings')).toBeInTheDocument();
  });

  it('displays error for field when provided', async () => {
    const { methods } = renderWithForm(createMockConfig());

    methods.setError('SECURITY_LITELLM_MASTER_KEY', {
      type: 'manual',
      message: 'Key is required',
    });

    await waitFor(() => {
      expect(screen.getByText('Key is required')).toBeInTheDocument();
    });
  });
});
