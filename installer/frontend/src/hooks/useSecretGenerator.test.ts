import { renderHook } from '@testing-library/react';
import { useSecretGenerator } from './useSecretGenerator';

describe('useSecretGenerator', () => {
  it('returns a generateSecret function', () => {
    const { result } = renderHook(() => useSecretGenerator());
    expect(typeof result.current.generateSecret).toBe('function');
  });

  it('generates a base64 encoded string', () => {
    const { result } = renderHook(() => useSecretGenerator());
    const secret = result.current.generateSecret();

    // Should be a valid base64 string
    expect(() => atob(secret)).not.toThrow();
  });

  it('generates unique secrets', () => {
    const { result } = renderHook(() => useSecretGenerator());

    const secret1 = result.current.generateSecret();
    const secret2 = result.current.generateSecret();

    expect(secret1).not.toBe(secret2);
  });

  it('generates secrets of consistent length', () => {
    const { result } = renderHook(() => useSecretGenerator());

    const secret1 = result.current.generateSecret();
    const secret2 = result.current.generateSecret();
    const secret3 = result.current.generateSecret();

    // All secrets should have the same length (32 bytes -> 44 chars in base64)
    expect(secret1.length).toBe(44);
    expect(secret2.length).toBe(44);
    expect(secret3.length).toBe(44);
  });

  it('generates secrets containing alphanumeric and special chars', () => {
    const { result } = renderHook(() => useSecretGenerator());
    const secret = result.current.generateSecret();

    // Base64 uses A-Za-z0-9+/=
    expect(secret).toMatch(/^[A-Za-z0-9+/=]+$/);
  });
});
