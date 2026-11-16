import { useMutation, useQueryClient } from '@tanstack/react-query';
import { policyApi } from '../api';
import { PolicyRefusal } from '../PolicyRefusal';
import { policyQueryKeys } from '../queryKeys';
import type { OrganizationPolicyResponse, UpdatePolicyRequest } from '../types';

export interface UpdateOrganizationPolicyState {
  update(
    request: UpdatePolicyRequest,
    onUpdated?: (policy: OrganizationPolicyResponse) => void
  ): void;
  pending: boolean;
  refusal: PolicyRefusal | null;
  error: string | null;
  reset(): void;
}

export function useUpdateOrganizationPolicy(organizationId: string): UpdateOrganizationPolicyState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (request: UpdatePolicyRequest) =>
      policyApi.updateOrganization(organizationId, request),
    onSuccess: (policy) => {
      queryClient.setQueryData(policyQueryKeys.organization(organizationId), policy);
      void queryClient.invalidateQueries({ queryKey: policyQueryKeys.workspaces(organizationId) });
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
