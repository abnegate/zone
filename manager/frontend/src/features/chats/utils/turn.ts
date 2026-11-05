import type { Handover, ToolCallRecord } from '../types';
import { agentChanged } from './handover';

export type TurnPart =
  | { kind: 'text'; text: string; offset: number }
  | { kind: 'calls'; calls: ToolCallRecord[] }
  | { kind: 'handover'; handover: Handover; agentChanged: boolean };

type Event =
  | { at: number; kind: 'call'; call: ToolCallRecord }
  | { at: number; kind: 'handover'; handover: Handover };

/**
 * A turn as it happened: its text cut wherever a tool was called or the answer
 * moved to another login. Offsets count code points, as the server does, so a
 * cut never halves a surrogate pair. A call stored before offsets were recorded
 * has none and opens the turn, which is where every call used to be drawn.
 */
export function splitTurn(
  content: string,
  calls: readonly ToolCallRecord[],
  handovers: readonly Handover[]
): TurnPart[] {
  const events: Event[] = [
    ...calls.map((call): Event => ({ at: call.at ?? 0, kind: 'call', call })),
    ...handovers.map((handover): Event => ({ at: handover.at, kind: 'handover', handover })),
  ].sort((left, right) => left.at - right.at);

  const characters = Array.from(content);
  const parts: TurnPart[] = [];
  let start = 0;
  const pushText = (end: number): void => {
    const text = characters.slice(start, end).join('');
    if (text.trim()) parts.push({ kind: 'text', text, offset: start });
    start = end;
  };

  for (const event of events) {
    pushText(Math.min(Math.max(event.at, start), characters.length));
    if (event.kind === 'handover') {
      parts.push({
        kind: 'handover',
        handover: event.handover,
        agentChanged: agentChanged(event.handover),
      });
      continue;
    }
    const previous = parts.at(-1);
    if (previous?.kind === 'calls') previous.calls.push(event.call);
    else parts.push({ kind: 'calls', calls: [event.call] });
  }
  pushText(characters.length);
  return parts;
}
