import { useState, useCallback } from 'react';
import type { InstallerConfig } from '../types';

interface StatusLine {
  message: string;
  type?: 'normal' | 'success' | 'error';
}

interface InstallationState {
  isInstalling: boolean;
  progress: number;
  statusLines: StatusLine[];
  isComplete: boolean;
  error: string | null;
}

export function useInstallation() {
  const [state, setState] = useState<InstallationState>({
    isInstalling: false,
    progress: 0,
    statusLines: [],
    isComplete: false,
    error: null,
  });

  const install = useCallback(async (config: InstallerConfig) => {
    setState({
      isInstalling: true,
      progress: 0,
      statusLines: [{ message: 'Preparing installation...' }],
      isComplete: false,
      error: null,
    });

    try {
      const response = await fetch('/api/install', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(config),
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
              }));
            }

            if (data.error) {
              throw new Error(data.error);
            }
          } catch (e) {
            // Handle non-JSON lines
            if (line.trim() && !line.includes('{')) {
              newStatusLines.push({ message: line });
              setState(prev => ({
                ...prev,
                statusLines: [...newStatusLines],
              }));
            }
          }
        }
      }
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Unknown error';
      setState(prev => ({
        ...prev,
        error: errorMessage,
      }));
    }
  }, []);

  const reset = useCallback(() => {
    setState({
      isInstalling: false,
      progress: 0,
      statusLines: [],
      isComplete: false,
      error: null,
    });
  }, []);

  return {
    ...state,
    install,
    reset,
  };
}
