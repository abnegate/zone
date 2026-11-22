import { useMutation, useQueryClient } from '@tanstack/react-query';
import { rolesApi } from '../api';
import { RoleRefusal } from '../RoleRefusal';
import { cacheRoles, refreshRoles } from './cache';

export interface DeleteRoleState {
  remove(roleId: string, onDeleted?: () => void): void;
  pending: boolean;
  refusal: RoleRefusal | null;
  error: string | null;
  reset(): void;
}

export function useDeleteRole(organizationId: string): DeleteRoleState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (roleId: string) => rolesApi.remove(organizationId, roleId),
    onSuccess: (_, roleId) =>
      cacheRoles(queryClient, organizationId, (roles) =>
        roles.filter((role) => role.id !== roleId)
      ),
    onSettled: () => refreshRoles(queryClient, organizationId),
  });

  return {
    remove: (roleId, onDeleted) => mutate(roleId, { onSuccess: () => onDeleted?.() }),
    pending: isPending,
    refusal: error instanceof RoleRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
