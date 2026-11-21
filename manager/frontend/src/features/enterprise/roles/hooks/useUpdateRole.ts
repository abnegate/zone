import { useMutation, useQueryClient } from '@tanstack/react-query';
import { rolesApi } from '../api';
import { RoleRefusal } from '../RoleRefusal';
import type { CustomRole, UpdateRoleRequest } from '../types';
import { cacheRoles, refreshRoles } from './cache';

interface Update {
  roleId: string;
  request: UpdateRoleRequest;
}

export interface UpdateRoleState {
  update(roleId: string, request: UpdateRoleRequest, onUpdated?: (role: CustomRole) => void): void;
  pending: boolean;
  refusal: RoleRefusal | null;
  error: string | null;
  reset(): void;
}

export function useUpdateRole(organizationId: string): UpdateRoleState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: ({ roleId, request }: Update) => rolesApi.update(organizationId, roleId, request),
    onSuccess: (saved) =>
      cacheRoles(queryClient, organizationId, (roles) =>
        roles.map((role) => (role.id === saved.id ? saved : role))
      ),
    onSettled: () => refreshRoles(queryClient, organizationId),
  });

  return {
    update: (roleId, request, onUpdated) =>
      mutate({ roleId, request }, { onSuccess: (role) => onUpdated?.(role) }),
    pending: isPending,
    refusal: error instanceof RoleRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
