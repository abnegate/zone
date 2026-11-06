import { useEffect, useState } from 'react';
import { client } from '../../../api/client';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { WorkspaceRole } from '../../../types';
import { useAuth } from '../../auth/context';

interface ResolvedRole {
  workspaceId: string;
  role: WorkspaceRole | undefined;
}

export interface WorkspaceRoleState {
  role: WorkspaceRole | undefined;
  resolving: boolean;
}

export function useWorkspaceRole(): WorkspaceRoleState {
  const { currentWorkspace } = useWorkspace();
  const userId = useAuth().user?.id;
  const workspaceId = currentWorkspace?.id;
  const [state, setState] = useState<ResolvedRole | null>(null);

  useEffect(() => {
    if (workspaceId === undefined || userId === undefined) {
      return;
    }
    let cancelled = false;
    const settle = (role: WorkspaceRole | undefined) => {
      if (!cancelled) {
        setState({ workspaceId, role });
      }
    };
    client
      .getWorkspaceMembers(workspaceId)
      .then(({ members }) => settle(members.find((member) => member.user_id === userId)?.role))
      .catch(() => settle(undefined));
    return () => {
      cancelled = true;
    };
  }, [workspaceId, userId]);

  if (workspaceId === undefined) {
    return { role: undefined, resolving: false };
  }

  const resolving = userId !== undefined && state?.workspaceId !== workspaceId;
  const role = state?.workspaceId === workspaceId ? state.role : undefined;
  return { role, resolving };
}
