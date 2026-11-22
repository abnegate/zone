import { z } from 'zod';
import { CustomRoleReferenceSchema } from '../roles/schemas';
import { EditionSchema, LicenseStatusSchema } from '../schemas';

export const ScimCode = {
  LicenseRequired: 'license_required',
  TokenNotFound: 'scim_token_not_found',
  GroupNotFound: 'scim_group_not_found',
  TokenNameInvalid: 'scim_token_name_invalid',
  MappingInvalid: 'scim_mapping_invalid',
  OwnerUnmappable: 'scim_owner_unmappable',
  RoleNotFound: 'scim_role_not_found',
  Exceeds: 'role_exceeds',
} as const;

export type ScimCode = (typeof ScimCode)[keyof typeof ScimCode];

export const MappedRole = {
  Admin: 'admin',
  Member: 'member',
} as const;

export type MappedRole = (typeof MappedRole)[keyof typeof MappedRole];

export const SCIM_ROUTE_CODES = [
  ScimCode.TokenNotFound,
  ScimCode.GroupNotFound,
  ScimCode.TokenNameInvalid,
  ScimCode.MappingInvalid,
  ScimCode.OwnerUnmappable,
  ScimCode.RoleNotFound,
] as const;

export const SCIM_CODES = [
  ScimCode.LicenseRequired,
  ...SCIM_ROUTE_CODES,
  ScimCode.Exceeds,
] as const;

export const MAPPED_ROLES = [MappedRole.Admin, MappedRole.Member] as const;

export const ScimCodeSchema = z.enum(SCIM_CODES);

export const MappedRoleSchema = z.enum(MAPPED_ROLES);

export const TokenResponseSchema = z.object({
  id: z.string(),
  name: z.string(),
  hint: z.string(),
  created_by: z.string().nullable(),
  created_at: z.string(),
  rotated_at: z.string().nullable(),
  last_used_at: z.string().nullable(),
});

export const GroupResponseSchema = z.object({
  id: z.string(),
  display_name: z.string(),
  external_id: z.string().nullable(),
  members: z.number().int().nonnegative(),
  role: MappedRoleSchema.nullable(),
  custom_role: CustomRoleReferenceSchema.nullable(),
  updated_at: z.string(),
});

export const FailureResponseSchema = z.object({
  id: z.string(),
  method: z.string(),
  path: z.string(),
  status: z.number().int(),
  scim_type: z.string().nullable(),
  detail: z.string(),
  occurred_at: z.string(),
});

export const SettingsResponseSchema = z.object({
  entitled: z.boolean(),
  edition: EditionSchema,
  status: LicenseStatusSchema,
  base_url: z.string(),
  last_synced_at: z.string().nullable(),
  tokens: z.array(TokenResponseSchema),
  groups: z.array(GroupResponseSchema),
  errors: z.array(FailureResponseSchema),
});

export const IssuedResponseSchema = z.object({
  token: TokenResponseSchema,
  secret: z.string().min(1),
});

export const CreateTokenRequestSchema = z.strictObject({
  name: z.string(),
});

export const MappingRequestSchema = z.union([
  z.strictObject({ role: MappedRoleSchema }),
  z.strictObject({ custom_role_id: z.string().min(1) }),
]);

export const ScimRefusalSchema = z.object({
  error: z.string(),
  code: ScimCodeSchema,
});
