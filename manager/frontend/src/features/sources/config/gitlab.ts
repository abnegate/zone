import type { GitLabConfig } from '../types';
import { gitLabProjectUrl } from '../utils/gitLabProjectUrl';
import { GitLabIcon } from './icons';
import type { SourceDefinition } from './types';

export const gitlabSource: SourceDefinition = {
  id: 'gitlab',
  name: 'GitLab',
  category: 'file',
  description: 'GitLab project',
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

  fromConfig: (config) => {
    const gitlab = config as GitLabConfig;
    return {
      glProjectId: gitlab.project_id,
      glHost: gitlab.host || gitlab.base_url || 'https://gitlab.com',
      glBranch: gitlab.branch || 'main',
    };
  },

  getDefaultName: (state) => state.glProjectId as string,

  getUrl: (state) => {
    const project = state.glProjectId as string;
    if (!project) return undefined;
    return gitLabProjectUrl(state.glHost as string, project);
  },

  getFieldIds: () => ['glHost', 'glProjectId', 'glBranch'],
};
