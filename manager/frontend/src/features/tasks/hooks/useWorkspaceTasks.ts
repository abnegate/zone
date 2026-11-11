import { useQuery } from '@tanstack/react-query';
import { tasksApi } from '../../../api/tasks';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import type { Task } from '../types';
import { tasksKey } from './useTasks';

export function useWorkspaceTasks(enabled: boolean): readonly Task[] | undefined {
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const { data } = useQuery({
    queryKey: tasksKey(workspaceId),
    queryFn: () => (workspaceId ? tasksApi.getTasks(workspaceId) : []),
    enabled: enabled && !!workspaceId,
  });

  return data;
}
