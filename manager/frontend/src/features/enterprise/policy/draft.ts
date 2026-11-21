import { POLICY_FIELDS, PolicyField, PolicyTier } from './schemas';
import type {
  PolicyField as PolicyFieldValue,
  PolicyListField,
  PolicyRules,
  PolicyTier as PolicyTierValue,
  UpdatePolicyRequest,
} from './types';

export type PolicyErrors = Partial<Record<PolicyFieldValue, string>>;

export const ENTRY_FIELDS = [
  PolicyField.Models,
  PolicyField.McpServers,
  PolicyField.Domains,
] as const;

export type PolicyEntryField = (typeof ENTRY_FIELDS)[number];

export type PolicyTexts = Partial<Record<PolicyEntryField, string>>;

const ENTRY_SEPARATOR = /[\s,]+/;

const WILDCARD = '*';

const DOMAIN_WILDCARD = '*.';

const DOMAIN_SEPARATOR = '.';

const IPV4 = /^\d{1,3}(\.\d{1,3}){3}$/;

const IPV6_SEPARATOR = ':';

const CEILING_RANKS: Readonly<Record<PolicyTierValue, number>> = {
  [PolicyTier.Read]: 0,
  [PolicyTier.Write]: 1,
  [PolicyTier.Host]: 2,
  [PolicyTier.Outward]: 3,
};

export const OPEN_RULES: PolicyRules = {
  providers: null,
  agents: null,
  models: null,
  ceiling: null,
  mcp_servers: null,
  domains: null,
};

function sameList(left: readonly string[] | null, right: readonly string[] | null): boolean {
  if (left === null || right === null) {
    return left === right;
  }
  const entries = new Set(left);
  return entries.size === new Set(right).size && right.every((entry) => entries.has(entry));
}

function same(field: PolicyFieldValue, saved: PolicyRules, draft: PolicyRules): boolean {
  return field === PolicyField.Ceiling
    ? saved.ceiling === draft.ceiling
    : sameList(saved[field], draft[field]);
}

export function patchFrom(saved: PolicyRules, draft: PolicyRules): UpdatePolicyRequest {
  const request: Record<string, unknown> = {};
  for (const field of POLICY_FIELDS) {
    if (!same(field, saved, draft)) {
      request[field] = draft[field];
    }
  }
  return request as UpdatePolicyRequest;
}

export function changed(request: UpdatePolicyRequest): boolean {
  return Object.keys(request).length > 0;
}

function hostOf(entry: string): string {
  const lowered = entry.toLowerCase();
  const host = lowered.startsWith(DOMAIN_WILDCARD)
    ? lowered.slice(DOMAIN_WILDCARD.length)
    : lowered;
  return host.endsWith(DOMAIN_SEPARATOR) ? host.slice(0, -DOMAIN_SEPARATOR.length) : host;
}

export function parseEntries(text: string, field: PolicyListField): string[] {
  const entries = text
    .split(ENTRY_SEPARATOR)
    .map((entry) => entry.trim())
    .map((entry) => (field === PolicyField.Domains ? hostOf(entry) : entry))
    .filter((entry) => entry !== '');
  return [...new Set(entries)];
}

function stem(pattern: string): string {
  return pattern.endsWith(WILDCARD) ? pattern.slice(0, -WILDCARD.length) : pattern;
}

function patternCovers(outer: string, inner: string): boolean {
  return outer.endsWith(WILDCARD) ? stem(inner).startsWith(stem(outer)) : outer === inner;
}

function literal(host: string): boolean {
  return IPV4.test(host) || host.includes(IPV6_SEPARATOR);
}

function domainCovers(outer: string, inner: string): boolean {
  return (
    inner === outer ||
    (!literal(outer) && !literal(inner) && inner.endsWith(`${DOMAIN_SEPARATOR}${outer}`))
  );
}

export function covers(field: PolicyListField, outer: string, inner: string): boolean {
  if (field === PolicyField.Models) {
    return patternCovers(outer, inner);
  }
  if (field === PolicyField.Domains) {
    return domainCovers(outer, inner);
  }
  return outer === inner;
}

export function uncovered(
  field: PolicyListField,
  limit: readonly string[] | null,
  entries: readonly string[]
): string[] {
  if (limit === null) {
    return [];
  }
  return entries.filter((entry) => !limit.some((outer) => covers(field, outer, entry)));
}

export function typed(texts: PolicyTexts): boolean {
  return ENTRY_FIELDS.some((field) => (texts[field] ?? '').trim() !== '');
}

export function withEntries(list: readonly string[], entries: readonly string[]): string[] {
  return [...new Set([...list, ...entries])];
}

export function withoutEntry(list: readonly string[], entry: string): string[] {
  return list.filter((current) => current !== entry);
}

export function toggled(list: readonly string[], entry: string, on: boolean): string[] {
  return on ? withEntries(list, [entry]) : withoutEntry(list, entry);
}

export function allows(limit: readonly string[] | null, entry: string): boolean {
  return limit === null || limit.includes(entry);
}

export function ceilingAllowed(
  limit: PolicyTierValue | null,
  ceiling: PolicyTierValue | null
): boolean {
  if (limit === null) {
    return true;
  }
  return ceiling !== null && CEILING_RANKS[ceiling] <= CEILING_RANKS[limit];
}
