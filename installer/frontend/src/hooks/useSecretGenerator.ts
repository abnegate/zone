import { useCallback } from 'react';

export function useSecretGenerator() {
  const generateSecret = useCallback((): string => {
    const array = new Uint8Array(32);
    crypto.getRandomValues(array);
    return btoa(String.fromCharCode.apply(null, Array.from(array)));
  }, []);

  return { generateSecret };
}
