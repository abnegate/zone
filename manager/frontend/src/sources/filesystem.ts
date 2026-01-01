import type { FilesystemConfig } from '../types';
import { FolderIcon } from './icons';
import type { SourceDefinition } from './index';

export const filesystemSource: SourceDefinition = {
  id: 'filesystem',
  name: 'Filesystem',
  category: 'file',
  description: 'Use a local directory',
  icon: FolderIcon,
  badgeColor: 'badge-blue',
  iconWrapperClass: 'filesystem',
  enabled: true,

  formFields: [
    {
      id: 'fsBasePath',
      label: 'Directory Path',
      type: 'text',
      placeholder: '/home/user/projects/my-app',
      required: true,
      monospace: true,
      hint: 'Absolute path to the project directory on the server',
    },
    {
      id: 'fsAllowWrites',
      label: 'Allow Writes',
      type: 'toggle',
      defaultValue: true,
      toggleTitle: 'Allow write operations',
      toggleDescription: 'Enable agents to modify files in this directory',
    },
  ],

  buildConfig: (state): FilesystemConfig => ({
    base_path: state.fsBasePath as string,
    allow_writes: state.fsAllowWrites as boolean,
  }),

  getDefaultName: (state) => {
    const path = state.fsBasePath as string;
    return path.split('/').pop() || path;
  },

  getFieldIds: () => ['fsBasePath', 'fsAllowWrites'],
};
