import { useMutation, useQueryClient } from '@tanstack/react-query';
import { AuditRefusal } from '../AuditRefusal';
import { auditApi } from '../api';
import { auditQueryKeys } from '../queryKeys';
import type { AuditScope, RetentionResponse } from '../types';

export interface SetAuditRetentionState {
  set(days: number | null, onSet?: (retention: RetentionResponse) => void): void;
  pending: boolean;
  refusal: AuditRefusal | null;
  error: string | null;
  reset(): void;
}

export function useSetAuditRetention(scope: AuditScope): SetAuditRetentionState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (days: number | null) => auditApi.setRetention(scope, { days }),
    onSuccess: (retention) => {
      queryClient.setQueryData(auditQueryKeys.retention(scope), retention);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: auditQueryKeys.retention(scope) }),
  });

  return {
    set: (days, onSet) => mutate(days, { onSuccess: (retention) => onSet?.(retention) }),
    pending: isPending,
    refusal: error instanceof AuditRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
