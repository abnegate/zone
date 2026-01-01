export interface InstallerConfig {
  // Domain
  DOMAIN_HOST_WEBUI: string;

  // Security
  SECURITY_BASICAUTH_REALM: string;
  SECURITY_LITELLM_MASTER_KEY: string;
  SECURITY_LITELLM_SALT_KEY: string;
  SECURITY_SEARXNG_SECRET_KEY: string;
  SECURITY_MANAGER_API_KEY: string;
  POSTGRES_PASSWORD: string;
  SECURITY_HTTP_REDIRECT: string;
  SECURITY_GENERATE_CERTIFICATE: string;

  // Models
  OLLAMA_MODEL_FAST: string;
  OLLAMA_MODEL_REASON: string;
  OLLAMA_MODEL_EMBED: string;

  // Interface
  WEBUI_AUTH: string;
  WEBUI_ENABLE_SIGNUP: string;
  WEBUI_DEFAULT_LOCALE: string;

  // Search
  SEARCH_ENABLE_WEB_SEARCH: string;
  SEARCH_RESULT_COUNT: string;
  SEARCH_CONCURRENT_REQUESTS: string;
  SEARCH_SEARXNG_INSTANCE_NAME: string;

  // VPN
  VPN_SERVICE_PROVIDER: string;
  VPN_TYPE: string;
  VPN_OPENVPN_USER: string;
  VPN_OPENVPN_PASSWORD: string;
  VPN_WIREGUARD_PRIVATE_KEY: string;
  VPN_WIREGUARD_ADDRESSES: string;
  VPN_SERVER_COUNTRIES: string;
  VPN_SERVER_CITIES: string;
  VPN_SERVER_REGIONS: string;

  // Monitoring
  MONITORING_ENABLED: string;
  MONITORING_GRAFANA_ADMIN_USER: string;
  MONITORING_GRAFANA_ADMIN_PASSWORD: string;
  MONITORING_RETENTION_TIME: string;

  // Alerting
  ALERT_ENABLED: string;
  ALERT_EMAIL_RECIPIENTS: string;
  ALERT_SMTP_HOST: string;
  ALERT_SMTP_PORT: string;
  ALERT_SMTP_USER: string;
  ALERT_SMTP_PASSWORD: string;
  ALERT_SMTP_FROM_ADDRESS: string;
  ALERT_SMTP_FROM_NAME: string;

  // Advanced
  ADVANCED_LITELLM_WORKERS: string;
  ADVANCED_LITELLM_REQUEST_TIMEOUT: string;
  ADVANCED_TZ: string;
  ADVANCED_ACME_EMAIL: string;

  // Derived/computed values (not in .env but used internally)
  SECURITY_BASIC_AUTH_USERS_FILE: string;
  OLLAMA_HOST: string;
  OLLAMA_KEEP_ALIVE: string;
  OLLAMA_MAX_LOADED_MODELS: string;
}

export type StepId = 'domain' | 'security' | 'models' | 'interface' | 'search' | 'vpn' | 'advanced';

export interface Step {
  id: StepId;
  label: string;
  number: number;
}

export const STEPS: Step[] = [
  { id: 'domain', label: 'Domain', number: 1 },
  { id: 'security', label: 'Security', number: 2 },
  { id: 'models', label: 'Models', number: 3 },
  { id: 'interface', label: 'Interface', number: 4 },
  { id: 'search', label: 'Search', number: 5 },
  { id: 'vpn', label: 'VPN', number: 6 },
  { id: 'advanced', label: 'Advanced', number: 7 },
];

export interface InstallProgress {
  progress: number;
  message: string;
  complete?: boolean;
  error?: boolean;
}
