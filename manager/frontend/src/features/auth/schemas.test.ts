import { describe, expect, it } from 'bun:test';
import { SessionsResponseSchema } from './schemas';

const session = {
  id: '56ab8307-0fc1-43bd-9997-0998c7c62951',
  ip_address: null,
  user_agent: null,
  device_info: null,
  last_active_at: '2026-09-20T13:43:51.508926Z',
  expires_at: '2026-09-27T13:43:51.504756Z',
  revoked_at: null,
  created_at: '2026-09-20T13:43:51.508926+00:00',
  updated_at: '2026-09-20T13:43:51.508926+00:00',
  deleted_at: null,
  is_current: false,
};

describe('SessionsResponseSchema', () => {
  it('reads the sessions the server lists without a user_id on each row', () => {
    const parsed = SessionsResponseSchema.parse({ sessions: [session] });
    expect(parsed.sessions).toHaveLength(1);
    expect(parsed.sessions[0]?.id).toBe(session.id);
    expect(parsed.sessions[0]?.user_id).toBeUndefined();
  });

  it('keeps a user_id when the server sends one', () => {
    const parsed = SessionsResponseSchema.parse({ sessions: [{ ...session, user_id: 'user-1' }] });
    expect(parsed.sessions[0]?.user_id).toBe('user-1');
  });
});
