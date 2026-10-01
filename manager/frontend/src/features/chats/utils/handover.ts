import type { Handover, HandoverAgent, HandoverReason } from '../types';

export type AnswerPart =
  | { kind: 'text'; text: string; offset: number }
  | { kind: 'handover'; handover: Handover; agentChanged: boolean };

const AGENT_NAMES: Record<HandoverAgent, string> = { claude: 'Claude', codex: 'Codex' };

const CAUSES: Record<HandoverReason, string> = {
  limit: 'reached its usage limit',
  credits: 'ran out of credits',
  signed_out: 'was signed out',
};

const RESETTING: ReadonlySet<HandoverReason> = new Set(['limit', 'credits']);

const MINUTE = 60_000;
const MINUTES_PER_HOUR = 60;
const MINUTES_PER_DAY = 24 * MINUTES_PER_HOUR;

export function agentName(agent: HandoverAgent): string {
  return AGENT_NAMES[agent];
}

/**
 * Only the switch before this one knows which agent the turn was on. The first
 * switch has none, and a carried session file only ever moves between logins
 * of the same agent, so anything not carried may have changed agent.
 */
export function agentChanged(handover: Handover, previous?: Handover): boolean {
  return previous ? previous.agent !== handover.agent : !handover.carried;
}

export function appendHandover(existing: readonly Handover[], handover: Handover): Handover[] {
  const repeated = existing.some(
    (item) => item.at === handover.at && item.to === handover.to && item.from === handover.from
  );
  return repeated ? [...existing] : [...existing, handover];
}

/** Offsets count code points, as the server does, so the split never halves a surrogate pair. */
export function splitAtHandovers(content: string, handovers: readonly Handover[]): AnswerPart[] {
  const parts: AnswerPart[] = [];
  if (handovers.length === 0) {
    if (content.trim()) parts.push({ kind: 'text', text: content, offset: 0 });
    return parts;
  }
  const characters = Array.from(content);
  const pushText = (start: number, end: number): void => {
    const text = characters.slice(start, end).join('');
    if (text.trim()) parts.push({ kind: 'text', text, offset: start });
  };
  let start = 0;
  let previous: Handover | undefined;
  for (const handover of [...handovers].sort((left, right) => left.at - right.at)) {
    const end = Math.min(Math.max(handover.at, start), characters.length);
    pushText(start, end);
    parts.push({ kind: 'handover', handover, agentChanged: agentChanged(handover, previous) });
    start = end;
    previous = handover;
  }
  pushText(start, characters.length);
  return parts;
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
