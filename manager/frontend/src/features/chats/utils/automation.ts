import type { MessageMetadata } from '../types';

interface AutomationKind {
  label: string;
  description: string;
  icon: string;
  describe?: (metadata: MessageMetadata) => string | undefined;
}

export interface Automation {
  kind: AutomationKind;
  detail?: string;
}

const BELL = 'M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9M13.73 21a2 2 0 0 1-3.46 0';
const BOLT = 'M13 2 3 14h9l-1 8 10-12h-9l1-8z';

function reminderDetail(metadata: MessageMetadata): string | undefined {
  const id = metadata.reminder_id;
  return typeof id === 'string' && id.trim() ? `Reminder ${id.trim()}.` : undefined;
}

const AUTOMATION_KINDS: ReadonlyMap<string, AutomationKind> = new Map([
  [
    'reminder',
    {
      label: 'Reminder',
      description: 'Sent by a reminder on its schedule, not typed in this chat.',
      icon: BELL,
      describe: reminderDetail,
    },
  ],
  [
    'auto_project',
    {
      label: 'Auto project',
      description: 'Sent by an auto project, not typed in this chat.',
      icon: BOLT,
    },
  ],
]);

function sourceOf(metadata: MessageMetadata | null | undefined): string | undefined {
  const source = metadata?.source;
  return typeof source === 'string' && source !== '' ? source : undefined;
}

export function automated(metadata: MessageMetadata | null | undefined): boolean {
  return sourceOf(metadata) !== undefined;
}

export function automation(metadata: MessageMetadata | null | undefined): Automation | undefined {
  const source = sourceOf(metadata);
  const kind = source === undefined ? undefined : AUTOMATION_KINDS.get(source);
  if (!metadata || !kind) return undefined;
  return { kind, detail: kind.describe?.(metadata) };
}
