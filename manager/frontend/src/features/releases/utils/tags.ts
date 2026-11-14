export const TagProblem = {
  Missing: 'Enter a tag',
  Whitespace: 'Tags cannot contain spaces',
  Characters: 'Tags cannot contain ~ ^ : ? * [ or \\',
  Dots: 'Tags cannot contain ..',
  Reflog: 'Tags cannot contain @{',
  Reserved: 'Tags cannot be @ or HEAD',
  Dash: 'Tags cannot start with -',
  Plus: 'Tags cannot start with +',
  Slashes: 'Tags cannot start or end with / or contain //',
  Component: 'Tag parts cannot start with . or end with .lock',
  Ending: 'Tags cannot end with .',
  Length: 'Tags cannot be longer than 255 bytes',
  Version: 'Tags must be a version such as v1.2.3, v1.2.3-rc.1 or pkg/v1.2.3',
} as const;

export type TagProblem = (typeof TagProblem)[keyof typeof TagProblem];

export interface ParsedTag {
  prefix: string | null;
  prerelease: boolean;
}

const WHITESPACE = /[ \t\n\v\f\r]/;
const FORBIDDEN = /[~^:?*[\\\p{Cc}]/u;
const RESERVED: readonly string[] = ['@', 'HEAD'];
const LOCK_SUFFIX = '.lock';
const SEPARATOR = '/';
const MARKER = 'v';
const VERSION_SEPARATOR = '.';
const PRERELEASE_SEPARATOR = '-';
const VERSION_PARTS = 3;
const MAXIMUM_BYTES = 255;
const NUMBER = /^(?:0|[1-9][0-9]*)$/;
const IDENTIFIER = /^[0-9A-Za-z-]+$/;
const LARGEST_NUMBER = '18446744073709551615';

const encoder = new TextEncoder();

export function tagExists(tag: string): string {
  return `${tag} already exists`;
}

function malformed(tag: string): TagProblem | null {
  if (WHITESPACE.test(tag)) return TagProblem.Whitespace;
  if (FORBIDDEN.test(tag)) return TagProblem.Characters;
  if (tag.includes('..')) return TagProblem.Dots;
  if (tag.includes('@{')) return TagProblem.Reflog;
  if (RESERVED.includes(tag)) return TagProblem.Reserved;
  if (tag.startsWith('-')) return TagProblem.Dash;
  if (tag.startsWith('+')) return TagProblem.Plus;
  const parts = tag.split(SEPARATOR);
  if (parts.some((part) => part.length === 0)) return TagProblem.Slashes;
  if (parts.some((part) => part.startsWith('.') || part.endsWith(LOCK_SUFFIX))) {
    return TagProblem.Component;
  }
  if (tag.endsWith('.')) return TagProblem.Ending;
  if (encoder.encode(tag).length > MAXIMUM_BYTES) return TagProblem.Length;
  return null;
}

function number(part: string): boolean {
  if (!NUMBER.test(part)) return false;
  return (
    part.length < LARGEST_NUMBER.length ||
    (part.length === LARGEST_NUMBER.length && part <= LARGEST_NUMBER)
  );
}

function identifiers(prerelease: string): boolean {
  return prerelease.split(VERSION_SEPARATOR).every((identifier) => IDENTIFIER.test(identifier));
}

function version(tag: string): ParsedTag | null {
  const split = tag.lastIndexOf(SEPARATOR);
  const prefix = split === -1 ? null : tag.slice(0, split);
  const versioned = split === -1 ? tag : tag.slice(split + 1);
  const unmarked = versioned.startsWith(MARKER) ? versioned.slice(MARKER.length) : versioned;
  const dash = unmarked.indexOf(PRERELEASE_SEPARATOR);
  const core = dash === -1 ? unmarked : unmarked.slice(0, dash);
  const prerelease = dash === -1 ? null : unmarked.slice(dash + 1);
  if (prerelease !== null && !identifiers(prerelease)) return null;
  const numbers = core.split(VERSION_SEPARATOR);
  if (numbers.length !== VERSION_PARTS || !numbers.every(number)) return null;
  return { prefix, prerelease: prerelease !== null };
}

export function parseTag(tag: string): ParsedTag | null {
  return malformed(tag) === null ? version(tag) : null;
}

export function tagProblem(tag: string, existing: readonly string[] = []): string | null {
  if (tag.trim().length === 0) return TagProblem.Missing;
  const problem = malformed(tag) ?? (version(tag) ? null : TagProblem.Version);
  return problem ?? (existing.includes(tag) ? tagExists(tag) : null);
}
