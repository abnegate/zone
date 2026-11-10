import { z } from 'zod';
import { Source, Status, Transport } from './constants';
import type {
  McpSecretField,
  McpServer,
  McpServerList,
  McpServerSource,
  McpServerStatus,
  McpTransport,
  WorkspaceMcpServer,
  WorkspaceMcpServerList,
} from './types';

export const McpTransportSchema = z.enum(Transport) satisfies z.ZodType<McpTransport>;

export const McpServerStatusSchema = z.enum(Status) satisfies z.ZodType<McpServerStatus>;

export const McpServerSourceSchema = z.enum(Source) satisfies z.ZodType<McpServerSource>;

export const McpSecretFieldSchema = z.object({
  name: z.string(),
  has_value: z.boolean(),
}) satisfies z.ZodType<McpSecretField>;

export const McpServerSchema = z.object({
  id: z.string().nullable(),
  name: z.string(),
  source: McpServerSourceSchema,
  transport: McpTransportSchema,
  command: z.string().nullable(),
  arguments: z.array(z.string()),
  url: z.string().nullable(),
  tools: z.array(z.string()).nullable(),
  enabled: z.boolean(),
  environment: z.array(McpSecretFieldSchema),
  headers: z.array(McpSecretFieldSchema),
  status: McpServerStatusSchema,
  error: z.string().nullable(),
  available_tools: z.array(z.string()),
  created_at: z.string().nullable(),
  updated_at: z.string().nullable(),
}) satisfies z.ZodType<McpServer>;

export const McpServerListSchema = z.object({
  commands: z.boolean(),
  servers: z.array(McpServerSchema),
}) satisfies z.ZodType<McpServerList>;

export const WorkspaceMcpServerSchema = McpServerSchema.extend({
  workspace_enabled: z.boolean().nullable(),
  effective: z.boolean(),
}) satisfies z.ZodType<WorkspaceMcpServer>;

export const WorkspaceMcpServerListSchema = z.object({
  servers: z.array(WorkspaceMcpServerSchema),
}) satisfies z.ZodType<WorkspaceMcpServerList>;
