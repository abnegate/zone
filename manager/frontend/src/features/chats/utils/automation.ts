import type { MessageMetadata } from '../types';

export interface AutomationSource {
  label: string;
  description: string;
  icon: string;
}

const BELL = 'M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9M13.73 21a2 2 0 0 1-3.46 0';
const BOLT = 'M13 2 3 14h9l-1 8 10-12h-9l1-8z';

/// Keyed by the server's `metadata.source`; a source missing here is not an automation.
export const AUTOMATION_SOURCES: ReadonlyMap<string, AutomationSource> = new Map([
  [
    'reminder',
    {
      label: 'Reminder',
      description: 'Posted by a reminder on its schedule, not typed here.',
      icon: BELL,
    },
  ],
  [
    'auto_project',
    {
      label: 'Auto project',
      description: 'Posted by an auto project, not typed here.',
      icon: BOLT,
    },
  ],
]);

export function automationSource(
  metadata: MessageMetadata | null | undefined
): AutomationSource | undefined {
  const source = metadata?.source;
  return typeof source === 'string' ? AUTOMATION_SOURCES.get(source) : undefined;
}
