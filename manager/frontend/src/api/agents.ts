import {
  type Agent,
  AgentFailureSchema,
  type AgentLogin,
  AgentLoginSchema,
  type AgentStatus,
  AgentStatusesSchema,
  AgentStatusSchema,
  type ClaudeScope,
  type SignInFlow,
} from '../features/settings/ai/schemas';
import { parse } from '../validation';
import { AgentRequestError } from './AgentRequestError';
import { API_BASE, client } from './client';

const CODE_AGENT: Agent = 'claude';

export interface StartRequest {
  scope?: ClaudeScope;
  flow?: SignInFlow;
}

function agentsUrl(organizationId: string): string {
  return `${API_BASE}/api/organizations/${encodeURIComponent(organizationId)}/agents`;
}

async function failure(response: Response, fallback: string): Promise<AgentRequestError> {
  const body = AgentFailureSchema.safeParse(await response.json().catch(() => null));
  const reason = body.success ? body.data.error : '';
  return new AgentRequestError(
    reason || `${fallback}: ${response.status}`,
    response.status,
    body.success ? body.data.kind : undefined
  );
}

async function send(url: string, init: RequestInit, fallback: string): Promise<Response> {
  const response = await fetch(url, { ...init, headers: client.getHeaders() });
  if (!response.ok) {
    throw await failure(response, fallback);
  }
  return response;
}

export const agentsApi = {
  async list(organizationId: string): Promise<AgentStatus[]> {
    const response = await send(
      agentsUrl(organizationId),
      {},
      'Failed to load coding agent sign-ins'
    );
    return parse(AgentStatusesSchema, await response.json()).agents;
  },

  /** One agent's status, with why the caller's own sign-in `attempt` failed once it has. */
  async get(organizationId: string, agent: Agent, attempt?: string): Promise<AgentStatus> {
    const query = attempt ? `?${new URLSearchParams({ attempt })}` : '';
    const response = await send(
      `${agentsUrl(organizationId)}/${agent}${query}`,
      {},
      `Failed to load the ${agent} sign-in`
    );
    return parse(AgentStatusSchema, await response.json());
  },

  async start(
    organizationId: string,
    agent: Agent,
    request: StartRequest = {}
  ): Promise<AgentLogin> {
    const response = await send(
      `${agentsUrl(organizationId)}/${agent}/login`,
      { method: 'POST', body: JSON.stringify(request) },
      `Failed to start the ${agent} sign-in`
    );
    return parse(AgentLoginSchema, await response.json());
  },

  async submitCode(organizationId: string, code: string): Promise<AgentStatus> {
    const response = await send(
      `${agentsUrl(organizationId)}/${CODE_AGENT}/login/code`,
      { method: 'POST', body: JSON.stringify({ code }) },
      'Failed to submit the code'
    );
    return parse(AgentStatusSchema, await response.json());
  },

  /** Hands in the receipt Zone's callback sent the browser on with, finishing its sign-in. */
  async redeem(organizationId: string, receipt: string): Promise<AgentStatus> {
    const response = await send(
      `${agentsUrl(organizationId)}/${CODE_AGENT}/login/receipt`,
      { method: 'POST', body: JSON.stringify({ receipt }) },
      'Failed to finish the sign-in'
    );
    return parse(AgentStatusSchema, await response.json());
  },

  /** Ends the caller's own sign-in, wherever its code is. */
  async cancel(organizationId: string, agent: Agent): Promise<void> {
    await send(
      `${agentsUrl(organizationId)}/${agent}/login/attempt`,
      { method: 'DELETE' },
      `Failed to cancel the ${agent} sign-in`
    );
  },

  async signOut(organizationId: string, agent: Agent): Promise<void> {
    await send(
      `${agentsUrl(organizationId)}/${agent}/login`,
      { method: 'DELETE' },
      `Failed to sign out of ${agent}`
    );
  },
};
