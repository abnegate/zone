import type { MessageMetadata } from '../types';

export interface AutomationSource {
  label: string;
  description: string;
  icon: string;
  detail?: (metadata: MessageMetadata) => string | undefined;
}

export interface Automation {
  source: AutomationSource;
  detail?: string;
}

const BELL = 'M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9M13.73 21a2 2 0 0 1-3.46 0';
const BOLT = 'M13 2 3 14h9l-1 8 10-12h-9l1-8z';

function reminder(metadata: MessageMetadata): string | undefined {
  const id = metadata.reminder_id;
  return typeof id === 'string' && id.trim() ? `Reminder ${id.trim()}.` : undefined;
}

export const AUTOMATION_SOURCES: ReadonlyMap<string, AutomationSource> = new Map([
  [
    'reminder',
    {
      label: 'Reminder',
      description: 'Sent by a reminder on its schedule, not typed in this chat.',
      icon: BELL,
      detail: reminder,
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

export function automation(metadata: MessageMetadata | null | undefined): Automation | undefined {
  if (!metadata || typeof metadata.source !== 'string') return undefined;
  const source = AUTOMATION_SOURCES.get(metadata.source);
  return source ? { source, detail: source.detail?.(metadata) } : undefined;
}
