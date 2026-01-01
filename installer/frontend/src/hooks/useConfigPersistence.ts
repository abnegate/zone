import { useEffect, useCallback, useRef } from 'react';
import { saveConfig, loadConfig, clearConfig } from '../utils/crypto';
import type { InstallerConfig } from '../types';

export function useConfigPersistence(
  config: InstallerConfig,
  setConfig: (config: InstallerConfig) => void,
  defaultConfig: InstallerConfig
) {
  const hasLoadedRef = useRef(false);
  const isInitialMountRef = useRef(true);

  // Load config on mount
  useEffect(() => {
    if (hasLoadedRef.current) return;
    hasLoadedRef.current = true;

    loadConfig().then((stored) => {
      if (stored && Object.keys(stored).length > 0) {
        setConfig({ ...defaultConfig, ...stored });
      }
    });
  }, [defaultConfig, setConfig]);

  // Auto-save on change (debounced), skip initial mount
  useEffect(() => {
    if (isInitialMountRef.current) {
      isInitialMountRef.current = false;
      return;
    }

    const timer = setTimeout(() => {
      saveConfig(config);
    }, 500);

    return () => clearTimeout(timer);
  }, [config]);

  const resetConfig = useCallback(() => {
    clearConfig();
    setConfig(defaultConfig);
  }, [defaultConfig, setConfig]);

  return { resetConfig };
}
