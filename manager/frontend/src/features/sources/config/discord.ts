import type { DiscordConfig } from '../types';
import { DiscordIcon } from './icons';
import type { SourceDefinition } from './types';

export const discordSource: SourceDefinition = {
  id: 'discord',
  name: 'Discord',
  category: 'chat',
  description: 'Connect to a Discord server',
  icon: DiscordIcon,
  badgeColor: 'badge-indigo',
  iconWrapperClass: 'discord',
  enabled: false, // Not yet implemented

  formFields: [],

  buildConfig: (): DiscordConfig => ({
    server_id: '',
  }),

  getDefaultName: () => 'Discord',

  getFieldIds: () => [],
};
