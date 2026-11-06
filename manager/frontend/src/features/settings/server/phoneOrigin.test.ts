import { describe, expect, it } from 'bun:test';
import { isPhoneReachableHost, phoneOrigin } from './phoneOrigin';

describe('isPhoneReachableHost', () => {
  it('accepts LAN, emulator, Tailscale, and Bonjour names', () => {
    expect(isPhoneReachableHost('192.168.1.10')).toBe(true);
    expect(isPhoneReachableHost('10.0.2.2')).toBe(true);
    expect(isPhoneReachableHost('100.64.1.2')).toBe(true);
    expect(isPhoneReachableHost('jake-macbook.local')).toBe(true);
    expect(isPhoneReachableHost('JAKE-MACBOOK.LOCAL')).toBe(true);
  });

  it('rejects loopback and names a phone cannot resolve', () => {
    expect(isPhoneReachableHost('127.0.0.1')).toBe(false);
    expect(isPhoneReachableHost('0.0.0.0')).toBe(false);
    expect(isPhoneReachableHost('localhost')).toBe(false);
    expect(isPhoneReachableHost('manager.localhost')).toBe(false);
    expect(isPhoneReachableHost('manager.webui.localhost')).toBe(false);
    expect(isPhoneReachableHost('zone.example.com')).toBe(false);
  });
});

describe('phoneOrigin', () => {
  it('keeps http origins for a LAN host', () => {
    expect(phoneOrigin('http', '192.168.1.10')).toBe('http://192.168.1.10');
    expect(phoneOrigin('http', '192.168.1.10:80')).toBe('http://192.168.1.10');
  });

  it('drops manager.localhost', () => {
    expect(phoneOrigin('https', 'manager.localhost')).toBeNull();
  });
});
