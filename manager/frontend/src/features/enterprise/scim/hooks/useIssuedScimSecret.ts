import { useQueryClient } from '@tanstack/react-query';
import { useCallback, useSyncExternalStore } from 'react';
import type { IssuedResponse } from '../types';
import { IssuedSecrets } from './IssuedSecrets';

export interface IssuedScimSecretState {
  issued: IssuedResponse | null;
  dismiss(): void;
}

export function useIssuedScimSecret(organizationId: string): IssuedScimSecretState {
  const secrets = IssuedSecrets.of(useQueryClient());
  const subscribe = useCallback((listener: () => void) => secrets.subscribe(listener), [secrets]);
  const snapshot = useCallback(() => secrets.get(organizationId), [secrets, organizationId]);
  const issued = useSyncExternalStore(subscribe, snapshot, snapshot);
  const dismiss = useCallback(() => secrets.forget(organizationId), [secrets, organizationId]);
  return { issued, dismiss };
}
