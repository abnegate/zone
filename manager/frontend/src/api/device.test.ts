import { afterEach, describe, expect, it, mock } from 'bun:test';
import {
  DEVICE_HEADER,
  DEVICE_NAME_HEADER,
  DEVICE_PLATFORM_HEADER,
  deviceHeaders,
  ensureDevice,
  resetDeviceForTests,
} from './device';

const originalFetch = global.fetch;
const mockFetch = mock();

afterEach(() => {
  resetDeviceForTests();
  global.fetch = originalFetch;
  mock.clearAllMocks();
});

describe('device identity', () => {
  it('mints a stable browser id', () => {
    const first = deviceHeaders();
    const second = deviceHeaders();
    expect(first[DEVICE_HEADER]).toEqual(second[DEVICE_HEADER]);
    expect(first[DEVICE_PLATFORM_HEADER]).toBe('browser');
    expect(first[DEVICE_NAME_HEADER]).toBe('Browser');
    expect(localStorage.getItem('manager_device_id')).toBe(first[DEVICE_HEADER]);
  });

  it('prefers the native client id', async () => {
    const id = '11111111-1111-4111-8111-111111111111';
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: async () => ({ client: true, device_id: id, platform: 'android' }),
    });
    global.fetch = mockFetch as typeof fetch;
    const identity = await ensureDevice();
    expect(identity).toEqual({ id, name: 'Android', platform: 'android' });
    expect(deviceHeaders()[DEVICE_HEADER]).toBe(id);
  });

  it('falls back to a browser id when the native client is absent', async () => {
    mockFetch.mockRejectedValueOnce(new Error('offline'));
    global.fetch = mockFetch as typeof fetch;
    const identity = await ensureDevice();
    expect(identity.platform).toBe('browser');
    expect(identity.id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
    );
  });
});
