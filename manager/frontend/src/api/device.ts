const STORAGE_KEY = 'manager_device_id';
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export const DEVICE_HEADER = 'X-Zone-Device';
export const DEVICE_NAME_HEADER = 'X-Zone-Device-Name';
export const DEVICE_PLATFORM_HEADER = 'X-Zone-Device-Platform';

export type DevicePlatform = 'android' | 'ios' | 'desktop' | 'browser' | 'cli';

export interface DeviceIdentity {
  id: string;
  name: string;
  platform: DevicePlatform;
}

let identity: DeviceIdentity | null = null;
let loading: Promise<DeviceIdentity> | null = null;
let nativeResolved = false;

function mintId(): string {
  return crypto.randomUUID();
}

function isUuid(value: string): boolean {
  return UUID.test(value);
}

function readStoredId(): string | null {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    return stored && isUuid(stored) ? stored : null;
  } catch {
    return null;
  }
}

function writeStoredId(id: string): void {
  try {
    localStorage.setItem(STORAGE_KEY, id);
  } catch {
    // Private mode can refuse storage; the in-memory identity still signs in.
  }
}

function fromStorage(): DeviceIdentity {
  const id = readStoredId() ?? mintId();
  writeStoredId(id);
  return { id, name: 'Browser', platform: 'browser' };
}

function platformOf(value: unknown): DevicePlatform {
  return value === 'android' || value === 'ios' || value === 'desktop' || value === 'cli'
    ? value
    : 'desktop';
}

async function fromNative(): Promise<DeviceIdentity | null> {
  if (typeof fetch !== 'function') {
    return null;
  }
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 150);
  try {
    const response = await fetch('/__zone/info', { signal: controller.signal });
    if (!response.ok) {
      return null;
    }
    const data: {
      client?: boolean;
      device_id?: string;
      platform?: string;
      name?: string;
    } | null = await response.json();
    if (!data?.client || typeof data.device_id !== 'string' || !isUuid(data.device_id)) {
      return null;
    }
    const platform = platformOf(data.platform);
    const name =
      typeof data.name === 'string' && data.name.trim()
        ? data.name.trim()
        : platform === 'android'
          ? 'Android'
          : platform === 'ios'
            ? 'iOS'
            : 'Desktop';
    writeStoredId(data.device_id);
    return { id: data.device_id, name, platform };
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

export async function ensureDevice(): Promise<DeviceIdentity> {
  if (identity && nativeResolved) {
    return identity;
  }
  if (!loading) {
    loading = (async () => {
      const native = await fromNative();
      nativeResolved = true;
      identity = native ?? identity ?? fromStorage();
      return identity;
    })();
  }
  return loading;
}

export function deviceHeaders(): Record<string, string> {
  const current = identity ?? fromStorage();
  identity = current;
  return {
    [DEVICE_HEADER]: current.id,
    [DEVICE_NAME_HEADER]: current.name,
    [DEVICE_PLATFORM_HEADER]: current.platform,
  };
}

export function resetDeviceForTests(): void {
  identity = null;
  loading = null;
  nativeResolved = false;
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // ignore
  }
}
