import { SERVER_NAME_LIMIT } from './constants';

export const NAME_REQUIRED = 'Enter a name';
export const NAME_CHARACTERS = 'Use only letters, digits, hyphens and underscores';
export const NAME_LENGTH = `Use at most ${SERVER_NAME_LIMIT} characters`;
export const NAME_SEPARATOR = 'Do not use two underscores in a row';
export const NAME_TRAILING = 'Do not end the name with an underscore';

export const URL_REQUIRED = 'Enter a URL';
export const URL_INVALID = 'Enter a valid URL';
export const URL_PROTOCOL = 'Use an http or https URL';
export const URL_HOST = 'Include a host in the URL';
export const URL_CREDENTIALS = 'Move the credentials out of the URL and into a header';
export const URL_FRAGMENT = 'Remove the # fragment from the URL';

export const REFERENCE = '${';
export const LITERAL = `Replace ${REFERENCE}…} references with the value`;

export const ENVIRONMENT_NAME =
  'Use letters, digits and underscores, starting with a letter or underscore';
export const HEADER_NAME = "Use a valid header name: letters, digits and !#$%&'*+-.^_`|~";

const NAME_PATTERN = /^[A-Za-z0-9_-]+$/;
const ENVIRONMENT_PATTERN = /^[A-Za-z_][A-Za-z0-9_]*$/;
const HEADER_PATTERN = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;
const PROTOCOLS: readonly string[] = ['http:', 'https:'];

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
  if (url.hostname === '') {
    return URL_HOST;
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

export function headerNameError(name: string): string | null {
  return HEADER_PATTERN.test(name) ? null : HEADER_NAME;
}
