import { useState, useCallback, useRef } from 'react';
import { withRetry, RetryError } from '../utils/retry';
import type { InstallerConfig } from '../types';

interface StatusLine {
  message: string;
  type?: 'normal' | 'success' | 'error' | 'retry';
}

interface InstallationState {
  isInstalling: boolean;
  progress: number;
  statusLines: StatusLine[];
  isComplete: boolean;
  error: string | null;
  retryCount: number;
}

const INSTALL_TIMEOUT = 120000; // 2 minutes
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

  const install = useCallback(async (config: InstallerConfig) => {
    // Cancel any existing installation
    abortControllerRef.current?.abort();
    abortControllerRef.current = new AbortController();

    setState({
      isInstalling: true,
      progress: 0,
      statusLines: [{ message: 'Preparing installation...' }],
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
          const newStatusLines: StatusLine[] = [];

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
                  const isSuccess = data.status.includes('✓');
                  newStatusLines.push({
                    message: data.status,
                    type: isSuccess ? 'success' : 'normal',
                  });

                  setState(prev => ({
                    ...prev,
                    statusLines: [...newStatusLines],
                  }));
                }

                if (data.progress) {
                  setState(prev => ({
                    ...prev,
                    progress: data.progress,
                  }));
                }

                if (data.complete) {
                  setState(prev => ({
                    ...prev,
                    isComplete: true,
                    isInstalling: false,
                  }));
                }

                if (data.error) {
                  throw new Error(data.error);
                }
              } catch (e) {
                // Handle non-JSON lines (but rethrow actual errors)
                if (e instanceof Error && e.message !== line.trim()) {
                  if (line.trim() && !line.includes('{')) {
                    newStatusLines.push({ message: line });
                    setState(prev => ({
                      ...prev,
                      statusLines: [...newStatusLines],
                    }));
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
            setState(prev => ({
              ...prev,
              retryCount: attempt,
              statusLines: [
                ...prev.statusLines,
                {
                  message: `Retry ${attempt}/${MAX_RETRIES}: ${error.message}`,
                  type: 'retry',
                },
              ],
            }));
          },
        }
      );
    } catch (error) {
      const errorMessage = error instanceof RetryError
        ? `Installation failed after ${error.attempts} attempts: ${error.lastError.message}`
        : error instanceof Error
        ? error.message
        : 'Unknown error';

      setState(prev => ({
        ...prev,
        isInstalling: false,
        error: errorMessage,
      }));
    }
  }, []);

  const cancel = useCallback(() => {
    abortControllerRef.current?.abort();
    setState(prev => ({
      ...prev,
      isInstalling: false,
      error: 'Installation cancelled',
    }));
  }, []);

  const reset = useCallback(() => {
    abortControllerRef.current?.abort();
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
