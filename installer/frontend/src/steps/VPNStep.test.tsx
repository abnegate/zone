import { fireEvent, render, screen } from '@testing-library/react';
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

describe('VPNStep', () => {
  const onChange = jest.fn();
  const getFieldError = jest.fn().mockReturnValue(undefined);

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('renders step header', () => {
    render(
      <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText('VPN Configuration')).toBeInTheDocument();
    expect(screen.getByText(/configure vpn for private web search/i)).toBeInTheDocument();
  });

  it('renders provider select with current value', () => {
    render(
      <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    const select = screen.getByLabelText(/vpn provider/i);
    expect(select).toHaveValue('surfshark');
  });

  it('calls onChange when provider is changed', () => {
    render(
      <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    fireEvent.change(screen.getByLabelText(/vpn provider/i), {
      target: { value: 'nordvpn' },
    });

    expect(onChange).toHaveBeenCalledWith('VPN_SERVICE_PROVIDER', 'nordvpn');
  });

  it('renders protocol select with current value', () => {
    render(
      <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    const select = screen.getByLabelText(/protocol/i);
    expect(select).toHaveValue('openvpn');
  });

  it('calls onChange when protocol is changed', () => {
    render(
      <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    fireEvent.change(screen.getByLabelText(/protocol/i), {
      target: { value: 'wireguard' },
    });

    expect(onChange).toHaveBeenCalledWith('VPN_TYPE', 'wireguard');
  });

  describe('OpenVPN fields', () => {
    it('shows OpenVPN fields when protocol is openvpn', () => {
      render(
        <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      expect(screen.getByLabelText(/username/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/^password$/i)).toBeInTheDocument();
    });

    it('displays OpenVPN username value', () => {
      render(
        <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      expect(screen.getByLabelText(/username/i)).toHaveValue('user@email.com');
    });

    it('calls onChange when username is changed', () => {
      render(
        <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      fireEvent.change(screen.getByLabelText(/username/i), {
        target: { value: 'newuser' },
      });

      expect(onChange).toHaveBeenCalledWith('VPN_OPENVPN_USER', 'newuser');
    });

    it('calls onChange when password is changed', () => {
      render(
        <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      fireEvent.change(screen.getByLabelText(/^password$/i), {
        target: { value: 'newpassword' },
      });

      expect(onChange).toHaveBeenCalledWith('VPN_OPENVPN_PASSWORD', 'newpassword');
    });

    it('does not show WireGuard fields when protocol is openvpn', () => {
      render(
        <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      expect(screen.queryByLabelText(/private key/i)).not.toBeInTheDocument();
      expect(screen.queryByLabelText(/addresses/i)).not.toBeInTheDocument();
    });
  });

  describe('WireGuard fields', () => {
    const wireguardConfig = createMockConfig({
      VPN_TYPE: 'wireguard',
      VPN_WIREGUARD_PRIVATE_KEY: 'wg-private-key',
      VPN_WIREGUARD_ADDRESSES: '10.0.0.1/32',
    });

    it('shows WireGuard fields when protocol is wireguard', () => {
      render(
        <VPNStep config={wireguardConfig} onChange={onChange} getFieldError={getFieldError} />
      );

      expect(screen.getByLabelText(/private key/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/addresses/i)).toBeInTheDocument();
    });

    it('displays WireGuard private key value', () => {
      render(
        <VPNStep config={wireguardConfig} onChange={onChange} getFieldError={getFieldError} />
      );

      expect(screen.getByLabelText(/private key/i)).toHaveValue('wg-private-key');
    });

    it('displays WireGuard addresses value', () => {
      render(
        <VPNStep config={wireguardConfig} onChange={onChange} getFieldError={getFieldError} />
      );

      expect(screen.getByLabelText(/addresses/i)).toHaveValue('10.0.0.1/32');
    });

    it('calls onChange when private key is changed', () => {
      render(
        <VPNStep config={wireguardConfig} onChange={onChange} getFieldError={getFieldError} />
      );

      fireEvent.change(screen.getByLabelText(/private key/i), {
        target: { value: 'new-key' },
      });

      expect(onChange).toHaveBeenCalledWith('VPN_WIREGUARD_PRIVATE_KEY', 'new-key');
    });

    it('calls onChange when addresses is changed', () => {
      render(
        <VPNStep config={wireguardConfig} onChange={onChange} getFieldError={getFieldError} />
      );

      fireEvent.change(screen.getByLabelText(/addresses/i), {
        target: { value: '10.0.0.2/32' },
      });

      expect(onChange).toHaveBeenCalledWith('VPN_WIREGUARD_ADDRESSES', '10.0.0.2/32');
    });

    it('does not show OpenVPN fields when protocol is wireguard', () => {
      render(
        <VPNStep config={wireguardConfig} onChange={onChange} getFieldError={getFieldError} />
      );

      expect(screen.queryByLabelText(/username/i)).not.toBeInTheDocument();
      expect(screen.queryByLabelText(/^password$/i)).not.toBeInTheDocument();
    });
  });

  describe('Server location fields', () => {
    it('renders country input with current value', () => {
      render(
        <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      expect(screen.getByLabelText(/country/i)).toHaveValue('United States');
    });

    it('calls onChange when country is changed', () => {
      render(
        <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      fireEvent.change(screen.getByLabelText(/country/i), {
        target: { value: 'Germany' },
      });

      expect(onChange).toHaveBeenCalledWith('VPN_SERVER_COUNTRIES', 'Germany');
    });

    it('renders city input with current value', () => {
      render(
        <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      expect(screen.getByLabelText(/city/i)).toHaveValue('New York');
    });

    it('calls onChange when city is changed', () => {
      render(
        <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      fireEvent.change(screen.getByLabelText(/city/i), {
        target: { value: 'Los Angeles' },
      });

      expect(onChange).toHaveBeenCalledWith('VPN_SERVER_CITIES', 'Los Angeles');
    });

    it('renders region input with current value', () => {
      render(
        <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      expect(screen.getByLabelText(/region/i)).toHaveValue('California');
    });

    it('calls onChange when region is changed', () => {
      render(
        <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
      );

      fireEvent.change(screen.getByLabelText(/region/i), {
        target: { value: 'Texas' },
      });

      expect(onChange).toHaveBeenCalledWith('VPN_SERVER_REGIONS', 'Texas');
    });
  });

  it('displays info box about VPN being optional', () => {
    render(
      <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText(/vpn is optional/i)).toBeInTheDocument();
  });

  it('displays server location section header', () => {
    render(
      <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText('Server Location (Optional)')).toBeInTheDocument();
  });

  it('displays all provider options', () => {
    render(
      <VPNStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    const select = screen.getByLabelText(/vpn provider/i);
    const options = select.querySelectorAll('option');

    expect(options.length).toBe(5);
    expect(options[0]).toHaveValue('surfshark');
    expect(options[1]).toHaveValue('nordvpn');
    expect(options[2]).toHaveValue('expressvpn');
    expect(options[3]).toHaveValue('protonvpn');
    expect(options[4]).toHaveValue('mullvad');
  });
});
