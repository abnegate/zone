import { useCallback, useEffect, useState } from 'react';
import { agentsApi } from '../../../api/agents';
import type { Agent, AgentStatus } from './schemas';
import type { Attempt } from './types';

export type AgentStatuses = Partial<Record<Agent, AgentStatus>>;
export type Attempts = Partial<Record<Agent, Attempt>>;

interface Loaded {
  organizationId: string;
  statuses: AgentStatuses;
  attempts: Attempts;
}

interface Failed {
  organizationId: string;
  message: string;
}

const none: AgentStatuses = {};
const idle: Attempts = {};

export function useAgentStatuses(
  organizationId: string | null,
  enabled: boolean
): {
  statuses: AgentStatuses;
  attempts: Attempts;
  error: string | null;
  update: (status: AgentStatus) => void;
  setAttempt: (agent: Agent, attempt: Attempt | null) => void;
} {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [failed, setFailed] = useState<Failed | null>(null);

  useEffect(() => {
    if (!enabled || !organizationId) return;
    let cancelled = false;
    setFailed(null);
    agentsApi.list(organizationId).then(
      (statuses) => {
        if (cancelled) return;
        setLoaded((current) => ({
          organizationId,
          statuses: Object.fromEntries(statuses.map((status) => [status.agent, status])),
          attempts: current?.organizationId === organizationId ? current.attempts : idle,
        }));
      },
      (reason: unknown) => {
        if (cancelled) return;
        setFailed({
          organizationId,
          message:
            reason instanceof Error ? reason.message : 'Failed to load coding agent sign-ins',
        });
      }
    );
    return () => {
      cancelled = true;
    };
  }, [organizationId, enabled]);

  const update = useCallback(
    (status: AgentStatus) => {
      setLoaded((current) =>
        current?.organizationId === organizationId
          ? { ...current, statuses: { ...current.statuses, [status.agent]: status } }
          : current
      );
    },
    [organizationId]
  );

  const setAttempt = useCallback(
    (agent: Agent, attempt: Attempt | null) => {
      setLoaded((current) => {
        if (current?.organizationId !== organizationId) return current;
        const attempts = { ...current.attempts };
        if (attempt) {
          attempts[agent] = attempt;
        } else {
          delete attempts[agent];
        }
        return { ...current, attempts };
      });
    },
    [organizationId]
  );

  const current = loaded?.organizationId === organizationId ? loaded : null;
  return {
    statuses: current?.statuses ?? none,
    attempts: current?.attempts ?? idle,
    error: failed?.organizationId === organizationId ? failed.message : null,
    update,
    setAttempt,
  };
}
