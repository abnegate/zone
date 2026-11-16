import { POLICY_FIELDS, PolicyField, PolicyTier } from './schemas';
import type {
  PolicyField as PolicyFieldValue,
  PolicyListField,
  PolicyRules,
  PolicyTier as PolicyTierValue,
  UpdatePolicyRequest,
} from './types';

export type PolicyErrors = Partial<Record<PolicyFieldValue, string>>;

const ENTRY_SEPARATOR = /[\s,]+/;

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

export function parseEntries(text: string, field: PolicyListField): string[] {
  const entries = text
    .split(ENTRY_SEPARATOR)
    .map((entry) => entry.trim())
    .map((entry) => (field === PolicyField.Domains ? entry.toLowerCase() : entry))
    .filter((entry) => entry !== '');
  return [...new Set(entries)];
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
