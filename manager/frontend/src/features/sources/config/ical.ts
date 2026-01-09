import type { ICalConfig } from '../types';
import { CalendarIcon } from './icons';
import type { SourceDefinition } from './types';

export const icalSource: SourceDefinition = {
  id: 'ical',
  name: 'Calendar',
  category: 'calendar',
  description: 'Subscribe to a calendar feed',
  icon: CalendarIcon,
  badgeColor: 'badge-green',
  iconWrapperClass: 'ical',
  enabled: true,

  formFields: [
    {
      id: 'icalUrl',
      label: 'Calendar URL',
      type: 'url',
      placeholder: 'https://calendar.example.com/feed.ics',
      required: true,
      monospace: true,
      hint: 'URL to an iCal (.ics) calendar feed',
    },
  ],

  buildConfig: (state): ICalConfig => ({
    url: state.icalUrl as string,
  }),

  getDefaultName: (state) => {
    try {
      return new URL(state.icalUrl as string).hostname;
    } catch {
      return 'Calendar';
    }
  },

  getFieldIds: () => ['icalUrl'],
};
