import { z } from 'zod';

// Hostname validation patterns
const hostnameRegex =
  /^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$/;

// Domain step schema
export const DomainSchema = z.object({
  DOMAIN_HOST_WEBUI: z
    .string()
    .min(1, 'Hostname is required')
    .refine(
      (val) => hostnameRegex.test(val),
      'Invalid hostname format (e.g., webui.localhost or app.example.com)'
    ),
});

// Security step schema
export const SecuritySchema = z.object({
  SECURITY_BASICAUTH_REALM: z.string().min(1, 'Auth realm is required'),
  SECURITY_LITELLM_MASTER_KEY: z.string().min(16, 'Master key must be at least 16 characters'),
  SECURITY_LITELLM_SALT_KEY: z.string().min(16, 'Salt key must be at least 16 characters'),
  SECURITY_SEARXNG_SECRET_KEY: z.string().min(16, 'Secret key must be at least 16 characters'),
  SECURITY_MANAGER_API_KEY: z.string().min(16, 'Manager API key must be at least 16 characters'),
  POSTGRES_PASSWORD: z.string().min(16, 'PostgreSQL password must be at least 16 characters'),
  SECURITY_HTTP_REDIRECT: z.enum(['true', 'false']),
  SECURITY_GENERATE_CERTIFICATE: z.enum(['true', 'false']),
});

// Models step schema
export const ModelsSchema = z.object({
  OLLAMA_MODEL_FAST: z.string().min(1, 'Fast model is required'),
  OLLAMA_MODEL_REASON: z.string().min(1, 'Reasoning model is required'),
  OLLAMA_MODEL_EMBED: z.string().min(1, 'Embedding model is required'),
});

// Interface step schema
export const InterfaceSchema = z.object({
  WEBUI_AUTH: z.enum(['true', 'false']),
  WEBUI_ENABLE_SIGNUP: z.enum(['true', 'false']),
  WEBUI_DEFAULT_LOCALE: z.string().min(1, 'Locale is required'),
});

// Search step schema
export const SearchSchema = z.object({
  SEARCH_ENABLE_WEB_SEARCH: z.enum(['true', 'false']),
  SEARCH_RESULT_COUNT: z
    .string()
    .regex(/^\d+$/, 'Must be a number')
    .refine((val) => {
      const num = Number.parseInt(val, 10);
      return num >= 1 && num <= 20;
    }, 'Must be between 1 and 20'),
  SEARCH_CONCURRENT_REQUESTS: z
    .string()
    .regex(/^\d+$/, 'Must be a number')
    .refine((val) => {
      const num = Number.parseInt(val, 10);
      return num >= 1 && num <= 32;
    }, 'Must be between 1 and 32'),
  SEARCH_SEARXNG_INSTANCE_NAME: z.string().min(1, 'Instance name is required'),
});

// VPN step schema
export const VPNSchema = z.object({
  VPN_SERVICE_PROVIDER: z.string(),
  VPN_TYPE: z.string(),
  VPN_OPENVPN_USER: z.string(),
  VPN_OPENVPN_PASSWORD: z.string(),
  VPN_WIREGUARD_PRIVATE_KEY: z.string(),
  VPN_WIREGUARD_ADDRESSES: z.string(),
  VPN_SERVER_COUNTRIES: z.string(),
  VPN_SERVER_CITIES: z.string(),
  VPN_SERVER_REGIONS: z.string(),
});

// Advanced step schema
export const AdvancedSchema = z
  .object({
    MONITORING_ENABLED: z.enum(['true', 'false']),
    MONITORING_GRAFANA_ADMIN_USER: z.string(),
    MONITORING_GRAFANA_ADMIN_PASSWORD: z.string(),
    MONITORING_RETENTION_TIME: z.string(),
    ALERT_ENABLED: z.enum(['true', 'false']),
    ALERT_EMAIL_RECIPIENTS: z.string(),
    ALERT_SMTP_HOST: z.string(),
    ALERT_SMTP_PORT: z.string(),
    ALERT_SMTP_USER: z.string(),
    ALERT_SMTP_PASSWORD: z.string(),
    ALERT_SMTP_FROM_ADDRESS: z.string(),
    ALERT_SMTP_FROM_NAME: z.string(),
    ADVANCED_LITELLM_WORKERS: z.string().regex(/^[1-9]\d*$/, 'Must be a positive number'),
    ADVANCED_LITELLM_REQUEST_TIMEOUT: z.string().regex(/^\d+$/, 'Must be a number'),
    ADVANCED_TZ: z.string().min(1, 'Timezone is required'),
    ADVANCED_ACME_EMAIL: z.string().min(1, 'Email is required').email('Invalid email address'),
  })
  .refine(
    (data) => {
      if (data.MONITORING_ENABLED === 'true') {
        return data.MONITORING_GRAFANA_ADMIN_PASSWORD.length >= 8;
      }
      return true;
    },
    {
      message: 'Grafana password must be at least 8 characters when monitoring is enabled',
      path: ['MONITORING_GRAFANA_ADMIN_PASSWORD'],
    }
  )
  .refine(
    (data) => {
      if (data.ALERT_ENABLED === 'true') {
        return data.ALERT_SMTP_HOST.length > 0;
      }
      return true;
    },
    {
      message: 'SMTP host is required when alerting is enabled',
      path: ['ALERT_SMTP_HOST'],
    }
  )
  .refine(
    (data) => {
      if (data.ALERT_ENABLED === 'true') {
        return data.ALERT_EMAIL_RECIPIENTS.length > 0;
      }
      return true;
    },
    {
      message: 'At least one alert recipient is required when alerting is enabled',
      path: ['ALERT_EMAIL_RECIPIENTS'],
    }
  );

// Map step IDs to schemas
export const StepSchemas = {
  domain: DomainSchema,
  security: SecuritySchema,
  models: ModelsSchema,
  interface: InterfaceSchema,
  search: SearchSchema,
  vpn: VPNSchema,
  advanced: AdvancedSchema,
} as const;

export type StepSchemaKey = keyof typeof StepSchemas;
