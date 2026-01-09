import { act, renderHook } from '@testing-library/react';
import { useValidation } from './useValidation';

describe('useValidation', () => {
  it('returns no errors for valid domain config', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('domain', {
        DOMAIN_HOST_WEBUI: 'webui.localhost',
      });
      expect(isValid).toBe(true);
    });

    expect(result.current.errors).toEqual({});
  });

  it('returns error for empty hostname', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('domain', {
        DOMAIN_HOST_WEBUI: '',
      });
      expect(isValid).toBe(false);
    });

    expect(result.current.errors.DOMAIN_HOST_WEBUI).toBeDefined();
  });

  it('returns error for invalid hostname format', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('domain', {
        DOMAIN_HOST_WEBUI: 'invalid hostname with spaces',
      });
      expect(isValid).toBe(false);
    });

    expect(result.current.errors.DOMAIN_HOST_WEBUI).toContain('Invalid hostname');
  });

  it('validates security keys minimum length', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('security', {
        SECURITY_BASICAUTH_REALM: 'Test',
        SECURITY_LITELLM_MASTER_KEY: 'short',
        SECURITY_LITELLM_SALT_KEY: 'short',
        SECURITY_SEARXNG_SECRET_KEY: 'short',
        SECURITY_MANAGER_API_KEY: 'short',
        POSTGRES_PASSWORD: 'short',
      });
      expect(isValid).toBe(false);
    });

    expect(result.current.errors.SECURITY_LITELLM_MASTER_KEY).toContain('at least 16');
  });

  it('clears errors', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      result.current.validateStep('domain', { DOMAIN_HOST_WEBUI: '' });
    });

    expect(Object.keys(result.current.errors).length).toBeGreaterThan(0);

    act(() => {
      result.current.clearErrors();
    });

    expect(result.current.errors).toEqual({});
  });

  it('getFieldError returns error for specific field', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      result.current.validateStep('domain', { DOMAIN_HOST_WEBUI: '' });
    });

    expect(result.current.getFieldError('DOMAIN_HOST_WEBUI')).toBeDefined();
    expect(result.current.getFieldError('OTHER_FIELD')).toBeUndefined();
  });

  it('hasErrors is true when errors exist', () => {
    const { result } = renderHook(() => useValidation());

    expect(result.current.hasErrors).toBe(false);

    act(() => {
      result.current.validateStep('domain', { DOMAIN_HOST_WEBUI: '' });
    });

    expect(result.current.hasErrors).toBe(true);
  });

  it('validates search step number fields', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('search', {
        SEARCH_ENABLE_WEB_SEARCH: 'true',
        SEARCH_RESULT_COUNT: '25', // Over max of 20
        SEARCH_CONCURRENT_REQUESTS: '8',
        SEARCH_SEARXNG_INSTANCE_NAME: 'Test',
      });
      expect(isValid).toBe(false);
    });

    expect(result.current.errors.SEARCH_RESULT_COUNT).toBeDefined();
  });

  it('validates email format in advanced step', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('advanced', {
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
        ADVANCED_ACME_EMAIL: 'not-an-email',
      });
      expect(isValid).toBe(false);
    });

    expect(result.current.errors.ADVANCED_ACME_EMAIL).toContain('email');
  });

  it('returns true for unknown step id (no schema)', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      // Cast to any to test with invalid step id
      const isValid = result.current.validateStep('unknownStep' as any, {});
      expect(isValid).toBe(true);
    });

    expect(result.current.errors).toEqual({});
  });

  it('validates models step', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('models', {
        AI_PROVIDER: 'self_hosted',
        AI_LITELLM_HOST: 'http://localhost:4000',
        AI_LITELLM_KEY: '',
        AI_OPENAI_API_KEY: '',
        AI_OPENAI_BASE_URL: '',
        AI_ANTHROPIC_API_KEY: '',
        AI_ANTHROPIC_BASE_URL: '',
        AI_BEDROCK_REGION: '',
        AI_BEDROCK_ACCESS_KEY: '',
        AI_BEDROCK_SECRET_KEY: '',
        AI_BEDROCK_USE_IAM_ROLE: 'false',
        AI_MODEL_FAST: 'llama3.1:8b',
        AI_MODEL_REASONING: 'deepseek-r1:7b',
        AI_MODEL_EMBEDDING: 'nomic-embed-text',
      });
      expect(isValid).toBe(true);
    });
  });

  it('validates interface step', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('interface', {
        WEBUI_AUTH: 'true',
        WEBUI_ENABLE_SIGNUP: 'false',
        WEBUI_DEFAULT_LOCALE: 'en-US',
      });
      expect(isValid).toBe(true);
    });
  });

  it('validates vpn step', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('vpn', {
        VPN_SERVICE_PROVIDER: 'surfshark',
        VPN_TYPE: 'openvpn',
        VPN_OPENVPN_USER: '',
        VPN_OPENVPN_PASSWORD: '',
        VPN_WIREGUARD_PRIVATE_KEY: '',
        VPN_WIREGUARD_ADDRESSES: '',
        VPN_SERVER_COUNTRIES: '',
        VPN_SERVER_CITIES: '',
        VPN_SERVER_REGIONS: '',
      });
      expect(isValid).toBe(true);
    });
  });

  it('requires Grafana password when monitoring is enabled', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('advanced', {
        MONITORING_ENABLED: 'true',
        MONITORING_GRAFANA_ADMIN_USER: 'admin',
        MONITORING_GRAFANA_ADMIN_PASSWORD: 'short', // Less than 8 chars
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
      });
      expect(isValid).toBe(false);
    });

    expect(result.current.errors.MONITORING_GRAFANA_ADMIN_PASSWORD).toContain('at least 8');
  });

  it('requires SMTP host when alerting is enabled', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('advanced', {
        MONITORING_ENABLED: 'true',
        MONITORING_GRAFANA_ADMIN_USER: 'admin',
        MONITORING_GRAFANA_ADMIN_PASSWORD: 'longenoughpassword',
        MONITORING_RETENTION_TIME: '15d',
        ALERT_ENABLED: 'true',
        ALERT_EMAIL_RECIPIENTS: 'test@example.com',
        ALERT_SMTP_HOST: '', // Empty
        ALERT_SMTP_PORT: '587',
        ALERT_SMTP_USER: '',
        ALERT_SMTP_PASSWORD: '',
        ALERT_SMTP_FROM_ADDRESS: '',
        ALERT_SMTP_FROM_NAME: '',
        ADVANCED_LITELLM_WORKERS: '4',
        ADVANCED_LITELLM_REQUEST_TIMEOUT: '600',
        ADVANCED_TZ: 'UTC',
        ADVANCED_ACME_EMAIL: 'admin@example.com',
      });
      expect(isValid).toBe(false);
    });

    expect(result.current.errors.ALERT_SMTP_HOST).toContain('required');
  });

  it('requires alert recipients when alerting is enabled', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('advanced', {
        MONITORING_ENABLED: 'true',
        MONITORING_GRAFANA_ADMIN_USER: 'admin',
        MONITORING_GRAFANA_ADMIN_PASSWORD: 'longenoughpassword',
        MONITORING_RETENTION_TIME: '15d',
        ALERT_ENABLED: 'true',
        ALERT_EMAIL_RECIPIENTS: '', // Empty
        ALERT_SMTP_HOST: 'smtp.example.com',
        ALERT_SMTP_PORT: '587',
        ALERT_SMTP_USER: '',
        ALERT_SMTP_PASSWORD: '',
        ALERT_SMTP_FROM_ADDRESS: '',
        ALERT_SMTP_FROM_NAME: '',
        ADVANCED_LITELLM_WORKERS: '4',
        ADVANCED_LITELLM_REQUEST_TIMEOUT: '600',
        ADVANCED_TZ: 'UTC',
        ADVANCED_ACME_EMAIL: 'admin@example.com',
      });
      expect(isValid).toBe(false);
    });

    expect(result.current.errors.ALERT_EMAIL_RECIPIENTS).toContain('required');
  });

  it('passes when monitoring is disabled and password is short', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('advanced', {
        MONITORING_ENABLED: 'false',
        MONITORING_GRAFANA_ADMIN_USER: 'admin',
        MONITORING_GRAFANA_ADMIN_PASSWORD: '', // Empty is ok when disabled
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
      });
      expect(isValid).toBe(true);
    });
  });

  it('handles multiple validation errors', () => {
    const { result } = renderHook(() => useValidation());

    act(() => {
      const isValid = result.current.validateStep('security', {
        SECURITY_BASICAUTH_REALM: '', // Empty
        SECURITY_LITELLM_MASTER_KEY: 'short', // Too short
        SECURITY_LITELLM_SALT_KEY: 'short', // Too short
        SECURITY_SEARXNG_SECRET_KEY: 'short', // Too short
        SECURITY_MANAGER_API_KEY: 'short', // Too short
        POSTGRES_PASSWORD: 'short', // Too short
      });
      expect(isValid).toBe(false);
    });

    // Multiple fields should have errors
    expect(Object.keys(result.current.errors).length).toBeGreaterThan(1);
  });

  it('clears previous errors on successful validation', () => {
    const { result } = renderHook(() => useValidation());

    // First, create some errors
    act(() => {
      result.current.validateStep('domain', { DOMAIN_HOST_WEBUI: '' });
    });
    expect(result.current.hasErrors).toBe(true);

    // Then validate successfully
    act(() => {
      result.current.validateStep('domain', { DOMAIN_HOST_WEBUI: 'valid.localhost' });
    });
    expect(result.current.hasErrors).toBe(false);
  });
});
