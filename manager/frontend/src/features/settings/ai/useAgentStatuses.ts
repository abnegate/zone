import { useCallback, useEffect, useState } from 'react';
import { agentsApi } from '../../../api/agents';
import type { Agent, AgentStatus } from './schemas';

export type AgentStatuses = Partial<Record<Agent, AgentStatus>>;

interface Loaded {
  organizationId: string;
  statuses: AgentStatuses;
}

interface Failed {
  organizationId: string;
  message: string;
}

const none: AgentStatuses = {};

export function useAgentStatuses(
  organizationId: string | null,
  enabled: boolean
): {
  statuses: AgentStatuses;
  error: string | null;
  update: (status: AgentStatus) => void;
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
        setLoaded({
          organizationId,
          statuses: Object.fromEntries(statuses.map((status) => [status.agent, status])),
        });
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
      if (!organizationId) return;
      setLoaded((current) => ({
        organizationId,
        statuses: {
          ...(current?.organizationId === organizationId ? current.statuses : none),
          [status.agent]: status,
        },
      }));
    },
    [organizationId]
  );

  return {
    statuses: loaded?.organizationId === organizationId ? loaded.statuses : none,
    error: failed?.organizationId === organizationId ? failed.message : null,
    update,
  };
}
