import type { z } from 'zod';
import type {
  CreateRoleRequestSchema,
  CustomRoleReferenceSchema,
  CustomRoleSchema,
  PermissionDefinitionSchema,
  RoleRefusalSchema,
  RoleResponseSchema,
  RolesResponseSchema,
  UpdateRoleRequestSchema,
} from './schemas';

export type PermissionDefinition = z.infer<typeof PermissionDefinitionSchema>;
export type CustomRole = z.infer<typeof CustomRoleSchema>;
export type CustomRoleReference = z.infer<typeof CustomRoleReferenceSchema>;
export type RolesResponse = z.infer<typeof RolesResponseSchema>;
export type RoleResponse = z.infer<typeof RoleResponseSchema>;
export type CreateRoleRequest = z.infer<typeof CreateRoleRequestSchema>;
export type UpdateRoleRequest = z.infer<typeof UpdateRoleRequestSchema>;
export type RoleRefusalBody = z.infer<typeof RoleRefusalSchema>;
