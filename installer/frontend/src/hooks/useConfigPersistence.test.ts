import { act, renderHook, waitFor } from '@testing-library/react';
import type { InstallerConfig } from '../types';
import { useConfigPersistence } from './useConfigPersistence';

// Mock crypto utils
jest.mock('../utils/crypto', () => ({
  loadConfig: jest.fn(),
  saveConfig: jest.fn(),
  clearConfig: jest.fn(),
}));

import { clearConfig, loadConfig, saveConfig } from '../utils/crypto';

const mockLoadConfig = loadConfig as jest.MockedFunction<typeof loadConfig>;
const mockSaveConfig = saveConfig as jest.MockedFunction<typeof saveConfig>;
const mockClearConfig = clearConfig as jest.MockedFunction<typeof clearConfig>;

const defaultConfig: InstallerConfig = {
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
};

describe('useConfigPersistence', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    jest.useFakeTimers();
    mockLoadConfig.mockResolvedValue(null);
  });

  afterEach(() => {
    jest.useRealTimers();
  });

  it('loads stored config on mount', async () => {
    const storedConfig = { DOMAIN_HOST_WEBUI: 'stored.localhost' };
    mockLoadConfig.mockResolvedValue(storedConfig);
    const setConfig = jest.fn();

    renderHook(() => useConfigPersistence(defaultConfig, setConfig, defaultConfig));

    await waitFor(() => {
      expect(mockLoadConfig).toHaveBeenCalled();
    });

    await waitFor(() => {
      expect(setConfig).toHaveBeenCalledWith({
        ...defaultConfig,
        DOMAIN_HOST_WEBUI: 'stored.localhost',
      });
    });
  });

  it('does not load config if no stored config exists', async () => {
    mockLoadConfig.mockResolvedValue(null);
    const setConfig = jest.fn();

    renderHook(() => useConfigPersistence(defaultConfig, setConfig, defaultConfig));

    await waitFor(() => {
      expect(mockLoadConfig).toHaveBeenCalled();
    });

    expect(setConfig).not.toHaveBeenCalled();
  });

  it('does not load config if stored config is empty', async () => {
    mockLoadConfig.mockResolvedValue({});
    const setConfig = jest.fn();

    renderHook(() => useConfigPersistence(defaultConfig, setConfig, defaultConfig));

    await waitFor(() => {
      expect(mockLoadConfig).toHaveBeenCalled();
    });

    expect(setConfig).not.toHaveBeenCalled();
  });

  it('only loads config once on mount', async () => {
    const storedConfig = { DOMAIN_HOST_WEBUI: 'stored.localhost' };
    mockLoadConfig.mockResolvedValue(storedConfig);
    const setConfig = jest.fn();

    const { rerender } = renderHook(() =>
      useConfigPersistence(defaultConfig, setConfig, defaultConfig)
    );

    await waitFor(() => {
      expect(mockLoadConfig).toHaveBeenCalledTimes(1);
    });

    rerender();

    expect(mockLoadConfig).toHaveBeenCalledTimes(1);
  });

  it('auto-saves config after debounce', async () => {
    const setConfig = jest.fn();
    const newConfig = { ...defaultConfig, DOMAIN_HOST_WEBUI: 'new.localhost' };

    const { rerender } = renderHook(
      ({ config }) => useConfigPersistence(config, setConfig, defaultConfig),
      { initialProps: { config: defaultConfig } }
    );

    // First render is skipped (initial mount)
    rerender({ config: newConfig });

    // Wait for debounce
    act(() => {
      jest.advanceTimersByTime(500);
    });

    expect(mockSaveConfig).toHaveBeenCalledWith(newConfig);
  });

  it('does not save on initial mount', async () => {
    const setConfig = jest.fn();

    renderHook(() => useConfigPersistence(defaultConfig, setConfig, defaultConfig));

    act(() => {
      jest.advanceTimersByTime(1000);
    });

    expect(mockSaveConfig).not.toHaveBeenCalled();
  });

  it('debounces save calls', async () => {
    const setConfig = jest.fn();

    const { rerender } = renderHook(
      ({ config }) => useConfigPersistence(config, setConfig, defaultConfig),
      { initialProps: { config: defaultConfig } }
    );

    // Make multiple changes quickly
    rerender({ config: { ...defaultConfig, DOMAIN_HOST_WEBUI: 'change1' } });
    act(() => {
      jest.advanceTimersByTime(100);
    });

    rerender({ config: { ...defaultConfig, DOMAIN_HOST_WEBUI: 'change2' } });
    act(() => {
      jest.advanceTimersByTime(100);
    });

    rerender({ config: { ...defaultConfig, DOMAIN_HOST_WEBUI: 'change3' } });
    act(() => {
      jest.advanceTimersByTime(500);
    });

    // Should only save the final config
    expect(mockSaveConfig).toHaveBeenCalledTimes(1);
    expect(mockSaveConfig).toHaveBeenCalledWith({
      ...defaultConfig,
      DOMAIN_HOST_WEBUI: 'change3',
    });
  });

  it('returns resetConfig function', () => {
    const setConfig = jest.fn();

    const { result } = renderHook(() =>
      useConfigPersistence(defaultConfig, setConfig, defaultConfig)
    );

    expect(typeof result.current.resetConfig).toBe('function');
  });

  it('resetConfig clears storage and resets to default', () => {
    const setConfig = jest.fn();

    const { result } = renderHook(() =>
      useConfigPersistence(defaultConfig, setConfig, defaultConfig)
    );

    act(() => {
      result.current.resetConfig();
    });

    expect(mockClearConfig).toHaveBeenCalled();
    expect(setConfig).toHaveBeenCalledWith(defaultConfig);
  });
});
