import type { SlackConfig } from '../types';
import { SlackIcon } from './icons';
import type { SourceDefinition } from './index';

export const slackSource: SourceDefinition = {
  id: 'slack',
  name: 'Slack',
  category: 'chat',
  description: 'Connect to a Slack workspace',
  icon: SlackIcon,
  badgeColor: 'badge-pink',
  iconWrapperClass: 'slack',
  enabled: false, // Not yet implemented

  formFields: [],

  buildConfig: (): SlackConfig => ({
    workspace_id: '',
  }),

  getDefaultName: () => 'Slack',

  getFieldIds: () => [],
};
