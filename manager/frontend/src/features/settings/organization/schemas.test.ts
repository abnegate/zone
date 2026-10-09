import { describe, expect, it } from 'bun:test';
import {
  DevicesResponseSchema,
  InvitationSchema,
  InvitationsResponseSchema,
  OrganizationMemberSchema,
  OrgMembersResponseSchema,
} from './schemas';

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
      email: '',
      display_name: null,
      joined_at: '2024-01-01T00:00:00Z',
    });
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
