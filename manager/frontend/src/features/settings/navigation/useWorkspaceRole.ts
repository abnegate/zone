import { useEffect, useState } from 'react';
import { client } from '../../../api/client';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { WorkspaceMember, WorkspaceRole } from '../../../types';
import { useAuth } from '../../auth/context';

export interface WorkspaceRoleState {
  role: WorkspaceRole | undefined;
  permissions?: readonly string[] | undefined;
  resolving: boolean;
}

interface Standing {
  role: WorkspaceRole | undefined;
  permissions: readonly string[] | undefined;
}

interface ResolvedStanding extends Standing {
  workspaceId: string;
}

const UNKNOWN: Standing = { role: undefined, permissions: undefined };

function standingOf(member: WorkspaceMember | undefined): Standing {
  return member === undefined ? UNKNOWN : { role: member.role, permissions: member.permissions };
}

export function useWorkspaceRole(): WorkspaceRoleState {
  const { currentWorkspace } = useWorkspace();
  const userId = useAuth().user?.id;
  const workspaceId = currentWorkspace?.id;
  const [state, setState] = useState<ResolvedStanding | null>(null);

  useEffect(() => {
    if (workspaceId === undefined || userId === undefined) {
      return;
    }
    let cancelled = false;
    const settle = (standing: Standing) => {
      if (!cancelled) {
        setState({ workspaceId, ...standing });
      }
    };
    client
      .getWorkspaceMembers(workspaceId)
      .then(({ members }) =>
        settle(standingOf(members.find((member) => member.user_id === userId)))
      )
      .catch(() => settle(UNKNOWN));
    return () => {
      cancelled = true;
    };
  }, [workspaceId, userId]);

  if (workspaceId === undefined) {
    return { ...UNKNOWN, resolving: false };
  }

  const resolving = userId !== undefined && state?.workspaceId !== workspaceId;
  const standing = state?.workspaceId === workspaceId ? state : UNKNOWN;
  return { role: standing.role, permissions: standing.permissions, resolving };
}
