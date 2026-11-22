import { describe, expect, it } from 'bun:test';
import {
  memberCount,
  PERMISSION_LABELS,
  permissionCount,
  permissionDescription,
  permissionLabel,
  SCOPE_DESCRIPTIONS,
  SCOPE_LABELS,
} from './labels';
import { PERMISSIONS, Permission, ROLE_SCOPES } from './schemas';

describe('permission labels', () => {
  it('labels and describes every permission, and nothing else', () => {
    expect(Object.keys(PERMISSION_LABELS).sort()).toEqual([...PERMISSIONS].sort());
    for (const name of PERMISSIONS) {
      expect(permissionLabel(name).length, name).toBeGreaterThan(0);
      expect(permissionDescription(name), name).toMatch(/\.$/);
    }
  });

  it('gives every permission its own label', () => {
    const labels = PERMISSIONS.map(permissionLabel);
    expect(new Set(labels).size).toBe(labels.length);
  });

  it('says how far an invitation can seat people', () => {
    expect(permissionDescription(Permission.InvitationsCreate)).toContain(
      'member or viewer in any workspace of the organization'
    );
  });

  it('labels and describes both scopes', () => {
    for (const scope of ROLE_SCOPES) {
      expect(SCOPE_LABELS[scope].length).toBeGreaterThan(0);
      expect(SCOPE_DESCRIPTIONS[scope].length).toBeGreaterThan(0);
    }
  });
});

describe('counts', () => {
  it('counts in the singular only for one', () => {
    expect(permissionCount(1)).toBe('1 permission');
    expect(permissionCount(0)).toBe('0 permissions');
    expect(memberCount(1)).toBe('1 member');
    expect(memberCount(3)).toBe('3 members');
  });
});
