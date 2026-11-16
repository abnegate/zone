import { useEffect, useState } from 'react';
import { listProviders } from '../../../api/sso';
import type { SsoPublicProviders } from '../types';

export function useSsoProviders(): SsoPublicProviders | null {
  const [providers, setProviders] = useState<SsoPublicProviders | null>(null);

  useEffect(() => {
    let current = true;
    listProviders().then(
      (loaded) => {
        if (current) {
          setProviders(loaded);
        }
      },
      () => {}
    );
    return () => {
      current = false;
    };
  }, []);

  return providers;
}
