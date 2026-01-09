import { useCallback, useEffect, useState } from 'react';
import { tasksApi } from '../../../api/tasks';
import type { TaskRun, TaskRunLog } from '../types';

export function useTaskRuns(taskId: string | null) {
  const [runs, setRuns] = useState<TaskRun[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadRuns = useCallback(async () => {
    if (!taskId) {
      setRuns([]);
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);
    try {
      const data = await tasksApi.getTaskRuns(taskId);
      setRuns(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load task runs');
      setRuns([]);
    } finally {
      setLoading(false);
    }
  }, [taskId]);

  useEffect(() => {
    loadRuns();
  }, [loadRuns]);

  const getRun = useCallback(
    async (runId: string): Promise<TaskRun> => {
      if (!taskId) {
        throw new Error('No task ID provided');
      }
      return tasksApi.getTaskRun(taskId, runId);
    },
    [taskId]
  );

  const getRunLogs = useCallback(
    async (runId: string): Promise<TaskRunLog[]> => {
      if (!taskId) {
        throw new Error('No task ID provided');
      }
      return tasksApi.getTaskRunLogs(taskId, runId);
    },
    [taskId]
  );

  const refetch = useCallback(() => {
    loadRuns();
  }, [loadRuns]);

  return {
    runs,
    loading,
    error,
    getRun,
    getRunLogs,
    refetch,
  };
}
