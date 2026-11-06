import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { renderHook } from '@testing-library/react';

let granted: string[] = [];

mock.module('../../auth/context', () => ({
  useAuth: () => ({
    hasPermission: (permission: string) => granted.includes(permission),
  }),
}));

let useModelAccess: typeof import('./useModelAccess').useModelAccess;

beforeAll(async () => {
  useModelAccess = (await import('./useModelAccess')).useModelAccess;
});

afterAll(() => {
  mock.restore();
});

describe('useModelAccess', () => {
  beforeEach(() => {
    granted = [];
  });

  it('grants nothing on models:read alone', () => {
    granted = ['models:read'];
    const { result } = renderHook(() => useModelAccess());
    expect(result.current).toEqual({ canInstall: false, canRemove: false });
  });

  it('installs on models:create and removes on models:delete', () => {
    granted = ['models:create'];
    expect(renderHook(() => useModelAccess()).result.current).toEqual({
      canInstall: true,
      canRemove: false,
    });
    granted = ['models:delete'];
    expect(renderHook(() => useModelAccess()).result.current).toEqual({
      canInstall: false,
      canRemove: true,
    });
  });
});
