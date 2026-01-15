import { useCallback, useRef, useState } from 'react';
import type { InstallerConfig } from '../types';
import { RetryError, withRetry } from '../utils/retry';

interface StatusLine {
  message: string;
  type?: 'normal' | 'success' | 'error' | 'retry' | 'in-progress';
  id?: string;
}

interface InstallationState {
  isInstalling: boolean;
  progress: number;
  statusLines: StatusLine[];
  isComplete: boolean;
  error: string | null;
  retryCount: number;
}

const INSTALL_TIMEOUT = 1200000; // 20 minutes
const MAX_RETRIES = 3;

export function useInstallation() {
  const [state, setState] = useState<InstallationState>({
    isInstalling: false,
    progress: 0,
    statusLines: [],
    isComplete: false,
    error: null,
    retryCount: 0,
  });

  const abortControllerRef = useRef<AbortController | null>(null);
  const statusLinesRef = useRef<StatusLine[]>([]);

  const install = useCallback(async (config: InstallerConfig) => {
    // Cancel any existing installation
    abortControllerRef.current?.abort();
    abortControllerRef.current = new AbortController();

    const initialLines = [
      {
        id: 'prepare',
        message: 'Preparing installation...',
        type: 'in-progress',
      },
    ];
    statusLinesRef.current = initialLines;
    setState({
      isInstalling: true,
      progress: 0,
      statusLines: initialLines,
      isComplete: false,
      error: null,
      retryCount: 0,
    });

    try {
      await withRetry(
        async (signal) => {
          const response = await fetch('/api/install', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(config),
            signal,
          });

          if (!response.ok) {
            throw new Error(`Installation failed: ${response.statusText}`);
          }

          const reader = response.body?.getReader();
          if (!reader) {
            throw new Error('No response body');
          }

          const decoder = new TextDecoder();
          let hasFinalizedPrepare = false;

          const updateLines = (line: StatusLine) => {
            const current = statusLinesRef.current;
            let next: StatusLine[] = [];

            if (line.id) {
              const index = current.findIndex((item) => item.id === line.id);
              next =
                index >= 0
                  ? current.map((item, i) => (i === index ? line : item))
                  : [...current, line];
            } else {
              next = [...current, line];
            }

            statusLinesRef.current = next;
            setState((prev) => ({
              ...prev,
              statusLines: next,
            }));
          };

          while (true) {
            const { done, value } = await reader.read();
            if (done) break;

            const chunk = decoder.decode(value);
            const lines = chunk.split('\n');

            for (const line of lines) {
              if (!line.trim()) continue;

              try {
                const data = JSON.parse(line);

                if (data.status) {
                  const statusText = typeof data.status === 'string' ? data.status : String(data.status);
                  const cleanStatus = statusText.replace(/^[\u2713\u2717]\s*/, '');
                  const stateValue = typeof data.state === 'string' ? data.state : undefined;
                  let lineType: StatusLine['type'] = 'success';

                  if (stateValue === 'in-progress') {
                    lineType = 'in-progress';
                  } else if (stateValue === 'retry') {
                    lineType = 'retry';
                  } else if (stateValue === 'error' || data.error) {
                    lineType = 'error';
                  }

                  if (!hasFinalizedPrepare) {
                    hasFinalizedPrepare = true;
                    updateLines({
                      id: 'prepare',
                      message: 'Preparation complete',
                      type: 'success',
                    });
                  }

                  updateLines({
                    id: typeof data.id === 'string' ? data.id : undefined,
                    message: cleanStatus,
                    type: lineType,
                  });
                }

                if (typeof data.progress === 'number') {
                  setState((prev) => ({
                    ...prev,
                    progress: data.progress,
                  }));
                }

                if (data.complete) {
                  setState((prev) => ({
                    ...prev,
                    isComplete: true,
                    isInstalling: false,
                  }));
                }

                if (data.error) {
                  const message =
                    typeof data.error === 'string'
                      ? data.error
                      : typeof data.status === 'string'
                        ? data.status
                        : 'Installation failed';
                  throw new Error(message);
                }
              } catch (e) {
                // Handle non-JSON lines (but rethrow actual errors)
                if (e instanceof Error && e.message !== line.trim()) {
                  if (line.trim() && !line.includes('{')) {
                    updateLines({ message: line });
                  }
                } else {
                  throw e;
                }
              }
            }
          }
        },
        {
          maxAttempts: MAX_RETRIES,
          timeout: INSTALL_TIMEOUT,
          onRetry: (attempt, error) => {
            const nextLines = [
              ...statusLinesRef.current,
              {
                message: `Retry ${attempt}/${MAX_RETRIES}: ${error.message}`,
                type: 'retry' as const,
              },
            ];
            statusLinesRef.current = nextLines;
            setState((prev) => ({
              ...prev,
              retryCount: attempt,
              statusLines: nextLines,
            }));
          },
        }
      );
    } catch (error) {
      const errorMessage =
        error instanceof RetryError
          ? `Installation failed after ${error.attempts} attempts: ${error.lastError.message}`
          : error instanceof Error
            ? error.message
            : 'Unknown error';

      setState((prev) => ({
        ...prev,
        isInstalling: false,
        error: errorMessage,
      }));
    }
  }, []);

  const cancel = useCallback(() => {
    abortControllerRef.current?.abort();
    statusLinesRef.current = [];
    setState((prev) => ({
      ...prev,
      isInstalling: false,
      error: 'Installation cancelled',
    }));
  }, []);

  const reset = useCallback(() => {
    abortControllerRef.current?.abort();
    statusLinesRef.current = [];
    setState({
      isInstalling: false,
      progress: 0,
      statusLines: [],
      isComplete: false,
      error: null,
      retryCount: 0,
    });
  }, []);

  return {
    ...state,
    install,
    cancel,
    reset,
  };
}
