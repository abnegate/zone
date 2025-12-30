import { useCallback, useRef, useState } from 'react';
import { client } from '../api/client';
import { useAuth } from '../context/AuthContext';
import type { PullProgress, Step } from '../types';

export function usePull() {
  const { apiKey } = useAuth();
  const [pulling, setPulling] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const [steps, setSteps] = useState<Step[]>([]);
  const [result, setResult] = useState<{ success: boolean; message: string } | null>(null);
  const wsRef = useRef<WebSocket | null>(null);

  const pull = useCallback(
    (modelName: string): Promise<boolean> => {
      return new Promise((resolve) => {
        if (!apiKey || !modelName.trim()) {
          resolve(false);
          return;
        }

        setPulling(true);
        setProgress(null);
        setSteps([]);
        setResult(null);

        client.setApiKey(apiKey);
        const ws = client.createPullWebSocket(modelName.trim());
        wsRef.current = ws;

        ws.onopen = () => {
          const msg = JSON.stringify({ model: modelName.trim() });
          console.log('[usePull] WebSocket opened, sending:', msg);
          ws.send(msg);
        };

        ws.onmessage = (event) => {
          console.log('[usePull] Received:', event.data);
          try {
            const data: PullProgress = JSON.parse(event.data);

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

        ws.onerror = (err) => {
          console.log('[usePull] WebSocket error:', err);
          setPulling(false);
          setResult({
            success: false,
            message: 'Connection error',
          });
          resolve(false);
        };

        ws.onclose = (event) => {
          console.log('[usePull] WebSocket closed:', event.code, event.reason);
          if (pulling) {
            setPulling(false);
          }
        };
      });
    },
    [apiKey, pulling]
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

  return { pulling, progress, steps, result, pull, reset, cancel };
}
