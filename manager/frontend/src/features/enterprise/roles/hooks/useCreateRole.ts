import { useMutation, useQueryClient } from '@tanstack/react-query';
import { rolesApi } from '../api';
import { RoleRefusal } from '../RoleRefusal';
import type { CreateRoleRequest, CustomRole } from '../types';
import { cacheRoles, refreshRoles } from './cache';

export interface CreateRoleState {
  create(request: CreateRoleRequest, onCreated?: (role: CustomRole) => void): void;
  pending: boolean;
  refusal: RoleRefusal | null;
  error: string | null;
  reset(): void;
}

export function useCreateRole(organizationId: string): CreateRoleState {
  const queryClient = useQueryClient();

  const { mutate, isPending, error, reset } = useMutation({
    mutationFn: (request: CreateRoleRequest) => rolesApi.create(organizationId, request),
    onSuccess: (role) => cacheRoles(queryClient, organizationId, (roles) => [...roles, role]),
    onSettled: () => refreshRoles(queryClient, organizationId),
  });

  return {
    create: (request, onCreated) => mutate(request, { onSuccess: (role) => onCreated?.(role) }),
    pending: isPending,
    refusal: error instanceof RoleRefusal ? error : null,
    error: error ? error.message : null,
    reset,
  };
}
