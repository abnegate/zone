import { fireEvent, render, screen } from '@testing-library/react';
import type { InstallerConfig } from '../types';
import { SecurityStep } from './SecurityStep';

// Mock the useSecretGenerator hook
jest.mock('../hooks', () => ({
  useSecretGenerator: () => ({
    generateSecret: jest.fn().mockReturnValue('generated-secret-123'),
  }),
}));

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

describe('SecurityStep', () => {
  const onChange = jest.fn();
  const getFieldError = jest.fn().mockReturnValue(undefined);

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('renders step header', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText('Security')).toBeInTheDocument();
    expect(
      screen.getByText(/configure authentication and generate secure keys/i)
    ).toBeInTheDocument();
  });

  it('renders authentication realm input with current value', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByLabelText(/authentication realm/i)).toHaveValue('Zone');
  });

  it('calls onChange when authentication realm is changed', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    fireEvent.change(screen.getByLabelText(/authentication realm/i), {
      target: { value: 'NewRealm' },
    });

    expect(onChange).toHaveBeenCalledWith('SECURITY_BASICAUTH_REALM', 'NewRealm');
  });

  it('renders LiteLLM master key input with current value', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByLabelText(/litellm master key/i)).toHaveValue('master-key');
  });

  it('calls onChange when LiteLLM master key is changed', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    fireEvent.change(screen.getByLabelText(/litellm master key/i), {
      target: { value: 'new-master-key' },
    });

    expect(onChange).toHaveBeenCalledWith('SECURITY_LITELLM_MASTER_KEY', 'new-master-key');
  });

  it('renders LiteLLM salt key input with current value', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByLabelText(/litellm salt key/i)).toHaveValue('salt-key');
  });

  it('renders SearXNG secret key input with current value', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByLabelText(/searxng secret key/i)).toHaveValue('searx-key');
  });

  it('renders Manager API key input with current value', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByLabelText(/manager api key/i)).toHaveValue('api-key');
  });

  it('renders PostgreSQL password input with current value', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByLabelText(/postgresql password/i)).toHaveValue('pg-password');
  });

  it('renders Generate All Secrets button', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByRole('button', { name: /generate all secrets/i })).toBeInTheDocument();
  });

  it('generates all secrets when button is clicked', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    fireEvent.click(screen.getByRole('button', { name: /generate all secrets/i }));

    expect(onChange).toHaveBeenCalledWith('SECURITY_LITELLM_MASTER_KEY', 'generated-secret-123');
    expect(onChange).toHaveBeenCalledWith('SECURITY_LITELLM_SALT_KEY', 'generated-secret-123');
    expect(onChange).toHaveBeenCalledWith('SECURITY_SEARXNG_SECRET_KEY', 'generated-secret-123');
    expect(onChange).toHaveBeenCalledWith('SECURITY_MANAGER_API_KEY', 'generated-secret-123');
    expect(onChange).toHaveBeenCalledWith('POSTGRES_PASSWORD', 'generated-secret-123');
  });

  it('renders individual generate buttons for each secret', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    // Each Input with onGenerate gets a Generate button
    const generateButtons = screen.getAllByRole('button', { name: /^generate$/i });
    expect(generateButtons.length).toBe(5);
  });

  it('generates individual LiteLLM master key when button is clicked', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    const generateButtons = screen.getAllByRole('button', { name: /^generate$/i });
    // LiteLLM Master Key is the first input with generate button
    fireEvent.click(generateButtons[0]);

    expect(onChange).toHaveBeenCalledWith('SECURITY_LITELLM_MASTER_KEY', 'generated-secret-123');
  });

  it('generates individual LiteLLM salt key when button is clicked', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    const generateButtons = screen.getAllByRole('button', { name: /^generate$/i });
    // LiteLLM Salt Key is the second input with generate button
    fireEvent.click(generateButtons[1]);

    expect(onChange).toHaveBeenCalledWith('SECURITY_LITELLM_SALT_KEY', 'generated-secret-123');
  });

  it('generates individual SearXNG secret key when button is clicked', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    const generateButtons = screen.getAllByRole('button', { name: /^generate$/i });
    // SearXNG Secret Key is the third input with generate button
    fireEvent.click(generateButtons[2]);

    expect(onChange).toHaveBeenCalledWith('SECURITY_SEARXNG_SECRET_KEY', 'generated-secret-123');
  });

  it('generates individual Manager API key when button is clicked', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    const generateButtons = screen.getAllByRole('button', { name: /^generate$/i });
    // Manager API Key is the fourth input with generate button
    fireEvent.click(generateButtons[3]);

    expect(onChange).toHaveBeenCalledWith('SECURITY_MANAGER_API_KEY', 'generated-secret-123');
  });

  it('generates individual PostgreSQL password when button is clicked', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    const generateButtons = screen.getAllByRole('button', { name: /^generate$/i });
    // PostgreSQL Password is the fifth input with generate button
    fireEvent.click(generateButtons[4]);

    expect(onChange).toHaveBeenCalledWith('POSTGRES_PASSWORD', 'generated-secret-123');
  });

  it('calls onChange when LiteLLM salt key is changed', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    fireEvent.change(screen.getByLabelText(/litellm salt key/i), {
      target: { value: 'new-salt-key' },
    });

    expect(onChange).toHaveBeenCalledWith('SECURITY_LITELLM_SALT_KEY', 'new-salt-key');
  });

  it('calls onChange when SearXNG secret key is changed', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    fireEvent.change(screen.getByLabelText(/searxng secret key/i), {
      target: { value: 'new-searx-key' },
    });

    expect(onChange).toHaveBeenCalledWith('SECURITY_SEARXNG_SECRET_KEY', 'new-searx-key');
  });

  it('calls onChange when Manager API key is changed', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    fireEvent.change(screen.getByLabelText(/manager api key/i), {
      target: { value: 'new-api-key' },
    });

    expect(onChange).toHaveBeenCalledWith('SECURITY_MANAGER_API_KEY', 'new-api-key');
  });

  it('calls onChange when PostgreSQL password is changed', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    fireEvent.change(screen.getByLabelText(/postgresql password/i), {
      target: { value: 'new-pg-password' },
    });

    expect(onChange).toHaveBeenCalledWith('POSTGRES_PASSWORD', 'new-pg-password');
  });

  it('renders HTTPS redirect checkbox checked when enabled', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    const checkbox = screen.getByLabelText(/enable https redirect/i);
    expect(checkbox).toBeChecked();
  });

  it('calls onChange when HTTPS redirect is toggled off', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    fireEvent.click(screen.getByLabelText(/enable https redirect/i));
    expect(onChange).toHaveBeenCalledWith('SECURITY_HTTP_REDIRECT', 'false');
  });

  it('calls onChange when HTTPS redirect is toggled on', () => {
    render(
      <SecurityStep
        config={createMockConfig({ SECURITY_HTTP_REDIRECT: 'false' })}
        onChange={onChange}
        getFieldError={getFieldError}
      />
    );

    fireEvent.click(screen.getByLabelText(/enable https redirect/i));
    expect(onChange).toHaveBeenCalledWith('SECURITY_HTTP_REDIRECT', 'true');
  });

  it('renders TLS certificate checkbox checked when enabled', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    const checkbox = screen.getByLabelText(/auto-generate tls certificate/i);
    expect(checkbox).toBeChecked();
  });

  it('calls onChange when TLS certificate is toggled off', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    fireEvent.click(screen.getByLabelText(/auto-generate tls certificate/i));
    expect(onChange).toHaveBeenCalledWith('SECURITY_GENERATE_CERTIFICATE', 'false');
  });

  it('calls onChange when TLS certificate is toggled on', () => {
    render(
      <SecurityStep
        config={createMockConfig({ SECURITY_GENERATE_CERTIFICATE: 'false' })}
        onChange={onChange}
        getFieldError={getFieldError}
      />
    );

    fireEvent.click(screen.getByLabelText(/auto-generate tls certificate/i));
    expect(onChange).toHaveBeenCalledWith('SECURITY_GENERATE_CERTIFICATE', 'true');
  });

  it('shows ACME email info box when TLS is enabled', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText(/set your acme email in advanced settings/i)).toBeInTheDocument();
  });

  it('hides ACME email info box when TLS is disabled', () => {
    render(
      <SecurityStep
        config={createMockConfig({ SECURITY_GENERATE_CERTIFICATE: 'false' })}
        onChange={onChange}
        getFieldError={getFieldError}
      />
    );

    expect(screen.queryByText(/set your acme email in advanced settings/i)).not.toBeInTheDocument();
  });

  it('displays warning about empty keys', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText(/empty keys are insecure/i)).toBeInTheDocument();
  });

  it('displays Production Settings header', () => {
    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText('Production Settings')).toBeInTheDocument();
  });

  it('displays error for field when provided', () => {
    getFieldError.mockImplementation((field: string) =>
      field === 'SECURITY_LITELLM_MASTER_KEY' ? 'Key is required' : undefined
    );

    render(
      <SecurityStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText('Key is required')).toBeInTheDocument();
  });
});
