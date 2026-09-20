import { useEffect, useState } from 'react';
import { sourcesApi } from '../../../api/sources';
import type { SourceType } from '../types';

/**
 * The kinds the server can verify, or null while unknown (still loading, or
 * the request failed and the local registry has to stand in).
 */
export function useSourceKinds(enabled: boolean): Set<SourceType> | null {
  const [kinds, setKinds] = useState<Set<SourceType> | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    Promise.resolve()
      .then(() => sourcesApi.getSourceTypes())
      .then((types) => {
        if (cancelled) return;
        setKinds(new Set(types.filter((type) => type.enabled).map((type) => type.id)));
      })
      .catch(() => {
        if (!cancelled) setKinds(null);
      });
    return () => {
      cancelled = true;
    };
  }, [enabled]);

  return kinds;
}
