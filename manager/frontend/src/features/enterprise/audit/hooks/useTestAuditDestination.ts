import { useMutation } from '@tanstack/react-query';
import { AuditRefusal } from '../AuditRefusal';
import { auditApi } from '../api';
import type { AuditScope, TestResponse } from '../types';

export interface TestAuditDestinationState {
  test(destinationId: string, onTested?: (result: TestResponse) => void): void;
  result: TestResponse | undefined;
  testing: string | null;
  pending: boolean;
  refusal: AuditRefusal | null;
  error: string | null;
  reset(): void;
}

export function useTestAuditDestination(scope: AuditScope): TestAuditDestinationState {
  const { mutate, data, variables, isPending, error, reset } = useMutation({
    mutationFn: (destinationId: string) => auditApi.testDestination(scope, destinationId),
  });

  return {
    test: (destinationId, onTested) =>
      mutate(destinationId, { onSuccess: (result) => onTested?.(result) }),
    result: data,
    testing: variables ?? null,
    pending: isPending,
    refusal: error instanceof AuditRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
