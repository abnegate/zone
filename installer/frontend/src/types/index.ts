export interface InstallerConfig {
  // Domain
  WEBUI_HOSTNAME: string;

  // Security
  SECURITY_AUTH_REALM: string;
  LITELLM_MASTER_KEY: string;
  LITELLM_SALT_KEY: string;
  SEARXNG_SECRET_KEY: string;

  // Models
  OLLAMA_FAST_MODEL: string;
  OLLAMA_REASONING_MODEL: string;
  OLLAMA_EMBEDDING_MODEL: string;

  // Interface
  WEBUI_AUTH: string;
  WEBUI_ENABLE_SIGNUP: string;
  WEBUI_DEFAULT_LOCALE: string;

  // Search
  ENABLE_RAG_WEB_SEARCH: string;
  RAG_WEB_SEARCH_RESULT_COUNT: string;
  RAG_WEB_SEARCH_CONCURRENT_REQUESTS: string;
  SEARXNG_INSTANCE_NAME: string;

  // VPN
  ENABLE_VPN: string;
  VPN_PROVIDER: string;
  VPN_PROTOCOL: string;
  OPENVPN_USER: string;
  OPENVPN_PASS: string;
  WIREGUARD_PRIVATE_KEY: string;
  WIREGUARD_ADDRESS: string;

  // Advanced - Monitoring
  ENABLE_MONITORING: string;
  GF_SECURITY_ADMIN_USER: string;
  GF_SECURITY_ADMIN_PASSWORD: string;
  METRICS_RETENTION: string;

  // Advanced - Performance
  WORKERS: string;
  REQUEST_TIMEOUT: string;
  TZ: string;
  ACME_EMAIL: string;

  // Derived/computed values
  SECURITY_BASIC_AUTH_USERS_FILE: string;
  OLLAMA_HOST: string;
  OLLAMA_KEEP_ALIVE: string;
  OLLAMA_MAX_LOADED_MODELS: string;
  WEBUI_OPENAI_API_BASE_URL: string;
  WEBUI_OPENAI_API_KEY: string;
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
