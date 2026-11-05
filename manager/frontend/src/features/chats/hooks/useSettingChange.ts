import { useEffect, useRef } from 'react';
import { SETTING_CHANGED } from '../utils/panel';

/** Calls back whenever the panel setting under the key changes, in this tab or another. */
export function useSettingChange(key: string, onChange: () => void): void {
  const latest = useRef(onChange);
  latest.current = onChange;

  useEffect(() => {
    const follow = (event: Event) => {
      const changed =
        event instanceof CustomEvent
          ? event.detail
          : event instanceof StorageEvent
            ? event.key
            : null;
      if (changed === key) latest.current();
    };
    window.addEventListener(SETTING_CHANGED, follow);
    window.addEventListener('storage', follow);
    return () => {
      window.removeEventListener(SETTING_CHANGED, follow);
      window.removeEventListener('storage', follow);
    };
  }, [key]);
}
