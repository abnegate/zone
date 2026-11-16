import { useIsMutating } from '@tanstack/react-query';
import { licenseMutationKeys } from '../queryKeys';

export function useLicenseChanging(): boolean {
  return useIsMutating({ mutationKey: licenseMutationKeys.change }) > 0;
}
