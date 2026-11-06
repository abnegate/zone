import type { Handover, ToolCallRecord } from '../types';
import { agentChanged } from './handover';

export type TurnPart =
  | { kind: 'text'; text: string; offset: number }
  | { kind: 'calls'; calls: ToolCallRecord[]; reasoning?: string }
  | { kind: 'handover'; handover: Handover; agentChanged: boolean };

export interface Thinking {
  text: string;
  at?: number;
}

type Mark =
  | { at: number; rank: number; kind: 'call'; call: ToolCallRecord }
  | { at: number; rank: number; kind: 'thinking'; text: string }
  | { at: number; rank: number; kind: 'handover'; handover: Handover };

const hasText = (characters: string[], start: number, end: number): boolean =>
  characters.slice(start, end).join('').trim().length > 0;

function marksOf(
  characters: string[],
  calls: readonly ToolCallRecord[],
  handovers: readonly Handover[],
  thinking?: Thinking
): Mark[] {
  const marks: Mark[] = [];
  calls.forEach((call, index) => {
    const at = call.at ?? 0;
    const { reasoning, reasoning_at: thoughtAt } = call;
    if (reasoning && thoughtAt !== undefined && hasText(characters, thoughtAt, at)) {
      marks.push({ at: thoughtAt, rank: 2 * index, kind: 'thinking', text: reasoning });
      marks.push({
        at,
        rank: 2 * index + 1,
        kind: 'call',
        call: { ...call, reasoning: undefined },
      });
    } else {
      marks.push({ at, rank: 2 * index + 1, kind: 'call', call });
    }
  });
  for (const handover of handovers) {
    const rank = 2 * (handover.calls ?? 0) - 0.5;
    marks.push({ at: handover.at, rank, kind: 'handover', handover });
  }
  if (thinking?.text.trim()) {
    const at = thinking.at ?? calls.at(-1)?.at ?? 0;
    marks.push({ at, rank: 2 * calls.length, kind: 'thinking', text: thinking.text });
  }
  return marks.sort((left, right) => left.at - right.at || left.rank - right.rank);
}

/**
 * Offsets count code points, as the server does. Calls stored before offsets
 * were kept have none and open the turn, where every call used to be drawn.
 */
export function splitTurn(
  content: string,
  calls: readonly ToolCallRecord[],
  handovers: readonly Handover[],
  thinking?: Thinking
): TurnPart[] {
  if (!calls.length && !handovers.length && !thinking?.text.trim()) {
    return content.trim() ? [{ kind: 'text', text: content, offset: 0 }] : [];
  }
  const characters = Array.from(content);
  const parts: TurnPart[] = [];
  let start = 0;
  const pushText = (end: number): void => {
    const text = characters.slice(start, end).join('');
    if (text.trim()) parts.push({ kind: 'text', text, offset: start });
    start = end;
  };

  const think = (text: string): void => {
    const previous = parts.at(-1);
    if (previous?.kind === 'calls' && previous.reasoning === undefined) previous.reasoning = text;
    else parts.push({ kind: 'calls', calls: [], reasoning: text });
  };

  for (const mark of marksOf(characters, calls, handovers, thinking)) {
    pushText(Math.min(Math.max(mark.at, start), characters.length));
    const previous = parts.at(-1);
    const open = previous?.kind === 'calls' && previous.reasoning === undefined ? previous : null;
    if (mark.kind === 'handover') {
      if (mark.handover.reasoning?.trim()) think(mark.handover.reasoning);
      parts.push({
        kind: 'handover',
        handover: mark.handover,
        agentChanged: agentChanged(mark.handover),
      });
    } else if (mark.kind === 'thinking') {
      think(mark.text);
    } else if (open) {
      open.calls.push(mark.call);
    } else {
      parts.push({ kind: 'calls', calls: [mark.call] });
    }
  }
  pushText(characters.length);
  return parts;
}
