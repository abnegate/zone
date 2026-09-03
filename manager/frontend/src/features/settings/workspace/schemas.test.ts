import { describe, expect, it } from 'bun:test';
import { WorkspaceMemberSchema, WorkspaceMembersResponseSchema } from './schemas';

describe('workspace member API schemas', () => {
  it('accepts members without email or display name', () => {
    const member = WorkspaceMemberSchema.parse({
      id: 'm1',
      workspace_id: 'ws-1',
      user_id: 'user-1',
      role: 'admin',
      is_active: true,
      invited_by: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    });

    expect(member).toMatchObject({
      id: 'm1',
      workspace_id: 'ws-1',
      email: '',
      display_name: null,
      joined_at: '2024-01-01T00:00:00Z',
    });
  });

  it('accepts a members list wrapper', () => {
    const result = WorkspaceMembersResponseSchema.parse({
      members: [
        {
          id: 'm1',
          workspace_id: 'ws-1',
          user_id: 'user-1',
          role: 'viewer',
          created_at: '2024-01-01T00:00:00Z',
          updated_at: '2024-01-01T00:00:00Z',
        },
      ],
    });

    expect(result.members).toHaveLength(1);
  });
});
