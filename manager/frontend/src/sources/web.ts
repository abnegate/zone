import type { WebConfig } from '../types';
import { GlobeIcon } from './icons';
import type { SourceDefinition } from './index';

export const webSource: SourceDefinition = {
  id: 'web',
  name: 'Web URL',
  category: 'web',
  description: 'Fetch content from a URL',
  icon: GlobeIcon,
  badgeColor: 'badge-cyan',
  iconWrapperClass: 'web',
  enabled: true,

  formFields: [
    {
      id: 'webUrl',
      label: 'URL',
      type: 'url',
      placeholder: 'https://example.com/data.json',
      required: true,
      monospace: true,
      hint: 'URL to fetch content from',
    },
  ],

  buildConfig: (state): WebConfig => ({
    url: state.webUrl as string,
  }),

  getDefaultName: (state) => {
    try {
      return new URL(state.webUrl as string).hostname;
    } catch {
      return 'Web URL';
    }
  },

  getFieldIds: () => ['webUrl'],
};
