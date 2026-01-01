import type { GitHubConfig } from '../types';
import { GitHubIcon } from './icons';
import type { SourceDefinition } from './index';

export const githubSource: SourceDefinition = {
  id: 'github',
  name: 'GitHub',
  category: 'file',
  description: 'Connect to a GitHub repository',
  icon: GitHubIcon,
  badgeColor: 'badge-purple',
  iconWrapperClass: 'github',
  enabled: true,

  formFields: [
    {
      fields: [
        { id: 'ghOwner', label: 'Owner', type: 'text', placeholder: 'org or user', required: true },
        {
          id: 'ghRepo',
          label: 'Repository',
          type: 'text',
          placeholder: 'repo-name',
          required: true,
        },
      ],
    },
    {
      fields: [
        {
          id: 'ghBranch',
          label: 'Branch',
          type: 'text',
          placeholder: 'main',
          defaultValue: 'main',
        },
      ],
    },
  ],

  credentialField: {
    id: 'credentials',
    label: 'Access Token',
    type: 'password',
    placeholder: 'ghp_xxxxx',
    hint: 'Token required for private repos and write access',
  },

  formHint: 'Token required for private repos and write access',

  buildConfig: (state): GitHubConfig => ({
    owner: state.ghOwner as string,
    repo: state.ghRepo as string,
    branch: (state.ghBranch as string) || 'main',
  }),

  getDefaultName: (state) => `${state.ghOwner}/${state.ghRepo}`,

  getFieldIds: () => ['ghOwner', 'ghRepo', 'ghBranch'],
};
