import type { GitLabConfig } from '../types';
import { GitLabIcon } from './icons';
import type { SourceDefinition } from './index';

export const gitlabSource: SourceDefinition = {
  id: 'gitlab',
  name: 'GitLab',
  category: 'file',
  description: 'Connect to a GitLab project',
  icon: GitLabIcon,
  badgeColor: 'badge-orange',
  iconWrapperClass: 'gitlab',
  enabled: true,

  formFields: [
    {
      id: 'glHost',
      label: 'GitLab Host',
      type: 'url',
      placeholder: 'https://gitlab.com',
      defaultValue: 'https://gitlab.com',
    },
    {
      fields: [
        {
          id: 'glProjectId',
          label: 'Project',
          type: 'text',
          placeholder: 'group/project',
          required: true,
        },
        {
          id: 'glBranch',
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
    placeholder: 'glpat-xxxxx',
    required: true,
  },

  buildConfig: (state): GitLabConfig => ({
    project_id: state.glProjectId as string,
    host: (state.glHost as string) || 'https://gitlab.com',
    branch: (state.glBranch as string) || 'main',
  }),

  getDefaultName: (state) => state.glProjectId as string,

  getFieldIds: () => ['glHost', 'glProjectId', 'glBranch'],
};
