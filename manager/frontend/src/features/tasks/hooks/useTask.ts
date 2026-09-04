import { useCallback, useEffect, useState } from 'react';
import { tasksApi } from '../../../api/tasks';
import type { Task, TaskRun, UpdateTaskRequest } from '../types';

export function useTask(id: string | null) {
  const [task, setTask] = useState<Task | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadTask = useCallback(async () => {
    if (!id) {
      setTask(null);
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);
    try {
      const data = await tasksApi.getTask(id);
      setTask(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load task');
      setTask(null);
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    loadTask();
  }, [loadTask]);

  const updateTask = useCallback(
    async (request: UpdateTaskRequest): Promise<Task> => {
      if (!id) {
        throw new Error('No task ID provided');
      }
      const updatedTask = await tasksApi.updateTask(id, request);
      setTask(updatedTask);
      return updatedTask;
    },
    [id]
  );

  const deleteTask = useCallback(async (): Promise<void> => {
    if (!id) {
      throw new Error('No task ID provided');
    }
    await tasksApi.deleteTask(id);
    setTask(null);
  }, [id]);

  const runTask = useCallback(async (): Promise<TaskRun> => {
    if (!id) {
      throw new Error('No task ID provided');
    }
    return tasksApi.runTask(id);
  }, [id]);

  const refetch = useCallback(() => {
    loadTask();
  }, [loadTask]);

  return {
    task,
    loading,
    error,
    updateTask,
    deleteTask,
    runTask,
    refetch,
  };
}
