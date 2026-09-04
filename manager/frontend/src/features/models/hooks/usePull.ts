import { useCallback, useEffect, useRef, useState } from 'react';
import { modelsApi } from '../../../api/models';
import { useAuth } from '../../../features/auth';
import type { PullProgress, Step } from '../types';

export function usePull() {
  const { isAuthenticated, accessToken } = useAuth();
  const [pulling, setPulling] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const [steps, setSteps] = useState<Step[]>([]);
  const [result, setResult] = useState<{ success: boolean; message: string } | null>(null);
  const finishRef = useRef<((success: boolean, message: string, update?: boolean) => void) | null>(
    null
  );

  const pull = useCallback(
    (modelName: string): Promise<boolean> => {
      return new Promise((resolve) => {
        if (!isAuthenticated || !accessToken || !modelName.trim()) {
          resolve(false);
          return;
        }

        finishRef.current?.(false, 'Installation cancelled');
        setPulling(true);
        setProgress(null);
        setSteps([]);
        setResult(null);

        let socket: WebSocket | null = null;
        let settled = false;
        let requested = false;
        const finish = (success: boolean, message: string, update = true): void => {
          if (settled) return;
          settled = true;
          if (finishRef.current === finish) finishRef.current = null;
          if (update) {
            setPulling(false);
            setSteps((previous) =>
              previous.map((step) =>
                step.status === 'pending'
                  ? { ...step, status: success ? 'success' : 'error' }
                  : step
              )
            );
            setResult({ success, message });
          }
          if (socket) {
            socket.onopen = null;
            socket.onmessage = null;
            socket.onerror = null;
            socket.onclose = null;
            socket.close();
          }
          resolve(success);
        };
        finishRef.current = finish;

        try {
          socket = modelsApi.createPullWebSocket(modelName.trim());
        } catch {
          finish(false, 'Connection error');
          return;
        }

        socket.onopen = () => {
          if (settled) return;
          try {
            socket?.send(JSON.stringify({ type: 'auth', token: accessToken }));
          } catch {
            finish(false, 'Connection error');
          }
        };

        socket.onmessage = (event) => {
          if (settled) return;
          try {
            const data: PullProgress = JSON.parse(event.data);
            if (data.type === 'authenticated') {
              if (requested) return;
              requested = true;
              try {
                socket?.send(JSON.stringify({ model: modelName.trim() }));
              } catch {
                finish(false, 'Connection error');
              }
              return;
            }

            switch (data.type) {
              case 'progress':
                if (data.percent !== undefined) setProgress(data.percent);
                break;
              case 'step':
                if (data.status) {
                  const name = data.status;
                  setSteps((previous) => {
                    const existing = previous.find((step) => step.name === name);
                    if (existing) {
                      return previous.map((step) =>
                        step.name === name
                          ? { ...step, message: data.message || '', status: 'success' as const }
                          : step
                      );
                    }
                    return [
                      ...previous,
                      { name, message: data.message || '', status: 'pending' as const },
                    ];
                  });
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
        socket.onerror = () => finish(false, 'Connection error');
        socket.onclose = () => finish(false, 'Connection closed before installation completed');
      });
    },
    [isAuthenticated, accessToken]
  );

  const reset = useCallback(() => {
    setProgress(null);
    setSteps([]);
    setResult(null);
  }, []);

  const cancel = useCallback(() => {
    if (finishRef.current) {
      finishRef.current(false, 'Installation cancelled');
    } else {
      setPulling(false);
      setResult({ success: false, message: 'Installation cancelled' });
    }
  }, []);

  useEffect(() => {
    return () => finishRef.current?.(false, 'Installation cancelled', false);
  }, []);

  return { pulling, progress, steps, result, pull, reset, cancel };
}
