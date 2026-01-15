import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { FormProvider, useForm, type UseFormReturn } from 'react-hook-form';
import type { InstallerConfig } from '../types';
import { AdvancedStep } from './AdvancedStep';

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

const renderWithForm = (defaultValues: InstallerConfig) => {
  let methods: UseFormReturn<InstallerConfig> | undefined;
  const Wrapper = ({ children }: { children: ReactNode }) => {
    const form = useForm<InstallerConfig>({ defaultValues });
    methods = form;
    return <FormProvider {...form}>{children}</FormProvider>;
  };

  const utils = render(<AdvancedStep />, { wrapper: Wrapper });

  if (!methods) {
    throw new Error('Form methods not initialized');
  }

  return { ...utils, methods };
};

describe('AdvancedStep', () => {
  describe('Monitoring section', () => {
    it('renders monitoring checkbox unchecked by default', () => {
      renderWithForm(createMockConfig());

      const checkbox = screen.getByLabelText(/enable prometheus \+ grafana monitoring/i);
      expect(checkbox).not.toBeChecked();
    });

    it('shows monitoring fields when monitoring is enabled', () => {
      renderWithForm(createMockConfig({ MONITORING_ENABLED: 'true' }));

      expect(screen.getByLabelText(/grafana admin username/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/grafana admin password/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/metrics retention/i)).toBeInTheDocument();
    });

    it('hides monitoring fields when monitoring is disabled', () => {
      renderWithForm(createMockConfig());

      expect(screen.queryByLabelText(/grafana admin username/i)).not.toBeInTheDocument();
      expect(screen.queryByLabelText(/grafana admin password/i)).not.toBeInTheDocument();
      expect(screen.queryByLabelText(/metrics retention/i)).not.toBeInTheDocument();
    });

    it('enables monitoring and generates password when toggled on with empty password', () => {
      const { methods } = renderWithForm(createMockConfig());

      fireEvent.click(screen.getByLabelText(/enable prometheus \+ grafana monitoring/i));

      const passwordInput = screen.getByLabelText(/grafana admin password/i);
      expect(passwordInput).not.toHaveValue('');
      expect(methods.getValues('MONITORING_ENABLED')).toBe('true');
    });

    it('does not override existing Grafana password when toggled on', () => {
      renderWithForm(
        createMockConfig({ MONITORING_GRAFANA_ADMIN_PASSWORD: 'existing-pw' })
      );

      fireEvent.click(screen.getByLabelText(/enable prometheus \+ grafana monitoring/i));

      expect(screen.getByLabelText(/grafana admin password/i)).toHaveValue('existing-pw');
    });

    it('disables alerting when monitoring is disabled', () => {
      const { methods } = renderWithForm(
        createMockConfig({ MONITORING_ENABLED: 'true', ALERT_ENABLED: 'true' })
      );

      fireEvent.click(screen.getByLabelText(/enable prometheus \+ grafana monitoring/i));

      expect(methods.getValues('ALERT_ENABLED')).toBe('false');
    });

    it('updates Grafana admin username', () => {
      renderWithForm(createMockConfig({ MONITORING_ENABLED: 'true' }));

      fireEvent.change(screen.getByLabelText(/grafana admin username/i), {
        target: { value: 'newadmin' },
      });

      expect(screen.getByLabelText(/grafana admin username/i)).toHaveValue('newadmin');
    });

    it('displays retention select with current value', () => {
      renderWithForm(createMockConfig({ MONITORING_ENABLED: 'true' }));

      expect(screen.getByLabelText(/metrics retention/i)).toHaveValue('15d');
    });

    it('updates retention selection', () => {
      const { methods } = renderWithForm(createMockConfig({ MONITORING_ENABLED: 'true' }));

      fireEvent.change(screen.getByLabelText(/metrics retention/i), {
        target: { value: '30d' },
      });

      expect(methods.getValues('MONITORING_RETENTION_TIME')).toBe('30d');
    });
  });

  describe('Alerting section', () => {
    it('shows alerting checkbox when monitoring is enabled', () => {
      renderWithForm(createMockConfig({ MONITORING_ENABLED: 'true' }));

      expect(screen.getByLabelText(/enable email alerts/i)).toBeInTheDocument();
    });

    it('shows alert fields when alerting is enabled', () => {
      renderWithForm(createMockConfig({ MONITORING_ENABLED: 'true', ALERT_ENABLED: 'true' }));

      expect(screen.getByLabelText(/alert recipients/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/smtp host/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/smtp port/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/smtp username/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/smtp password/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/from address/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/from name/i)).toBeInTheDocument();
    });

    it('hides alert fields when alerting is disabled', () => {
      renderWithForm(createMockConfig({ MONITORING_ENABLED: 'true', ALERT_ENABLED: 'false' }));

      expect(screen.queryByLabelText(/alert recipients/i)).not.toBeInTheDocument();
      expect(screen.queryByLabelText(/smtp host/i)).not.toBeInTheDocument();
    });

    it('toggles alerting on and off', () => {
      const { methods } = renderWithForm(createMockConfig({ MONITORING_ENABLED: 'true' }));

      fireEvent.click(screen.getByLabelText(/enable email alerts/i));
      expect(methods.getValues('ALERT_ENABLED')).toBe('true');

      fireEvent.click(screen.getByLabelText(/enable email alerts/i));
      expect(methods.getValues('ALERT_ENABLED')).toBe('false');
    });

    it('updates alert fields', () => {
      const { methods } = renderWithForm(
        createMockConfig({ MONITORING_ENABLED: 'true', ALERT_ENABLED: 'true' })
      );

      fireEvent.change(screen.getByLabelText(/alert recipients/i), {
        target: { value: 'test@example.com' },
      });
      expect(methods.getValues('ALERT_EMAIL_RECIPIENTS')).toBe('test@example.com');

      fireEvent.change(screen.getByLabelText(/smtp host/i), {
        target: { value: 'smtp.gmail.com' },
      });
      expect(methods.getValues('ALERT_SMTP_HOST')).toBe('smtp.gmail.com');

      fireEvent.change(screen.getByLabelText(/smtp port/i), {
        target: { value: '465' },
      });
      expect(methods.getValues('ALERT_SMTP_PORT')).toBe('465');

      fireEvent.change(screen.getByLabelText(/smtp username/i), {
        target: { value: 'user@gmail.com' },
      });
      expect(methods.getValues('ALERT_SMTP_USER')).toBe('user@gmail.com');

      fireEvent.change(screen.getByLabelText(/smtp password/i), {
        target: { value: 'app-password' },
      });
      expect(methods.getValues('ALERT_SMTP_PASSWORD')).toBe('app-password');

      fireEvent.change(screen.getByLabelText(/from address/i), {
        target: { value: 'alerts@zone.io' },
      });
      expect(methods.getValues('ALERT_SMTP_FROM_ADDRESS')).toBe('alerts@zone.io');

      fireEvent.change(screen.getByLabelText(/from name/i), {
        target: { value: 'Zone Alerts' },
      });
      expect(methods.getValues('ALERT_SMTP_FROM_NAME')).toBe('Zone Alerts');
    });
  });

  describe('Performance section', () => {
    it('renders worker count input with current value', () => {
      renderWithForm(createMockConfig());

      expect(screen.getByLabelText(/worker count/i)).toHaveValue(4);
    });

    it('updates worker count', () => {
      const { methods } = renderWithForm(createMockConfig());

      fireEvent.change(screen.getByLabelText(/worker count/i), {
        target: { value: '8' },
      });

      expect(methods.getValues('ADVANCED_LITELLM_WORKERS')).toBe('8');
    });

    it('renders request timeout input with current value', () => {
      renderWithForm(createMockConfig());

      expect(screen.getByLabelText(/request timeout/i)).toHaveValue(600);
    });

    it('updates request timeout', () => {
      const { methods } = renderWithForm(createMockConfig());

      fireEvent.change(screen.getByLabelText(/request timeout/i), {
        target: { value: '300' },
      });

      expect(methods.getValues('ADVANCED_LITELLM_REQUEST_TIMEOUT')).toBe('300');
    });

    it('renders timezone select with current value', () => {
      renderWithForm(createMockConfig());

      expect(screen.getByLabelText(/timezone/i)).toHaveValue('UTC');
    });

    it('updates timezone', () => {
      const { methods } = renderWithForm(createMockConfig());

      fireEvent.change(screen.getByLabelText(/timezone/i), {
        target: { value: 'America/New_York' },
      });

      expect(methods.getValues('ADVANCED_TZ')).toBe('America/New_York');
    });

    it('renders ACME email input with current value', () => {
      renderWithForm(createMockConfig());

      expect(screen.getByLabelText(/acme email/i)).toHaveValue('admin@example.com');
    });

    it('updates ACME email', () => {
      const { methods } = renderWithForm(createMockConfig());

      fireEvent.change(screen.getByLabelText(/acme email/i), {
        target: { value: 'new@example.com' },
      });

      expect(methods.getValues('ADVANCED_ACME_EMAIL')).toBe('new@example.com');
    });
  });

  it('displays completion info box', () => {
    renderWithForm(createMockConfig());

    expect(screen.getByText(/configuration complete/i)).toBeInTheDocument();
  });

  it('displays Monitoring section header', () => {
    renderWithForm(createMockConfig());

    expect(screen.getByText('Monitoring')).toBeInTheDocument();
  });

  it('displays Performance section header', () => {
    renderWithForm(createMockConfig());

    expect(screen.getByText('Performance')).toBeInTheDocument();
  });

  it('displays field errors when provided', async () => {
    const { methods } = renderWithForm(createMockConfig());

    methods.setError('ADVANCED_LITELLM_WORKERS', {
      type: 'manual',
      message: 'Must be between 1 and 16',
    });

    await waitFor(() => {
      expect(screen.getByText('Must be between 1 and 16')).toBeInTheDocument();
    });
  });
});
