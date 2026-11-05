import type { Handover, HandoverAgent, HandoverReason } from '../types';

const AGENT_NAMES: Record<HandoverAgent, string> = { claude: 'Claude', codex: 'Codex' };

const CAUSES: Record<HandoverReason, string> = {
  limit: 'reached its usage limit',
  credits: 'ran out of credits',
  signed_out: 'was signed out',
  configured: 'gave way to the configured agent',
};

const RESETTING: ReadonlySet<HandoverReason> = new Set(['limit', 'credits']);

const MINUTE = 60_000;
const MINUTES_PER_HOUR = 60;
const MINUTES_PER_DAY = 24 * MINUTES_PER_HOUR;

export function agentName(agent: HandoverAgent): string {
  return AGENT_NAMES[agent];
}

export function agentChanged(handover: Handover): boolean {
  return handover.from_agent !== handover.agent;
}

export function appendHandover(existing: readonly Handover[], handover: Handover): Handover[] {
  const repeated = existing.some(
    (item) => item.at === handover.at && item.to === handover.to && item.from === handover.from
  );
  return repeated ? [...existing] : [...existing, handover];
}

export function resetsIn(resetsAt: string | undefined, now: number): string | undefined {
  if (!resetsAt) return undefined;
  const remaining = Date.parse(resetsAt) - now;
  if (!Number.isFinite(remaining) || remaining <= 0) return undefined;
  const total = Math.ceil(remaining / MINUTE);
  const days = Math.floor(total / MINUTES_PER_DAY);
  const hours = Math.floor((total % MINUTES_PER_DAY) / MINUTES_PER_HOUR);
  const minutes = total % MINUTES_PER_HOUR;
  if (days > 0) return hours > 0 ? `${days}d ${hours}h` : `${days}d`;
  if (hours > 0) return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
  return `${minutes}m`;
}

export function handoverTarget(handover: Handover, changed: boolean): string {
  return changed ? `${agentName(handover.agent)} · ${handover.to}` : handover.to;
}

export function handoverCause(handover: Handover, now: number): string {
  const cause = `${handover.from ?? 'the previous account'} ${CAUSES[handover.reason]}`;
  const reset = RESETTING.has(handover.reason) ? resetsIn(handover.resets_at, now) : undefined;
  return reset ? `${cause}; resets in ${reset}` : cause;
}
