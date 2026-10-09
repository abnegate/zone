export function isPhoneReachableHost(host: string): boolean {
  const normalized = host.trim().replace(/\.+$/, '').toLowerCase();
  if (!normalized) {
    return false;
  }
  if (normalized === 'localhost' || normalized.endsWith('.localhost')) {
    return false;
  }
  const octets = ipv4(normalized);
  if (octets) {
    return octets[0] !== 127 && !octets.every((part) => part === 0);
  }
  return normalized.endsWith('.local');
}

export function phoneOrigin(scheme: string, host: string): string | null {
  const trimmed = host.trim();
  if (!trimmed) {
    return null;
  }
  let url: URL;
  try {
    url = new URL(`${scheme}://${trimmed}`);
  } catch {
    return null;
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    return null;
  }
  if (!isPhoneReachableHost(url.hostname)) {
    return null;
  }
  return url.origin;
}

function ipv4(host: string): [number, number, number, number] | null {
  const parts = host.split('.');
  if (parts.length !== 4) {
    return null;
  }
  const octets = parts.map((part) => {
    if (!/^\d{1,3}$/.test(part)) {
      return Number.NaN;
    }
    return Number(part);
  });
  if (octets.some((part) => !Number.isInteger(part) || part < 0 || part > 255)) {
    return null;
  }
  return octets as [number, number, number, number];
}
