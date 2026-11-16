import { useMutation, useQueryClient } from '@tanstack/react-query';
import { policyApi } from '../api';
import { PolicyRefusal } from '../PolicyRefusal';
import { policyQueryKeys } from '../queryKeys';
import type { UpdatePolicyRequest, WorkspacePolicyResponse } from '../types';

export interface UpdateWorkspacePolicyState {
  update(request: UpdatePolicyRequest, onUpdated?: (policy: WorkspacePolicyResponse) => void): void;
  pending: boolean;
  refusal: PolicyRefusal | null;
  error: string | null;
  reset(): void;
}

export function useUpdateWorkspacePolicy(
  organizationId: string,
  workspaceId: string
): UpdateWorkspacePolicyState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (request: UpdatePolicyRequest) =>
      policyApi.updateWorkspace(organizationId, workspaceId, request),
    onSuccess: (policy) => {
      queryClient.setQueryData(policyQueryKeys.workspace(organizationId, workspaceId), policy);
    },
  });

  return {
    update: (request, onUpdated) => mutate(request, { onSuccess: (policy) => onUpdated?.(policy) }),
    pending: isPending,
    refusal: error instanceof PolicyRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
