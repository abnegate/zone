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
import { modelsApi, type TrainJob } from '../../../api/models';
import { useAuth } from '../../../features/auth';

const POLL_MS = 2000;
const SUCCESS_DISMISS_MS = 12_000;

export interface TrainApi {
  job: TrainJob | null;
  dismiss: () => void;
}

const TrainContext = createContext<TrainApi | null>(null);

export function useTrainState(): TrainApi {
  const { isAuthenticated } = useAuth();
  const [job, setJob] = useState<TrainJob | null>(null);
  const [dismissedId, setDismissedId] = useState<string | null>(null);
  const dismissTimer = useRef<number | null>(null);

  const clearDismissTimer = useCallback(() => {
    if (dismissTimer.current !== null) {
      window.clearTimeout(dismissTimer.current);
      dismissTimer.current = null;
    }
  }, []);

  useEffect(() => {
    if (!isAuthenticated) {
      setJob(null);
      return;
    }
    let cancelled = false;
    const tick = async () => {
      try {
        const next = await modelsApi.trainJob();
        if (!cancelled) setJob(next);
      } catch {
        if (!cancelled) setJob(null);
      }
    };
    void tick();
    const interval = window.setInterval(() => {
      void tick();
    }, POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [isAuthenticated]);

  useEffect(() => {
    if (!job?.id || job.status === 'running') {
      clearDismissTimer();
      return;
    }
    if (job.status !== 'succeeded' || dismissedId === job.id) return;
    clearDismissTimer();
    dismissTimer.current = window.setTimeout(() => {
      setDismissedId(job.id ?? null);
    }, SUCCESS_DISMISS_MS);
    return clearDismissTimer;
  }, [clearDismissTimer, dismissedId, job]);

  useEffect(() => {
    if (job?.status === 'running' && job.id && dismissedId === job.id) {
      setDismissedId(null);
    }
  }, [dismissedId, job]);

  const dismiss = useCallback(() => {
    clearDismissTimer();
    const key = job?.id ?? 'current';
    setDismissedId(key);
    void modelsApi.dismissTrain().catch(() => {
      setDismissedId((current) => (current === key ? null : current));
    });
  }, [clearDismissTimer, job?.id]);

  const visible = job && dismissedId && (job.id ?? 'current') === dismissedId ? null : job;

  return useMemo(
    () => ({
      job: visible,
      dismiss,
    }),
    [dismiss, visible]
  );
}

export function TrainProvider({ children }: { children: ReactNode }) {
  const value = useTrainState();
  return createElement(TrainContext.Provider, { value }, children);
}

export function useTrain(): TrainApi {
  const context = useContext(TrainContext);
  if (!context) {
    throw new Error('useTrain must be used within TrainProvider');
  }
  return context;
}
