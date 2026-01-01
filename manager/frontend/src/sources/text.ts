import type { TextConfig } from '../types';
import { DocumentIcon } from './icons';
import type { SourceDefinition } from './index';

export const textSource: SourceDefinition = {
  id: 'text',
  name: 'Text',
  category: 'text',
  description: 'Add raw text content',
  icon: DocumentIcon,
  badgeColor: 'badge-gray',
  iconWrapperClass: 'text',
  enabled: true,

  formFields: [
    {
      id: 'textLabel',
      label: 'Label',
      type: 'text',
      placeholder: 'My notes',
    },
    {
      id: 'textContent',
      label: 'Content',
      type: 'textarea',
      placeholder: 'Enter text content...',
      required: true,
    },
  ],

  buildConfig: (state): TextConfig => ({
    content: state.textContent as string,
    label: (state.textLabel as string) || undefined,
  }),

  getDefaultName: (state) => (state.textLabel as string) || 'Text content',

  getFieldIds: () => ['textLabel', 'textContent'],
};
