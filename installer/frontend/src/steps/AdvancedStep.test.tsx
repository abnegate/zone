import { fireEvent, render, screen } from '@testing-library/react';
import type { InstallerConfig } from '../types';
import { AdvancedStep } from './AdvancedStep';

// Mock the useSecretGenerator hook
jest.mock('../hooks', () => ({
  useSecretGenerator: () => ({
    generateSecret: jest.fn().mockReturnValue('generated-secret-456'),
  }),
}));

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
  ADVANCED_ACME_EMAIL: 'admin@example.com',
  SECURITY_BASIC_AUTH_USERS_FILE: '',
  OLLAMA_HOST: '',
  OLLAMA_KEEP_ALIVE: '',
  OLLAMA_MAX_LOADED_MODELS: '',
  ...overrides,
});

describe('AdvancedStep', () => {
  const onChange = jest.fn();
  const getFieldError = jest.fn().mockReturnValue(undefined);

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('renders step header', () => {
    render(
      <AdvancedStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText('Advanced Settings')).toBeInTheDocument();
    expect(screen.getByText(/performance tuning and system configuration/i)).toBeInTheDocument();
  });

  describe('Monitoring section', () => {
    it('renders monitoring checkbox unchecked by default', () => {
      render(
        <AdvancedStep
          config={createMockConfig()}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      const checkbox = screen.getByLabelText(/enable prometheus \+ grafana monitoring/i);
      expect(checkbox).not.toBeChecked();
    });

    it('shows monitoring fields when monitoring is enabled', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_ENABLED: 'true' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByLabelText(/grafana admin username/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/grafana admin password/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/metrics retention/i)).toBeInTheDocument();
    });

    it('hides monitoring fields when monitoring is disabled', () => {
      render(
        <AdvancedStep
          config={createMockConfig()}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.queryByLabelText(/grafana admin username/i)).not.toBeInTheDocument();
      expect(screen.queryByLabelText(/grafana admin password/i)).not.toBeInTheDocument();
      expect(screen.queryByLabelText(/metrics retention/i)).not.toBeInTheDocument();
    });

    it('enables monitoring and generates password when toggled on with empty password', () => {
      render(
        <AdvancedStep
          config={createMockConfig()}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.click(screen.getByLabelText(/enable prometheus \+ grafana monitoring/i));

      expect(onChange).toHaveBeenCalledWith('MONITORING_ENABLED', 'true');
      expect(onChange).toHaveBeenCalledWith(
        'MONITORING_GRAFANA_ADMIN_PASSWORD',
        'generated-secret-456'
      );
    });

    it('does not regenerate password when toggled on if password exists', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_GRAFANA_ADMIN_PASSWORD: 'existing-pw' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.click(screen.getByLabelText(/enable prometheus \+ grafana monitoring/i));

      expect(onChange).toHaveBeenCalledWith('MONITORING_ENABLED', 'true');
      expect(onChange).not.toHaveBeenCalledWith(
        'MONITORING_GRAFANA_ADMIN_PASSWORD',
        expect.anything()
      );
    });

    it('disables alerting when monitoring is disabled', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_ENABLED: 'true', ALERT_ENABLED: 'true' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.click(screen.getByLabelText(/enable prometheus \+ grafana monitoring/i));

      expect(onChange).toHaveBeenCalledWith('MONITORING_ENABLED', 'false');
      expect(onChange).toHaveBeenCalledWith('ALERT_ENABLED', 'false');
    });

    it('displays Grafana admin username value', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_ENABLED: 'true' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByLabelText(/grafana admin username/i)).toHaveValue('admin');
    });

    it('calls onChange when Grafana username is changed', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_ENABLED: 'true' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.change(screen.getByLabelText(/grafana admin username/i), {
        target: { value: 'newadmin' },
      });

      expect(onChange).toHaveBeenCalledWith('MONITORING_GRAFANA_ADMIN_USER', 'newadmin');
    });

    it('displays retention select with current value', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_ENABLED: 'true' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByLabelText(/metrics retention/i)).toHaveValue('15d');
    });

    it('calls onChange when retention is changed', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_ENABLED: 'true' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.change(screen.getByLabelText(/metrics retention/i), {
        target: { value: '30d' },
      });

      expect(onChange).toHaveBeenCalledWith('MONITORING_RETENTION_TIME', '30d');
    });
  });

  describe('Alerting section', () => {
    it('shows alerting checkbox when monitoring is enabled', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_ENABLED: 'true' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByLabelText(/enable email alerts/i)).toBeInTheDocument();
    });

    it('shows alert fields when alerting is enabled', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_ENABLED: 'true', ALERT_ENABLED: 'true' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByLabelText(/alert recipients/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/smtp host/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/smtp port/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/smtp username/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/smtp password/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/from address/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/from name/i)).toBeInTheDocument();
    });

    it('hides alert fields when alerting is disabled', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_ENABLED: 'true', ALERT_ENABLED: 'false' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.queryByLabelText(/alert recipients/i)).not.toBeInTheDocument();
      expect(screen.queryByLabelText(/smtp host/i)).not.toBeInTheDocument();
    });

    it('calls onChange when alerting is toggled on', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_ENABLED: 'true' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.click(screen.getByLabelText(/enable email alerts/i));
      expect(onChange).toHaveBeenCalledWith('ALERT_ENABLED', 'true');
    });

    it('calls onChange when alerting is toggled off', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_ENABLED: 'true', ALERT_ENABLED: 'true' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.click(screen.getByLabelText(/enable email alerts/i));
      expect(onChange).toHaveBeenCalledWith('ALERT_ENABLED', 'false');
    });

    it('calls onChange when alert fields are changed', () => {
      render(
        <AdvancedStep
          config={createMockConfig({ MONITORING_ENABLED: 'true', ALERT_ENABLED: 'true' })}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.change(screen.getByLabelText(/alert recipients/i), {
        target: { value: 'test@example.com' },
      });
      expect(onChange).toHaveBeenCalledWith('ALERT_EMAIL_RECIPIENTS', 'test@example.com');

      fireEvent.change(screen.getByLabelText(/smtp host/i), {
        target: { value: 'smtp.gmail.com' },
      });
      expect(onChange).toHaveBeenCalledWith('ALERT_SMTP_HOST', 'smtp.gmail.com');

      fireEvent.change(screen.getByLabelText(/smtp port/i), {
        target: { value: '465' },
      });
      expect(onChange).toHaveBeenCalledWith('ALERT_SMTP_PORT', '465');

      fireEvent.change(screen.getByLabelText(/smtp username/i), {
        target: { value: 'user@gmail.com' },
      });
      expect(onChange).toHaveBeenCalledWith('ALERT_SMTP_USER', 'user@gmail.com');

      fireEvent.change(screen.getByLabelText(/smtp password/i), {
        target: { value: 'app-password' },
      });
      expect(onChange).toHaveBeenCalledWith('ALERT_SMTP_PASSWORD', 'app-password');

      fireEvent.change(screen.getByLabelText(/from address/i), {
        target: { value: 'alerts@zone.io' },
      });
      expect(onChange).toHaveBeenCalledWith('ALERT_SMTP_FROM_ADDRESS', 'alerts@zone.io');

      fireEvent.change(screen.getByLabelText(/from name/i), {
        target: { value: 'Zone Alerts' },
      });
      expect(onChange).toHaveBeenCalledWith('ALERT_SMTP_FROM_NAME', 'Zone Alerts');
    });
  });

  describe('Performance section', () => {
    it('renders worker count input with current value', () => {
      render(
        <AdvancedStep
          config={createMockConfig()}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByLabelText(/worker count/i)).toHaveValue(4);
    });

    it('calls onChange when worker count is changed', () => {
      render(
        <AdvancedStep
          config={createMockConfig()}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.change(screen.getByLabelText(/worker count/i), {
        target: { value: '8' },
      });

      expect(onChange).toHaveBeenCalledWith('ADVANCED_LITELLM_WORKERS', '8');
    });

    it('renders request timeout input with current value', () => {
      render(
        <AdvancedStep
          config={createMockConfig()}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByLabelText(/request timeout/i)).toHaveValue(600);
    });

    it('calls onChange when request timeout is changed', () => {
      render(
        <AdvancedStep
          config={createMockConfig()}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.change(screen.getByLabelText(/request timeout/i), {
        target: { value: '300' },
      });

      expect(onChange).toHaveBeenCalledWith('ADVANCED_LITELLM_REQUEST_TIMEOUT', '300');
    });

    it('renders timezone select with current value', () => {
      render(
        <AdvancedStep
          config={createMockConfig()}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByLabelText(/timezone/i)).toHaveValue('UTC');
    });

    it('calls onChange when timezone is changed', () => {
      render(
        <AdvancedStep
          config={createMockConfig()}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.change(screen.getByLabelText(/timezone/i), {
        target: { value: 'America/New_York' },
      });

      expect(onChange).toHaveBeenCalledWith('ADVANCED_TZ', 'America/New_York');
    });

    it('renders ACME email input with current value', () => {
      render(
        <AdvancedStep
          config={createMockConfig()}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      expect(screen.getByLabelText(/acme email/i)).toHaveValue('admin@example.com');
    });

    it('calls onChange when ACME email is changed', () => {
      render(
        <AdvancedStep
          config={createMockConfig()}
          onChange={onChange}
          getFieldError={getFieldError}
        />
      );

      fireEvent.change(screen.getByLabelText(/acme email/i), {
        target: { value: 'new@example.com' },
      });

      expect(onChange).toHaveBeenCalledWith('ADVANCED_ACME_EMAIL', 'new@example.com');
    });
  });

  it('displays completion info box', () => {
    render(
      <AdvancedStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText(/configuration complete/i)).toBeInTheDocument();
  });

  it('displays Monitoring section header', () => {
    render(
      <AdvancedStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText('Monitoring')).toBeInTheDocument();
  });

  it('displays Performance section header', () => {
    render(
      <AdvancedStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText('Performance')).toBeInTheDocument();
  });

  it('displays field errors when provided', () => {
    getFieldError.mockImplementation((field: string) =>
      field === 'ADVANCED_LITELLM_WORKERS' ? 'Must be between 1 and 16' : undefined
    );

    render(
      <AdvancedStep config={createMockConfig()} onChange={onChange} getFieldError={getFieldError} />
    );

    expect(screen.getByText('Must be between 1 and 16')).toBeInTheDocument();
  });
});
