const WEB_PROTOCOLS = ['http:', 'https:'] as const;

type WebProtocol = (typeof WEB_PROTOCOLS)[number];

export const NOT_WEB_URL_MESSAGE = 'GitHub sent an address that is not an http or https URL';

function isWebProtocol(protocol: string): protocol is WebProtocol {
  return (WEB_PROTOCOLS as readonly string[]).includes(protocol);
}

export function isWebUrl(value: string): boolean {
  try {
    return isWebProtocol(new URL(value).protocol);
  } catch {
    return false;
  }
}

export function requireWebUrl(value: string): string {
  if (!isWebUrl(value)) {
    throw new Error(NOT_WEB_URL_MESSAGE);
  }
  return value;
}
