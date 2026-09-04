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

// Models step schema with conditional validation based on provider
export const ModelsSchema = z
  .object({
    AI_PROVIDER: z.enum(['self_hosted', 'openai', 'anthropic', 'bedrock']),
    // Self-hosted settings
    AI_LITELLM_HOST: z.string(),
    AI_LITELLM_KEY: z.string(),
    // OpenAI settings
    AI_OPENAI_API_KEY: z.string(),
    AI_OPENAI_BASE_URL: z.string(),
    // Anthropic settings
    AI_ANTHROPIC_API_KEY: z.string(),
    AI_ANTHROPIC_BASE_URL: z.string(),
    // Bedrock settings
    AI_BEDROCK_REGION: z.string(),
    AI_BEDROCK_ACCESS_KEY: z.string(),
    AI_BEDROCK_SECRET_KEY: z.string(),
    AI_BEDROCK_USE_IAM_ROLE: z.enum(['true', 'false']),
    // Model selections
    AI_MODEL_FAST: z.string().min(1, 'Fast model is required'),
    AI_MODEL_REASONING: z.string().min(1, 'Reasoning model is required'),
    AI_MODEL_EMBEDDING: z.string(),
  })
  .superRefine((data, ctx) => {
    // Self-hosted validation
    if (data.AI_PROVIDER === 'self_hosted') {
      if (!data.AI_LITELLM_HOST) {
        ctx.addIssue({
          code: z.ZodIssueCode.custom,
          message: 'LiteLLM host is required',
          path: ['AI_LITELLM_HOST'],
        });
      }
    }
    // OpenAI validation
    if (data.AI_PROVIDER === 'openai') {
      if (!data.AI_OPENAI_API_KEY) {
        ctx.addIssue({
          code: z.ZodIssueCode.custom,
          message: 'OpenAI API key is required',
          path: ['AI_OPENAI_API_KEY'],
        });
      }
    }
    // Anthropic validation
    if (data.AI_PROVIDER === 'anthropic') {
      if (!data.AI_ANTHROPIC_API_KEY) {
        ctx.addIssue({
          code: z.ZodIssueCode.custom,
          message: 'Anthropic API key is required',
          path: ['AI_ANTHROPIC_API_KEY'],
        });
      }
      if (!data.AI_MODEL_EMBEDDING) {
        ctx.addIssue({
          code: z.ZodIssueCode.custom,
          message: 'Embedding model is required (use an external provider)',
          path: ['AI_MODEL_EMBEDDING'],
        });
      }
    }
    // Bedrock validation
    if (data.AI_PROVIDER === 'bedrock') {
      if (!data.AI_BEDROCK_REGION) {
        ctx.addIssue({
          code: z.ZodIssueCode.custom,
          message: 'AWS region is required',
          path: ['AI_BEDROCK_REGION'],
        });
      }
      if (data.AI_BEDROCK_USE_IAM_ROLE !== 'true') {
        if (!data.AI_BEDROCK_ACCESS_KEY) {
          ctx.addIssue({
            code: z.ZodIssueCode.custom,
            message: 'AWS access key is required when not using IAM role',
            path: ['AI_BEDROCK_ACCESS_KEY'],
          });
        }
        if (!data.AI_BEDROCK_SECRET_KEY) {
          ctx.addIssue({
            code: z.ZodIssueCode.custom,
            message: 'AWS secret key is required when not using IAM role',
            path: ['AI_BEDROCK_SECRET_KEY'],
          });
        }
      }
    }
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
  .superRefine((data, ctx) => {
    if (data.MONITORING_ENABLED === 'true' && data.MONITORING_GRAFANA_ADMIN_PASSWORD.length < 8) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: 'Grafana password must be at least 8 characters when monitoring is enabled',
        path: ['MONITORING_GRAFANA_ADMIN_PASSWORD'],
      });
    }
    if (data.ALERT_ENABLED === 'true' && data.ALERT_SMTP_HOST.length === 0) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: 'SMTP host is required when alerting is enabled',
        path: ['ALERT_SMTP_HOST'],
      });
    }
    if (data.ALERT_ENABLED === 'true' && data.ALERT_EMAIL_RECIPIENTS.length === 0) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: 'At least one alert recipient is required when alerting is enabled',
        path: ['ALERT_EMAIL_RECIPIENTS'],
      });
    }
  });

// Map step IDs to schemas
export const StepSchemas = {
  domain: DomainSchema,
  security: SecuritySchema,
  models: ModelsSchema,
  search: SearchSchema,
  vpn: VPNSchema,
  advanced: AdvancedSchema,
} as const;

export type StepSchemaKey = keyof typeof StepSchemas;
