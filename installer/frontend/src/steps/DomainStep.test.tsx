import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import { DomainStep } from './DomainStep';
import type { InstallerConfig } from '../types';

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
  OLLAMA_MODEL_FAST: '',
  OLLAMA_MODEL_REASON: '',
  OLLAMA_MODEL_EMBED: '',
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

describe('DomainStep', () => {
  it('renders hostname input', () => {
    const onChange = jest.fn();
    const getFieldError = jest.fn().mockReturnValue(undefined);
    render(
      <DomainStep
        config={createMockConfig()}
        onChange={onChange}
        getFieldError={getFieldError}
      />
    );

    expect(screen.getByLabelText(/Web Interface Hostname/i)).toBeInTheDocument();
  });

  it('displays current hostname value', () => {
    const onChange = jest.fn();
    const getFieldError = jest.fn().mockReturnValue(undefined);
    render(
      <DomainStep
        config={createMockConfig({ DOMAIN_HOST_WEBUI: 'my.host.com' })}
        onChange={onChange}
        getFieldError={getFieldError}
      />
    );

    expect(screen.getByDisplayValue('my.host.com')).toBeInTheDocument();
  });

  it('calls onChange when hostname changes', () => {
    const onChange = jest.fn();
    const getFieldError = jest.fn().mockReturnValue(undefined);
    render(
      <DomainStep
        config={createMockConfig()}
        onChange={onChange}
        getFieldError={getFieldError}
      />
    );

    fireEvent.change(screen.getByLabelText(/Web Interface Hostname/i), {
      target: { value: 'new.localhost' },
    });

    expect(onChange).toHaveBeenCalledWith('DOMAIN_HOST_WEBUI', 'new.localhost');
  });

  it('displays error when provided', () => {
    const onChange = jest.fn();
    const getFieldError = jest.fn().mockReturnValue('Hostname is required');
    render(
      <DomainStep
        config={createMockConfig()}
        onChange={onChange}
        getFieldError={getFieldError}
      />
    );

    expect(screen.getByText('Hostname is required')).toBeInTheDocument();
  });

  it('renders step title and description', () => {
    const onChange = jest.fn();
    const getFieldError = jest.fn().mockReturnValue(undefined);
    render(
      <DomainStep
        config={createMockConfig()}
        onChange={onChange}
        getFieldError={getFieldError}
      />
    );

    expect(screen.getByText('Domain Configuration')).toBeInTheDocument();
    expect(screen.getByText('Configure hostnames for your services')).toBeInTheDocument();
  });
});
