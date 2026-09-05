import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from 'react';
import { modelsApi } from '../../../api/models';
import { useAuth } from '../../../features/auth';
import type { PullChunk, PullProgress, Step } from '../types';

const STORAGE_KEY = 'zone.activePull';
const MAX_RECONNECTS = 8;

export interface PullApi {
  pulling: boolean;
  progress: number | null;
  chunk: PullChunk | null;
  steps: Step[];
  result: { success: boolean; message: string } | null;
  model: string | null;
  pull: (modelName: string) => Promise<boolean>;
  reset: () => void;
  cancel: () => void;
}

const PullContext = createContext<PullApi | null>(null);

function persist(model: string | null): void {
  try {
    if (model) sessionStorage.setItem(STORAGE_KEY, model);
    else sessionStorage.removeItem(STORAGE_KEY);
  } catch {
    // Private browsing can reject sessionStorage writes.
  }
}

function savedPull(): string | null {
  try {
    return sessionStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
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

function usePullState(): PullApi {
  const { isAuthenticated, accessToken } = useAuth();
  const [pulling, setPulling] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const [chunk, setChunk] = useState<PullChunk | null>(null);
  const [steps, setSteps] = useState<Step[]>([]);
  const [result, setResult] = useState<{ success: boolean; message: string } | null>(null);
  const [model, setModel] = useState<string | null>(null);

  const socketRef = useRef<WebSocket | null>(null);
  const modelRef = useRef<string | null>(null);
  const tokenRef = useRef(accessToken);
  const generationRef = useRef(0);
  const reconnectsRef = useRef(0);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const settledRef = useRef(true);
  const waitersRef = useRef<Array<(ok: boolean) => void>>([]);
  tokenRef.current = accessToken;

  const clearTimer = useCallback(() => {
    if (reconnectTimerRef.current) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
  }, []);

  const detachSocket = useCallback((socket: WebSocket | null) => {
    if (!socket) return;
    socket.onopen = null;
    socket.onmessage = null;
    socket.onerror = null;
    socket.onclose = null;
    socket.close();
  }, []);

  const finish = useCallback(
    (success: boolean, message: string, update = true) => {
      if (settledRef.current) return;
      settledRef.current = true;
      generationRef.current += 1;
      clearTimer();
      persist(null);
      const socket = socketRef.current;
      socketRef.current = null;
      detachSocket(socket);
      if (update) {
        setPulling(false);
        setSteps((previous) =>
          previous.map((step) =>
            step.status === 'pending' ? { ...step, status: success ? 'success' : 'error' } : step
          )
        );
        setResult({ success, message });
      }
      const waiters = waitersRef.current.splice(0);
      for (const waiter of waiters) waiter(success);
    },
    [clearTimer, detachSocket]
  );

  const connect = useCallback(
    (name: string, generation: number) => {
      if (generation !== generationRef.current) return;
      let socket: WebSocket;
      try {
        socket = modelsApi.createPullWebSocket(name);
      } catch {
        finish(false, 'Connection error');
        return;
      }
      if (!socket) {
        finish(false, 'Connection closed before installation completed');
        return;
      }

      socketRef.current = socket;
      let requested = false;

      socket.onopen = () => {
        if (generation !== generationRef.current) return;
        try {
          socket.send(JSON.stringify({ type: 'auth', token: tokenRef.current }));
        } catch {
          finish(false, 'Connection error');
        }
      };

      socket.onmessage = (event) => {
        if (generation !== generationRef.current || settledRef.current) return;
        try {
          const data: PullProgress = JSON.parse(event.data);
          if (data.type === 'authenticated') {
            if (requested) return;
            requested = true;
            try {
              socket.send(JSON.stringify({ model: name }));
            } catch {
              finish(false, 'Connection error');
            }
            return;
          }

          switch (data.type) {
            case 'progress':
              if (data.percent !== undefined) setProgress(data.percent);
              if (data.completed !== undefined && data.total !== undefined) {
                setChunk({
                  completed: data.completed,
                  total: data.total,
                  digest: data.digest,
                });
              }
              reconnectsRef.current = 0;
              break;
            case 'step':
              if (data.status) {
                const status = data.status;
                const message = data.message || '';
                setSteps((previous) => applyStep(previous, status, message));
                reconnectsRef.current = 0;
              }
              break;
            case 'complete':
              finish(data.success ?? true, data.message || 'Model installed successfully');
              break;
            case 'error':
              finish(false, data.message || 'Failed to install model');
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
        if (generation !== generationRef.current || settledRef.current) return;
        if (socketRef.current === socket) socketRef.current = null;
        reconnectsRef.current += 1;
        if (reconnectsRef.current > MAX_RECONNECTS) {
          finish(false, 'Connection closed before installation completed');
          return;
        }
        const delay =
          reconnectsRef.current === 1 ? 0 : Math.min(500 * 2 ** (reconnectsRef.current - 2), 8000);
        reconnectTimerRef.current = setTimeout(() => connect(name, generation), delay);
      };
    },
    [finish]
  );

  const pull = useCallback(
    (modelName: string): Promise<boolean> => {
      return new Promise((resolve) => {
        if (!isAuthenticated || !accessToken || !modelName.trim()) {
          resolve(false);
          return;
        }

        const name = modelName.trim();
        if (!settledRef.current && modelRef.current === name) {
          waitersRef.current.push(resolve);
          return;
        }

        if (!settledRef.current && modelRef.current && modelRef.current !== name) {
          try {
            socketRef.current?.send(JSON.stringify({ model: modelRef.current, cancel: true }));
          } catch {
            // The replaced job is cancelled best-effort.
          }
          const previous = waitersRef.current.splice(0);
          for (const waiter of previous) waiter(false);
        }

        settledRef.current = false;
        reconnectsRef.current = 0;
        generationRef.current += 1;
        const generation = generationRef.current;
        modelRef.current = name;
        waitersRef.current.push(resolve);
        persist(name);
        setModel(name);
        setPulling(true);
        setProgress(null);
        setChunk(null);
        setSteps([]);
        setResult(null);
        clearTimer();
        detachSocket(socketRef.current);
        socketRef.current = null;
        connect(name, generation);
      });
    },
    [accessToken, clearTimer, connect, detachSocket, isAuthenticated]
  );

  const reset = useCallback(() => {
    setProgress(null);
    setChunk(null);
    setSteps([]);
    setResult(null);
  }, []);

  const cancel = useCallback(() => {
    if (settledRef.current) {
      setPulling(false);
      setResult({ success: false, message: 'Installation cancelled' });
      return;
    }
    try {
      if (modelRef.current) {
        socketRef.current?.send(JSON.stringify({ model: modelRef.current, cancel: true }));
      }
    } catch {
      // Local cancel still settles even if the frame cannot be written.
    }
    finish(false, 'Installation cancelled');
  }, [finish]);

  useEffect(() => {
    if (!isAuthenticated || !accessToken) return;
    const saved = savedPull();
    if (saved && settledRef.current) {
      void pull(saved);
    }
  }, [accessToken, isAuthenticated, pull]);

  useEffect(() => {
    return () => {
      generationRef.current += 1;
      clearTimer();
      detachSocket(socketRef.current);
    };
  }, [clearTimer, detachSocket]);

  return { pulling, progress, chunk, steps, result, model, pull, reset, cancel };
}

export function PullProvider({ children }: { children: ReactNode }) {
  const value = usePullState();
  return <PullContext.Provider value={value}>{children}</PullContext.Provider>;
}

export function usePull(): PullApi {
  const context = useContext(PullContext);
  if (!context) {
    throw new Error('usePull requires PullProvider');
  }
  return context;
}
