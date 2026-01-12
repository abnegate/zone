import { fireEvent, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { FormProvider, useForm, type UseFormReturn } from 'react-hook-form';
import type { InstallerConfig } from '../types';
import { VPNStep } from './VPNStep';

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
  VPN_SERVICE_PROVIDER: 'surfshark',
  VPN_TYPE: 'openvpn',
  VPN_OPENVPN_USER: 'user@email.com',
  VPN_OPENVPN_PASSWORD: 'secret123',
  VPN_WIREGUARD_PRIVATE_KEY: '',
  VPN_WIREGUARD_ADDRESSES: '',
  VPN_SERVER_COUNTRIES: 'United States',
  VPN_SERVER_CITIES: 'New York',
  VPN_SERVER_REGIONS: 'California',
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
  const utils = render(<VPNStep />, { wrapper: Wrapper });
  if (!methods) {
    throw new Error('Form methods not initialized');
  }
  return { ...utils, methods };
};

describe('VPNStep', () => {
  it('renders step header', () => {
    renderWithForm(createMockConfig());

    expect(screen.getByText('VPN Configuration')).toBeInTheDocument();
    expect(screen.getByText(/configure vpn for private web search/i)).toBeInTheDocument();
  });

  it('renders provider select with current value', () => {
    renderWithForm(createMockConfig());

    const select = screen.getByLabelText(/vpn provider/i);
    expect(select).toHaveValue('surfshark');
  });

  it('updates provider selection', () => {
    const { methods } = renderWithForm(createMockConfig());

    fireEvent.change(screen.getByLabelText(/vpn provider/i), {
      target: { value: 'nordvpn' },
    });

    expect(methods.getValues('VPN_SERVICE_PROVIDER')).toBe('nordvpn');
  });

  it('renders protocol select with current value', () => {
    renderWithForm(createMockConfig());

    const select = screen.getByLabelText(/protocol/i);
    expect(select).toHaveValue('openvpn');
  });

  it('updates protocol selection', () => {
    const { methods } = renderWithForm(createMockConfig());

    fireEvent.change(screen.getByLabelText(/protocol/i), {
      target: { value: 'wireguard' },
    });

    expect(methods.getValues('VPN_TYPE')).toBe('wireguard');
  });

  it('shows OpenVPN fields when protocol is openvpn', () => {
    renderWithForm(createMockConfig());

    expect(screen.getByLabelText(/username/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/^password$/i)).toBeInTheDocument();
    expect(screen.queryByLabelText(/private key/i)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/addresses/i)).not.toBeInTheDocument();
  });

  it('updates OpenVPN credentials', () => {
    const { methods } = renderWithForm(createMockConfig());

    fireEvent.change(screen.getByLabelText(/username/i), {
      target: { value: 'newuser' },
    });
    fireEvent.change(screen.getByLabelText(/^password$/i), {
      target: { value: 'newpassword' },
    });

    expect(methods.getValues('VPN_OPENVPN_USER')).toBe('newuser');
    expect(methods.getValues('VPN_OPENVPN_PASSWORD')).toBe('newpassword');
  });

  it('shows WireGuard fields when protocol is wireguard', () => {
    renderWithForm(
      createMockConfig({
        VPN_TYPE: 'wireguard',
        VPN_WIREGUARD_PRIVATE_KEY: 'wg-private-key',
        VPN_WIREGUARD_ADDRESSES: '10.0.0.1/32',
      })
    );

    expect(screen.getByLabelText(/private key/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/addresses/i)).toBeInTheDocument();
    expect(screen.queryByLabelText(/username/i)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/^password$/i)).not.toBeInTheDocument();
  });

  it('updates WireGuard settings', () => {
    const { methods } = renderWithForm(
      createMockConfig({
        VPN_TYPE: 'wireguard',
        VPN_WIREGUARD_PRIVATE_KEY: '',
        VPN_WIREGUARD_ADDRESSES: '',
      })
    );

    fireEvent.change(screen.getByLabelText(/private key/i), {
      target: { value: 'new-key' },
    });
    fireEvent.change(screen.getByLabelText(/addresses/i), {
      target: { value: '10.0.0.2/32' },
    });

    expect(methods.getValues('VPN_WIREGUARD_PRIVATE_KEY')).toBe('new-key');
    expect(methods.getValues('VPN_WIREGUARD_ADDRESSES')).toBe('10.0.0.2/32');
  });

  it('renders server location inputs with current values', () => {
    renderWithForm(createMockConfig());

    expect(screen.getByLabelText(/country/i)).toHaveValue('United States');
    expect(screen.getByLabelText(/city/i)).toHaveValue('New York');
    expect(screen.getByLabelText(/region/i)).toHaveValue('California');
  });

  it('displays VPN info box', () => {
    renderWithForm(createMockConfig());

    expect(screen.getByText(/docker compose --profile vpn up/i)).toBeInTheDocument();
  });
});
