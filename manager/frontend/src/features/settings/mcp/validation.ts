import { SERVER_NAME_LIMIT } from './constants';

export const NAME_REQUIRED = 'Enter a name';
export const NAME_CHARACTERS = 'Use only letters, digits, hyphens and underscores';
export const NAME_LENGTH = `Use at most ${SERVER_NAME_LIMIT} characters`;
export const NAME_SEPARATOR = 'Do not use two underscores in a row';
export const NAME_TRAILING = 'Do not end the name with an underscore';

export const URL_REQUIRED = 'Enter a URL';
export const URL_INVALID = 'Enter a valid URL';
export const URL_PROTOCOL = 'Use an http or https URL';
export const URL_METADATA = 'Do not use a link-local or cloud metadata address';
export const URL_CREDENTIALS = 'Move the credentials out of the URL and into a header';
export const URL_FRAGMENT = 'Remove the # fragment from the URL';

export const REFERENCE = '${';
export const LITERAL = `Replace ${REFERENCE}…} references with the value`;

export const ENVIRONMENT_NAME =
  'Use letters, digits and underscores, starting with a letter or underscore';
export const HEADER_NAME = "Use a valid header name: letters, digits and !#$%&'*+-.^_`|~";
export const HEADER_RESERVED = 'The connection sets this header itself';

export const RESERVED_HEADERS: readonly string[] = [
  'accept',
  'connection',
  'content-length',
  'content-type',
  'host',
  'keep-alive',
  'last-event-id',
  'mcp-method',
  'mcp-name',
  'mcp-protocol-version',
  'mcp-session-id',
  'metadata',
  'metadata-flavor',
  'proxy-authorization',
  'proxy-connection',
  'te',
  'trailer',
  'transfer-encoding',
  'upgrade',
  'x-aliyun-ecs-metadata-token',
  'x-aliyun-ecs-metadata-token-ttl-seconds',
  'x-aws-ec2-metadata-token',
  'x-aws-ec2-metadata-token-ttl-seconds',
  'x-google-metadata-request',
];
export const RESERVED_HEADER_PREFIX = 'mcp-param-';

const NAME_PATTERN = /^[A-Za-z0-9_-]+$/;
const ENVIRONMENT_PATTERN = /^[A-Za-z_][A-Za-z0-9_]*$/;
const HEADER_PATTERN = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;
const PROTOCOLS: readonly string[] = ['http:', 'https:'];

type Address = readonly number[];

const METADATA_HOSTS: readonly string[] = ['metadata.google.internal', 'metadata.goog'];
const METADATA_IPV4: readonly Address[] = [
  [169, 254],
  [100, 100, 100, 200],
  [168, 63, 129, 16],
];
const METADATA_IPV6: readonly Address[] = [
  [0x64, 0xff9b, 1],
  [0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254],
  [0xfd20, 0xce, 0, 0, 0, 0, 0, 0x254],
];
const LINK_LOCAL_IPV6 = { first: 0xfe80, last: 0xfebf } as const;
const EMBEDDED_IPV4: readonly Address[] = [
  [0x64, 0xff9b, 0, 0, 0, 0],
  [0, 0, 0, 0, 0, 0xffff],
  [0, 0, 0, 0, 0, 0],
];

const TRAILING_DOTS = /\.+$/;
const OCTET = '(0|[1-9]\\d{0,2})';
const IPV4 = new RegExp(`^${OCTET}\\.${OCTET}\\.${OCTET}\\.${OCTET}$`);
const IPV6 = /^\[([0-9a-f:]+)\]$/;
const OCTET_LIMIT = 255;
const OCTET_RANGE = 256;
const GROUPS = 8;
const HEXADECIMAL = 16;
const COMPRESSION = '::';
const SEPARATOR = ':';

function within(address: Address, blocks: readonly Address[]): boolean {
  return blocks.some((block) => block.every((part, index) => address[index] === part));
}

function ipv4(host: string): Address | null {
  const octets = IPV4.exec(host)?.slice(1).map(Number);
  return octets?.every((octet) => octet <= OCTET_LIMIT) ? octets : null;
}

function ipv6(host: string): Address | null {
  const sides = IPV6.exec(host)?.[1].split(COMPRESSION);
  if (sides === undefined || sides.length > 2) {
    return null;
  }
  const [head, tail = []] = sides.map((side) => (side === '' ? [] : side.split(SEPARATOR)));
  const zeros = sides.length === 2 ? Math.max(0, GROUPS - head.length - tail.length) : 0;
  const groups = [...head, ...Array<string>(zeros).fill('0'), ...tail];
  return groups.length === GROUPS
    ? groups.map((group) => Number.parseInt(group, HEXADECIMAL))
    : null;
}

function metadataIpv4(octets: Address): boolean {
  return within(octets, METADATA_IPV4);
}

function metadataIpv6(groups: Address): boolean {
  if (within(groups, EMBEDDED_IPV4)) {
    return metadataIpv4(
      groups.slice(-2).flatMap((group) => [Math.floor(group / OCTET_RANGE), group % OCTET_RANGE])
    );
  }
  const [first] = groups;
  return (
    (first >= LINK_LOCAL_IPV6.first && first <= LINK_LOCAL_IPV6.last) ||
    within(groups, METADATA_IPV6)
  );
}

function metadataHost(hostname: string): boolean {
  const host = hostname.replace(TRAILING_DOTS, '').toLowerCase();
  const octets = ipv4(host);
  if (octets !== null) {
    return metadataIpv4(octets);
  }
  const groups = ipv6(host);
  return groups === null ? METADATA_HOSTS.includes(host) : metadataIpv6(groups);
}

export function nameError(name: string): string | null {
  if (name === '') {
    return NAME_REQUIRED;
  }
  if (!NAME_PATTERN.test(name)) {
    return NAME_CHARACTERS;
  }
  if (name.length > SERVER_NAME_LIMIT) {
    return NAME_LENGTH;
  }
  if (name.includes('__')) {
    return NAME_SEPARATOR;
  }
  if (name.endsWith('_')) {
    return NAME_TRAILING;
  }
  return null;
}

export function urlError(raw: string): string | null {
  if (raw === '') {
    return URL_REQUIRED;
  }
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return URL_INVALID;
  }
  if (!PROTOCOLS.includes(url.protocol)) {
    return URL_PROTOCOL;
  }
  if (metadataHost(url.hostname)) {
    return URL_METADATA;
  }
  if (url.username !== '' || url.password !== '') {
    return URL_CREDENTIALS;
  }
  if (raw.includes('#')) {
    return URL_FRAGMENT;
  }
  return null;
}

export function literalError(value: string): string | null {
  return value.includes(REFERENCE) ? LITERAL : null;
}

export function environmentNameError(name: string): string | null {
  return ENVIRONMENT_PATTERN.test(name) ? null : ENVIRONMENT_NAME;
}

function reservedHeader(name: string): boolean {
  const header = name.toLowerCase();
  return RESERVED_HEADERS.includes(header) || header.startsWith(RESERVED_HEADER_PREFIX);
}

export function headerNameError(name: string): string | null {
  if (!HEADER_PATTERN.test(name)) {
    return HEADER_NAME;
  }
  return reservedHeader(name) ? HEADER_RESERVED : null;
}
