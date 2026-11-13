export type McpTransport = 'stdio' | 'http' | 'sse';
export type McpServerStatus = 'connected' | 'failed' | 'disabled' | 'connecting';
export type McpServerSource = 'organization' | 'instance';

export interface McpSecretField {
  name: string;
  has_value: boolean;
}

export interface McpServer {
  id: string | null;
  name: string;
  source: McpServerSource;
  transport: McpTransport;
  command: string | null;
  arguments: string[];
  url: string | null;
  tools: string[] | null;
  enabled: boolean;
  environment: McpSecretField[];
  headers: McpSecretField[];
  status: McpServerStatus;
  error: string | null;
  available_tools: string[];
  created_at: string | null;
  updated_at: string | null;
}

export interface McpServerList {
  commands: boolean;
  servers: McpServer[];
}

export interface WorkspaceMcpServer extends McpServer {
  workspace_enabled: boolean | null;
  effective: boolean;
}

export interface WorkspaceMcpServerList {
  servers: WorkspaceMcpServer[];
}

export interface CreateMcpServerRequest {
  name: string;
  transport: McpTransport;
  command?: string;
  arguments?: string[];
  url?: string;
  tools?: string[] | null;
  enabled?: boolean;
  environment?: Record<string, string>;
  headers?: Record<string, string>;
}

export interface UpdateMcpServerRequest {
  name?: string;
  command?: string;
  arguments?: string[];
  url?: string;
  tools?: string[] | null;
  enabled?: boolean;
  environment?: Record<string, string | null>;
  headers?: Record<string, string | null>;
}

export interface UpdateWorkspaceMcpServersRequest {
  servers: Record<string, boolean | null>;
}

/** One server a connected provider has configured, which an admin may import. */
export interface ProviderMcpServer {
  name: string;
  transport: McpTransport;
  command: string | null;
  arguments: string[];
  url: string | null;
  tools: string[] | null;
  enabled: boolean;
  environment: McpSecretField[];
  headers: McpSecretField[];
  /** Keys the provider's entry holds that an import does not carry. */
  ignored: string[];
  /** Why this server cannot be imported as it stands, or null when it can. */
  refusal: string | null;
}

export interface ProviderMcpServers {
  provider: string;
  label: string;
  servers: ProviderMcpServer[];
  errors: string[];
}

export interface ProviderMcpServerList {
  commands: boolean;
  providers: ProviderMcpServers[];
}

export interface ImportMcpServersRequest {
  provider: string;
  servers: string[];
}

export interface ImportedMcpServers {
  created: McpServer[];
  failures: { name: string; message: string }[];
}
