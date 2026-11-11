import { useQuery } from '@tanstack/react-query';
import { tasksApi } from '../../../api/tasks';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { titlePoll } from '../title';
import type { Task } from '../types';
import { LOAD_FAILED, tasksKey } from './useTasks';

export interface WorkspaceTasks {
  tasks: readonly Task[] | undefined;
  error: string | null;
}

export function useWorkspaceTasks(enabled: boolean): WorkspaceTasks {
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const { data, error } = useQuery({
    queryKey: tasksKey(workspaceId),
    queryFn: () => (workspaceId ? tasksApi.getTasks(workspaceId) : []),
    enabled: enabled && !!workspaceId,
    refetchInterval: (query) => titlePoll(query.state.data),
  });

  return {
    tasks: data,
    error: error instanceof Error ? error.message : error ? LOAD_FAILED : null,
  };
}
