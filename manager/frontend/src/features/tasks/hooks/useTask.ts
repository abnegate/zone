import { useCallback, useEffect, useState } from 'react';
import { tasksApi } from '../../../api/tasks';
import { awaitsTitle, TITLE_POLL_MS } from '../title';
import type { Task, TaskRun, UpdateTaskRequest } from '../types';

export function useTask(id: string | null) {
  const [task, setTask] = useState<Task | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [missedPolls, setMissedPolls] = useState(0);

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

  useEffect(() => {
    if (!id || !task || task.id !== id || !awaitsTitle(task)) return;
    let cancelled = false;
    const timer = setTimeout(
      async () => {
        const refreshed = await tasksApi.getTask(id).catch(() => null);
        if (cancelled) return;
        if (refreshed) {
          setMissedPolls(0);
          setTask(refreshed);
        } else {
          setMissedPolls((missed) => missed + 1);
        }
      },
      TITLE_POLL_MS * (missedPolls + 1)
    );
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [id, task, missedPolls]);

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
