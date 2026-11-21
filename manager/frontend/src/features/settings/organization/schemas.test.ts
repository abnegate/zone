import { describe, expect, it } from 'bun:test';
import {
  AUDIT_ACTIONS,
  AUDIT_RESOURCE_TYPES,
  DevicesResponseSchema,
  InvitationSchema,
  InvitationsResponseSchema,
  OrganizationMemberSchema,
  OrganizationSchema,
  OrgMembersResponseSchema,
  UpdateOrgMemberRequestSchema,
} from './schemas';

const ORGANIZATION = {
  id: 'org-1',
  name: 'Acme',
  slug: 'acme',
  description: null,
  is_active: true,
  role: 'member',
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const MEMBER = {
  id: 'm1',
  organization_id: 'org-1',
  user_id: 'user-1',
  role: 'member',
  created_at: '2024-01-01T00:00:00Z',
};

describe('organization API schemas', () => {
  it('accepts members without email or display name', () => {
    const member = OrganizationMemberSchema.parse({
      id: 'm1',
      organization_id: 'org-1',
      user_id: 'user-1',
      role: 'owner',
      is_active: true,
      invited_by: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    });

    expect(member).toEqual({
      id: 'm1',
      organization_id: 'org-1',
      user_id: 'user-1',
      role: 'owner',
      custom_role: null,
      permissions: undefined,
      email: '',
      display_name: null,
      joined_at: '2024-01-01T00:00:00Z',
    });
    expect(member.permissions).toBeUndefined();
  });

  it('keeps the custom role and permissions a member holds', () => {
    const member = OrganizationMemberSchema.parse({
      ...MEMBER,
      custom_role: { id: 'role-1', name: 'Auditor' },
      permissions: ['organization:read', 'audit_logs:read'],
    });

    expect(member.custom_role).toEqual({ id: 'role-1', name: 'Auditor' });
    expect(member.permissions).toEqual(['organization:read', 'audit_logs:read']);
  });

  it('reads a member the server sends with a null custom role as holding a built-in role', () => {
    expect(OrganizationMemberSchema.parse({ ...MEMBER, custom_role: null }).custom_role).toBeNull();
  });

  it('leaves the permissions of an organization unknown when the server does not send them', () => {
    const organization = OrganizationSchema.parse(ORGANIZATION);

    expect(organization.permissions).toBeUndefined();
    expect(organization.custom_role).toBeUndefined();
  });

  it('keeps the permissions and custom role of an organization', () => {
    const organization = OrganizationSchema.parse({
      ...ORGANIZATION,
      custom_role: { id: 'role-1', name: 'Auditor' },
      permissions: ['organization:read', 'audit_logs:read'],
    });

    expect(organization.custom_role).toEqual({ id: 'role-1', name: 'Auditor' });
    expect(organization.permissions).toEqual(['organization:read', 'audit_logs:read']);
  });

  it('accepts a built-in role or a custom role id as a role change, never both or neither', () => {
    expect(UpdateOrgMemberRequestSchema.safeParse({ role: 'admin' }).success).toBe(true);
    expect(UpdateOrgMemberRequestSchema.safeParse({ custom_role_id: 'role-1' }).success).toBe(true);
    expect(
      UpdateOrgMemberRequestSchema.safeParse({ role: 'admin', custom_role_id: 'role-1' }).success
    ).toBe(false);
    expect(UpdateOrgMemberRequestSchema.safeParse({}).success).toBe(false);
    expect(UpdateOrgMemberRequestSchema.safeParse({ role: 'viewer' }).success).toBe(false);
    expect(UpdateOrgMemberRequestSchema.safeParse({ custom_role_id: '' }).success).toBe(false);
  });

  it('offers the role audit events as filters', () => {
    expect(AUDIT_ACTIONS).toContain('role.created');
    expect(AUDIT_ACTIONS).toContain('role.updated');
    expect(AUDIT_ACTIONS).toContain('role.deleted');
    expect(AUDIT_RESOURCE_TYPES).toContain('role');
  });

  it('accepts a members list wrapper', () => {
    const result = OrgMembersResponseSchema.parse({
      members: [
        {
          id: 'm1',
          organization_id: 'org-1',
          user_id: 'user-1',
          role: 'member',
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
        },
      ],
    });

    expect(result.members).toHaveLength(1);
  });

  it('accepts a devices list wrapper', () => {
    const result = DevicesResponseSchema.parse({
      devices: [
        {
          id: 'd1',
          user_id: 'user-1',
          email: 'jake@test.com',
          display_name: 'Jake',
          name: 'S23 Ultra',
          platform: 'android',
          user_agent: 'Zone/1',
          last_ip: '192.168.4.31',
          last_seen_at: '2026-10-09T12:00:00Z',
          status: 'allowed',
          connected: true,
          session_count: 1,
          created_at: '2026-10-09T11:00:00Z',
        },
      ],
    });
    expect(result.devices).toHaveLength(1);
    expect(result.devices[0].platform).toBe('android');
  });

  it('unwraps a raw invitations array', () => {
    const result = InvitationsResponseSchema.parse([
      {
        id: 'inv-1',
        email: 'person@example.com',
        organization_id: 'org-1',
        workspace_ids: ['ws-1'],
        org_role: 'member',
        workspace_role: 'member',
        invited_by: 'user-2',
        expires_at: '2024-12-31T00:00:00Z',
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-01T00:00:00Z',
      },
    ]);

    expect(result.invitations).toHaveLength(1);
    expect(InvitationSchema.parse(result.invitations[0])).toMatchObject({
      id: 'inv-1',
      email: 'person@example.com',
      workspace_id: 'ws-1',
      workspace_role: 'member',
      invited_by_email: '',
    });
  });
});
