export type AiProvider = 'self_hosted' | 'openai' | 'anthropic' | 'bedrock';

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

  // AI Provider
  AI_PROVIDER: AiProvider;
  AI_LITELLM_HOST: string;
  AI_LITELLM_KEY: string;
  AI_OPENAI_API_KEY: string;
  AI_OPENAI_BASE_URL: string;
  AI_ANTHROPIC_API_KEY: string;
  AI_ANTHROPIC_BASE_URL: string;
  AI_BEDROCK_REGION: string;
  AI_BEDROCK_ACCESS_KEY: string;
  AI_BEDROCK_SECRET_KEY: string;
  AI_BEDROCK_USE_IAM_ROLE: string;
  AI_MODEL_FAST: string;
  AI_MODEL_REASONING: string;
  AI_MODEL_EMBEDDING: string;

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
  title: string;
  description: string;
  sidebarDescription: string;
  number: number;
}

export const STEPS: Step[] = [
  {
    id: 'domain',
    label: 'Domain',
    title: 'Domain Configuration',
    description: 'Configure hostnames for your services',
    sidebarDescription: 'Configure your domain settings',
    number: 1,
  },
  {
    id: 'security',
    label: 'Security',
    title: 'Security',
    description: 'Configure authentication and generate secure keys',
    sidebarDescription: 'Set up authentication and keys',
    number: 2,
  },
  {
    id: 'models',
    label: 'Models',
    title: 'AI Provider Configuration',
    description: 'Choose your AI provider and configure models',
    sidebarDescription: 'Choose your AI models',
    number: 3,
  },
  {
    id: 'interface',
    label: 'Interface',
    title: 'Interface Settings',
    description: 'Configure the web interface',
    sidebarDescription: 'Customize the web interface',
    number: 4,
  },
  {
    id: 'search',
    label: 'Search',
    title: 'Web Search',
    description: 'Configure search integration',
    sidebarDescription: 'Configure search settings',
    number: 5,
  },
  {
    id: 'vpn',
    label: 'VPN',
    title: 'VPN Configuration',
    description: 'Optional: Configure VPN for private web search',
    sidebarDescription: 'Set up VPN connection',
    number: 6,
  },
  {
    id: 'advanced',
    label: 'Advanced',
    title: 'Advanced Settings',
    description: 'Performance tuning and system configuration',
    sidebarDescription: 'Fine-tune advanced options',
    number: 7,
  },
];

export interface InstallProgress {
  progress: number;
  status: string;
  id?: string;
  state?: 'in-progress' | 'success' | 'error' | 'retry' | 'normal';
  complete?: boolean;
  error?: boolean;
}
