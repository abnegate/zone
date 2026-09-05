import {
  createContext,
  createElement,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { modelsApi } from '../../../api/models';
import { useAuth } from '../../../features/auth';
import {
  MAX_PARALLEL_PULLS,
  PULL_SUCCESS_DISMISS_MS,
  type PullChunk,
  type PullJob,
  type PullProgress,
  type Step,
} from '../types';

const STORAGE_KEY = 'zone.activePulls';
const LEGACY_STORAGE_KEY = 'zone.activePull';
const MAX_RECONNECTS = 8;

export interface PullApi {
  jobs: PullJob[];
  pulling: boolean;
  activeCount: number;
  progress: number | null;
  chunk: PullChunk | null;
  steps: Step[];
  result: { success: boolean; message: string } | null;
  model: string | null;
  minimized: boolean;
  setMinimized: (minimized: boolean) => void;
  canStart: (modelName: string) => boolean;
  pull: (modelName: string) => Promise<boolean>;
  cancel: (id?: string) => void;
  dismiss: (id: string) => void;
  reset: () => void;
}

type Finish = (success: boolean, message: string, update?: boolean) => void;

type JobRuntime = {
  id: string;
  name: string;
  generation: number;
  reconnects: number;
  reconnectTimer: ReturnType<typeof setTimeout> | null;
  socket: WebSocket | null;
  settled: boolean;
  waiters: Array<(ok: boolean) => void>;
  finish: Finish;
};

const PullContext = createContext<PullApi | null>(null);

function persist(models: string[]): void {
  try {
    if (models.length) {
      sessionStorage.setItem(STORAGE_KEY, JSON.stringify(models));
      sessionStorage.setItem(LEGACY_STORAGE_KEY, models[models.length - 1]);
    } else {
      sessionStorage.removeItem(STORAGE_KEY);
      sessionStorage.removeItem(LEGACY_STORAGE_KEY);
    }
  } catch {
    // Private browsing can reject sessionStorage writes.
  }
}

function savedPulls(): string[] {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed: unknown = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        return parsed.filter(
          (name): name is string => typeof name === 'string' && Boolean(name.trim())
        );
      }
    }
    const legacy = sessionStorage.getItem(LEGACY_STORAGE_KEY);
    return legacy ? [legacy] : [];
  } catch {
    return [];
  }
}

function applyStep(previous: Step[], name: string, message: string): Step[] {
  const existing = previous.find((step) => step.name === name);
  if (existing) {
    return previous.map((step) =>
      step.name === name ? { ...step, message, status: 'success' as const } : step
    );
  }
  return [
    ...previous.map((step) =>
      step.status === 'pending' ? { ...step, status: 'success' as const } : step
    ),
    { name, message, status: 'pending' },
  ];
}

function createJob(id: string, modelName: string): PullJob {
  return {
    id,
    modelName,
    pulling: true,
    progress: null,
    chunk: null,
    steps: [],
    result: null,
  };
}

function detachSocket(socket: WebSocket | null): void {
  if (!socket) return;
  socket.onopen = null;
  socket.onmessage = null;
  socket.onerror = null;
  socket.onclose = null;
  socket.close();
}

export function usePullState(): PullApi {
  const { isAuthenticated, accessToken } = useAuth();
  const [jobs, setJobs] = useState<PullJob[]>([]);
  const [minimized, setMinimized] = useState(false);
  const jobsRef = useRef<PullJob[]>([]);
  const pullingNames = useRef(new Set<string>());
  const runtimes = useRef(new Map<string, JobRuntime>());
  const dismissTimers = useRef(new Map<string, number>());
  const nextId = useRef(0);
  const tokenRef = useRef(accessToken);
  const resumedRef = useRef(false);
  tokenRef.current = accessToken;
  jobsRef.current = jobs;

  const persistActive = useCallback(() => {
    persist([...pullingNames.current]);
  }, []);

  const clearDismiss = useCallback((id: string) => {
    const timer = dismissTimers.current.get(id);
    if (timer !== undefined) {
      window.clearTimeout(timer);
      dismissTimers.current.delete(id);
    }
  }, []);

  const scheduleDismiss = useCallback(
    (id: string) => {
      clearDismiss(id);
      const timer = window.setTimeout(() => {
        dismissTimers.current.delete(id);
        setJobs((previous) => previous.filter((job) => job.id !== id));
      }, PULL_SUCCESS_DISMISS_MS);
      dismissTimers.current.set(id, timer);
    },
    [clearDismiss]
  );

  const clearReconnect = useCallback((runtime: JobRuntime) => {
    if (runtime.reconnectTimer) {
      clearTimeout(runtime.reconnectTimer);
      runtime.reconnectTimer = null;
    }
  }, []);

  const canStart = useCallback((modelName: string) => {
    const name = modelName.trim();
    if (!name) return false;
    if (pullingNames.current.has(name)) return false;
    return pullingNames.current.size < MAX_PARALLEL_PULLS;
  }, []);

  const finishJob = useCallback(
    (runtime: JobRuntime, success: boolean, message: string, update = true) => {
      if (runtime.settled) return;
      runtime.settled = true;
      runtime.generation += 1;
      clearReconnect(runtime);
      pullingNames.current.delete(runtime.name);
      persistActive();
      const socket = runtime.socket;
      runtime.socket = null;
      detachSocket(socket);
      runtimes.current.delete(runtime.id);
      if (update) {
        setJobs((previous) =>
          previous.map((job) =>
            job.id === runtime.id
              ? {
                  ...job,
                  pulling: false,
                  steps: job.steps.map((step) =>
                    step.status === 'pending'
                      ? { ...step, status: success ? 'success' : 'error' }
                      : step
                  ),
                  result: { success, message },
                }
              : job
          )
        );
        if (success) scheduleDismiss(runtime.id);
      }
      const waiters = runtime.waiters.splice(0);
      for (const waiter of waiters) waiter(success);
    },
    [clearReconnect, persistActive, scheduleDismiss]
  );

  const connect = useCallback(
    (runtime: JobRuntime) => {
      if (runtime.settled) return;
      const generation = runtime.generation;
      let socket: WebSocket;
      try {
        socket = modelsApi.createPullWebSocket(runtime.name);
      } catch {
        finishJob(runtime, false, 'Connection error');
        return;
      }
      if (!socket) {
        finishJob(runtime, false, 'Connection closed before installation completed');
        return;
      }

      runtime.socket = socket;
      let requested = false;

      socket.onopen = () => {
        if (generation !== runtime.generation || runtime.settled) return;
        try {
          socket.send(JSON.stringify({ type: 'auth', token: tokenRef.current }));
        } catch {
          finishJob(runtime, false, 'Connection error');
        }
      };

      socket.onmessage = (event) => {
        if (generation !== runtime.generation || runtime.settled) return;
        try {
          const data: PullProgress = JSON.parse(event.data);
          if (data.type === 'authenticated') {
            if (requested) return;
            requested = true;
            try {
              socket.send(JSON.stringify({ model: runtime.name }));
            } catch {
              finishJob(runtime, false, 'Connection error');
            }
            return;
          }

          switch (data.type) {
            case 'progress':
              setJobs((previous) =>
                previous.map((job) => {
                  if (job.id !== runtime.id) return job;
                  const chunk =
                    data.completed !== undefined && data.total !== undefined
                      ? {
                          completed: data.completed,
                          total: data.total,
                          digest: data.digest,
                        }
                      : job.chunk;
                  return {
                    ...job,
                    progress: data.percent ?? job.progress,
                    chunk,
                  };
                })
              );
              runtime.reconnects = 0;
              break;
            case 'step':
              if (data.status) {
                const status = data.status;
                const message = data.message || '';
                setJobs((previous) =>
                  previous.map((job) =>
                    job.id === runtime.id
                      ? { ...job, steps: applyStep(job.steps, status, message) }
                      : job
                  )
                );
                runtime.reconnects = 0;
              }
              break;
            case 'complete':
              finishJob(
                runtime,
                data.success ?? true,
                data.message || 'Model installed successfully'
              );
              break;
            case 'error':
              finishJob(runtime, false, data.message || 'Failed to install model');
              break;
          }
        } catch {
          // Ignore frames that do not belong to the pull protocol.
        }
      };

      socket.onerror = () => {
        // onclose reconnects or settles; avoid double-finishing here.
      };

      socket.onclose = () => {
        if (generation !== runtime.generation || runtime.settled) return;
        if (runtime.socket === socket) runtime.socket = null;
        runtime.reconnects += 1;
        if (runtime.reconnects > MAX_RECONNECTS) {
          finishJob(runtime, false, 'Connection closed before installation completed');
          return;
        }
        const delay =
          runtime.reconnects === 1 ? 0 : Math.min(500 * 2 ** (runtime.reconnects - 2), 8000);
        runtime.reconnectTimer = setTimeout(() => connect(runtime), delay);
      };
    },
    [finishJob]
  );

  const pull = useCallback(
    (modelName: string): Promise<boolean> => {
      return new Promise((resolve) => {
        const name = modelName.trim();
        if (!isAuthenticated || !accessToken || !name || !canStart(name)) {
          resolve(false);
          return;
        }

        pullingNames.current.add(name);
        const id = `pull-${nextId.current++}`;
        for (const existing of jobsRef.current) {
          if (existing.modelName === name && !existing.pulling) {
            clearDismiss(existing.id);
          }
        }
        setJobs((previous) => [
          ...previous.filter((job) => job.modelName !== name || job.pulling),
          createJob(id, name),
        ]);

        const runtime: JobRuntime = {
          id,
          name,
          generation: 1,
          reconnects: 0,
          reconnectTimer: null,
          socket: null,
          settled: false,
          waiters: [resolve],
          finish: (success, message, update = true) => finishJob(runtime, success, message, update),
        };
        runtimes.current.set(id, runtime);
        persistActive();
        connect(runtime);
      });
    },
    [accessToken, canStart, clearDismiss, connect, finishJob, isAuthenticated, persistActive]
  );

  const cancelRuntime = useCallback(
    (runtime: JobRuntime) => {
      try {
        runtime.socket?.send(JSON.stringify({ model: runtime.name, cancel: true }));
      } catch {
        // Local cancel still settles even if the frame cannot be written.
      }
      finishJob(runtime, false, 'Installation cancelled');
    },
    [finishJob]
  );

  const dismiss = useCallback(
    (id: string) => {
      clearDismiss(id);
      const runtime = runtimes.current.get(id);
      if (runtime) {
        cancelRuntime(runtime);
      }
      setJobs((previous) => previous.filter((job) => job.id !== id));
    },
    [cancelRuntime, clearDismiss]
  );

  const cancel = useCallback(
    (id?: string) => {
      if (id) {
        const runtime = runtimes.current.get(id);
        if (runtime) {
          cancelRuntime(runtime);
          return;
        }
        setJobs((previous) =>
          previous.map((job) =>
            job.id === id
              ? {
                  ...job,
                  pulling: false,
                  result: { success: false, message: 'Installation cancelled' },
                }
              : job
          )
        );
        return;
      }
      const pulling = jobsRef.current.filter((job) => job.pulling);
      if (pulling.length === 0) {
        setJobs((previous) => [
          ...previous,
          {
            id: `pull-${nextId.current++}`,
            modelName: '',
            pulling: false,
            progress: null,
            chunk: null,
            steps: [],
            result: { success: false, message: 'Installation cancelled' },
          },
        ]);
        return;
      }
      for (const job of pulling) {
        const runtime = runtimes.current.get(job.id);
        if (runtime) cancelRuntime(runtime);
      }
    },
    [cancelRuntime]
  );

  const reset = useCallback(() => {
    for (const job of jobsRef.current) {
      if (!job.pulling) clearDismiss(job.id);
    }
    setJobs((previous) => previous.filter((job) => job.pulling));
  }, [clearDismiss]);

  useEffect(() => {
    if (!isAuthenticated || !accessToken || resumedRef.current) return;
    resumedRef.current = true;
    for (const name of savedPulls()) {
      void pull(name);
    }
  }, [accessToken, isAuthenticated, pull]);

  useEffect(() => {
    return () => {
      for (const timer of dismissTimers.current.values()) {
        window.clearTimeout(timer);
      }
      dismissTimers.current.clear();
      for (const runtime of runtimes.current.values()) {
        runtime.generation += 1;
        clearReconnect(runtime);
        detachSocket(runtime.socket);
        runtime.socket = null;
      }
    };
  }, [clearReconnect]);

  const latest = jobs[jobs.length - 1];
  const pulling = jobs.some((job) => job.pulling);
  const activeCount = jobs.filter((job) => job.pulling).length;

  return useMemo(
    () => ({
      jobs,
      pulling,
      activeCount,
      progress: latest?.progress ?? null,
      chunk: latest?.chunk ?? null,
      steps: latest?.steps ?? [],
      result: latest?.result ?? null,
      model: latest?.modelName ?? null,
      minimized,
      setMinimized,
      canStart,
      pull,
      cancel,
      dismiss,
      reset,
    }),
    [
      activeCount,
      canStart,
      cancel,
      dismiss,
      jobs,
      latest?.chunk,
      latest?.modelName,
      latest?.progress,
      latest?.result,
      latest?.steps,
      minimized,
      pull,
      pulling,
      reset,
    ]
  );
}

export function PullProvider({ children }: { children: ReactNode }) {
  const value = usePullState();
  return createElement(PullContext.Provider, { value }, children);
}

export function usePull(): PullApi {
  const context = useContext(PullContext);
  if (!context) {
    throw new Error('usePull must be used within PullProvider');
  }
  return context;
}

export { MAX_PARALLEL_PULLS, PULL_SUCCESS_DISMISS_MS };
