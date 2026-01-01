import { renderHook, act } from '@testing-library/react';
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
});
