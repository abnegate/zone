/**
 * Source Registry
 *
 * Each source type is defined in one place with all its metadata, icons, form fields, and config builders.
 * To add a new source type:
 * 1. Create a new file in this folder (e.g., mySource.ts)
 * 2. Export a SourceDefinition from it
 * 3. Import and add it to the `sources` array below
 */

import type { SourceCategory, SourceType } from '../types';
import { discordSource } from './discord';
import { filesystemSource } from './filesystem';
import { githubSource } from './github';
import { gitlabSource } from './gitlab';
import { icalSource } from './ical';
import { imapSource } from './imap';
import { slackSource } from './slack';
import { textSource } from './text';
import type { FormField, SourceDefinition } from './types';
import { webSource } from './web';

// Re-export types
export type { FormField, FormRow, SourceDefinition } from './types';

// Registry of all source types
export const sourceRegistry: SourceDefinition[] = [
  githubSource,
  gitlabSource,
  filesystemSource,
  icalSource,
  imapSource,
  webSource,
  textSource,
  discordSource,
  slackSource,
];

// Lookup helpers
export const getSourceById = (id: SourceType): SourceDefinition | undefined =>
  sourceRegistry.find((s) => s.id === id);

export const getSourcesByCategory = (category: SourceCategory): SourceDefinition[] =>
  sourceRegistry.filter((s) => s.category === category);

export const getEnabledSources = (): SourceDefinition[] => sourceRegistry.filter((s) => s.enabled);

// For SourceTypeBadge component
export const getSourceBadgeColor = (type: SourceType): string =>
  getSourceById(type)?.badgeColor ?? 'badge-gray';

export const getSourceLabel = (type: SourceType): string => getSourceById(type)?.name ?? type;

// Initialize form state for a source type
export const initializeFormState = (sourceId: SourceType): Record<string, unknown> => {
  const source = getSourceById(sourceId);
  if (!source) return {};

  const state: Record<string, unknown> = {};

  const processField = (field: FormField) => {
    if (field.defaultValue !== undefined) {
      state[field.id] = field.defaultValue;
    } else if (field.type === 'toggle') {
      state[field.id] = false;
    } else if (field.type === 'number') {
      state[field.id] = 0;
    } else {
      state[field.id] = '';
    }
  };

  for (const item of source.formFields) {
    if ('fields' in item) {
      item.fields.forEach(processField);
    } else {
      processField(item);
    }
  }

  if (source.credentialField) {
    state.credentials = '';
  }

  return state;
};
