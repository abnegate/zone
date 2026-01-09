import { useCallback, useEffect, useRef, useState } from 'react';
import { modelsApi } from '../../../api/models';
import { useAuth } from '../../../features/auth';
import type { PullProgress, Step } from '../types';

export function usePull() {
  const { isAuthenticated } = useAuth();
  const [pulling, setPulling] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const [steps, setSteps] = useState<Step[]>([]);
  const [result, setResult] = useState<{ success: boolean; message: string } | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const pullingRef = useRef(pulling);

  // Keep ref in sync with state to avoid stale closures
  useEffect(() => {
    pullingRef.current = pulling;
  }, [pulling]);

  const pull = useCallback(
    (modelName: string): Promise<boolean> => {
      return new Promise((resolve) => {
        if (!isAuthenticated || !modelName.trim()) {
          resolve(false);
          return;
        }

        setPulling(true);
        setProgress(null);
        setSteps([]);
        setResult(null);

        const ws = modelsApi.createPullWebSocket(modelName.trim());
        wsRef.current = ws;

        ws.onopen = () => {
          // For now, we'll skip authentication via WebSocket
          // and send the pull request directly
          const msg = JSON.stringify({ model: modelName.trim() });
          ws.send(msg);
        };

        ws.onmessage = (event) => {
          try {
            const data: PullProgress = JSON.parse(event.data);

            // Handle authentication response
            if (data.type === 'authenticated') {
              // Now send the pull request
              const msg = JSON.stringify({ model: modelName.trim() });
              ws.send(msg);
              return;
            }

            switch (data.type) {
              case 'progress':
                if (data.percent !== undefined) {
                  setProgress(data.percent);
                }
                break;

              case 'step':
                if (data.status) {
                  const stepName = data.status;
                  setSteps((prev) => {
                    const existing = prev.find((s) => s.name === stepName);
                    if (existing) {
                      return prev.map((s) =>
                        s.name === stepName
                          ? { ...s, message: data.message || '', status: 'success' as const }
                          : s
                      );
                    }
                    return [
                      ...prev,
                      {
                        name: stepName,
                        message: data.message || '',
                        status: 'pending' as const,
                      },
                    ];
                  });
                }
                break;

              case 'complete':
                setPulling(false);
                setResult({
                  success: data.success ?? true,
                  message: data.message || 'Model installed successfully',
                });
                ws.close();
                resolve(data.success ?? true);
                break;

              case 'error':
                setPulling(false);
                setResult({
                  success: false,
                  message: data.message || 'Failed to install model',
                });
                ws.close();
                resolve(false);
                break;
            }
          } catch {
            // Ignore parse errors
          }
        };

        ws.onerror = () => {
          setPulling(false);
          setResult({
            success: false,
            message: 'Connection error',
          });
          resolve(false);
        };

        ws.onclose = () => {
          if (pullingRef.current) {
            setPulling(false);
          }
        };
      });
    },
    [isAuthenticated]
  );

  const reset = useCallback(() => {
    setProgress(null);
    setSteps([]);
    setResult(null);
  }, []);

  const cancel = useCallback(() => {
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    setPulling(false);
    setResult({
      success: false,
      message: 'Installation cancelled',
    });
  }, []);

  // Cleanup WebSocket on unmount
  useEffect(() => {
    return () => {
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, []);

  return { pulling, progress, steps, result, pull, reset, cancel };
}
