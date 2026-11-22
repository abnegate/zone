import { Permission, type PermissionName, RoleScope } from './schemas';

export interface PermissionLabel {
  label: string;
  description: string;
}

export const PERMISSION_LABELS: Readonly<Record<PermissionName, PermissionLabel>> = {
  [Permission.OrganizationRead]: {
    label: 'See the organization',
    description: 'Open the organization and see its members and workspaces. Every role has this.',
  },
  [Permission.OrganizationUpdate]: {
    label: 'Edit the organization',
    description: 'Rename the organization and change its details.',
  },
  [Permission.OrganizationDelete]: {
    label: 'Delete the organization',
    description: 'Delete the organization and everything in it. Only owners can do this.',
  },
  [Permission.MembersCreate]: {
    label: 'Add members',
    description: 'Add people who already have a Zone account to the organization.',
  },
  [Permission.MembersUpdate]: {
    label: 'Change member roles',
    description: 'Give members another role, up to the permissions this role holds itself.',
  },
  [Permission.MembersDelete]: {
    label: 'Remove members',
    description: 'Remove members from the organization.',
  },
  [Permission.AdministratorsUpdate]: {
    label: 'Manage owners and admins',
    description: 'Promote, demote and remove owners and admins. Only owners can do this.',
  },
  [Permission.InvitationsCreate]: {
    label: 'Invite people',
    description:
      'Send invitations to join the organization. An invitation can seat people as a member or viewer in any workspace of the organization, even ones this role holder is not in.',
  },
  [Permission.InvitationsRead]: {
    label: 'See invitations',
    description: 'See the invitations waiting to be accepted.',
  },
  [Permission.InvitationsDelete]: {
    label: 'Cancel invitations',
    description: 'Cancel invitations before they are accepted.',
  },
  [Permission.WorkspacesCreate]: {
    label: 'Create workspaces',
    description: 'Create new workspaces in the organization.',
  },
  [Permission.DevicesRead]: {
    label: 'See devices',
    description: 'See the devices connected to the organization.',
  },
  [Permission.DevicesUpdate]: {
    label: 'Manage devices',
    description: "Change and disconnect the organization's devices.",
  },
  [Permission.GitHubUpdate]: {
    label: 'Manage GitHub',
    description: 'Connect GitHub and choose which installations the organization uses.',
  },
  [Permission.McpServersUpdate]: {
    label: 'Manage MCP servers',
    description: "Add, change and remove the organization's MCP servers.",
  },
  [Permission.AiSettingsRead]: {
    label: 'See AI defaults',
    description: "See the organization's AI providers and default models.",
  },
  [Permission.AiSettingsUpdate]: {
    label: 'Change AI defaults',
    description: "Change the organization's AI providers and default models.",
  },
  [Permission.AgentLoginsUpdate]: {
    label: 'Manage coding agent sign-ins',
    description: 'Connect and disconnect the Claude and ChatGPT sign-ins coding agents run on.',
  },
  [Permission.PoliciesUpdate]: {
    label: 'Change the agent policy',
    description: 'Choose which providers, models and coding agents the organization can use.',
  },
  [Permission.AuditLogsRead]: {
    label: 'Read the audit log',
    description: 'See who changed what in the organization.',
  },
  [Permission.AuditUpdate]: {
    label: 'Manage audit export',
    description: 'Change how long audit events are kept and where they are sent.',
  },
  [Permission.SpendRead]: {
    label: 'See spend',
    description: "See the organization's spend reports and budgets, and download spend as CSV.",
  },
  [Permission.SpendUpdate]: {
    label: 'Manage budgets',
    description: "Create, change and delete the organization's monthly budgets.",
  },
  [Permission.RolesUpdate]: {
    label: 'Manage roles',
    description: 'Create, edit and delete custom roles. Only owners and admins can do this.',
  },
  [Permission.WorkspaceRead]: {
    label: 'See the workspace',
    description: 'Open the workspace and read its chats, files and sources. Every role has this.',
  },
  [Permission.WorkspaceWrite]: {
    label: 'Work in the workspace',
    description: 'Start chats, run agents and change files, folders and sources.',
  },
  [Permission.WorkspaceUpdate]: {
    label: 'Edit the workspace',
    description: 'Rename the workspace and change its details.',
  },
  [Permission.WorkspaceDelete]: {
    label: 'Delete the workspace',
    description: 'Delete the workspace and everything in it. Only owners can do this.',
  },
  [Permission.WorkspaceMembersCreate]: {
    label: 'Add workspace members',
    description: 'Add organization members to the workspace.',
  },
  [Permission.WorkspaceMembersUpdate]: {
    label: 'Change workspace member roles',
    description:
      'Give workspace members another role, up to the permissions this role holds itself.',
  },
  [Permission.WorkspaceMembersDelete]: {
    label: 'Remove workspace members',
    description: 'Remove members from the workspace.',
  },
  [Permission.WorkspaceAdministratorsUpdate]: {
    label: 'Manage workspace owners and admins',
    description: 'Promote, demote and remove workspace owners and admins. Only owners can do this.',
  },
  [Permission.WorkspaceAiSettingsUpdate]: {
    label: 'Change workspace AI settings',
    description: "Change the workspace's AI providers and models.",
  },
  [Permission.WorkspaceMcpServersUpdate]: {
    label: 'Manage workspace MCP servers',
    description: "Add, change and remove the workspace's MCP servers.",
  },
  [Permission.WorkspacePoliciesUpdate]: {
    label: 'Narrow the agent policy',
    description: "Narrow the organization's agent policy for the workspace.",
  },
  [Permission.ProjectSyncsUpdate]: {
    label: 'Manage project syncs',
    description: 'Connect projects to GitHub or Linear and change those connections.',
  },
  [Permission.PullRequestsOverride]: {
    label: 'Merge as an admin',
    description:
      "Merge pull requests with GitHub's admin merge, past branch protection, when this server allows it.",
  },
};

export const SCOPE_LABELS: Readonly<Record<RoleScope, string>> = {
  [RoleScope.Organization]: 'Organization',
  [RoleScope.Workspace]: 'Workspace',
};

export const SCOPE_DESCRIPTIONS: Readonly<Record<RoleScope, string>> = {
  [RoleScope.Organization]: 'Assigned to organization members, in place of the member role.',
  [RoleScope.Workspace]:
    'Assigned to workspace members in any workspace of this organization, in place of the viewer role.',
};

export const TITLE = 'Roles';

export const DESCRIPTION =
  'Compose roles from individual permissions and assign them to members in place of a built-in role.';

export const LIST_TITLE = 'Custom roles';

export const CREATE_LABEL = 'Create role';

export const SAVE_LABEL = 'Save';

export const CANCEL_LABEL = 'Cancel';

export const DELETE_LABEL = 'Delete role';

export const EMPTY_EDITABLE =
  'No custom roles yet. Create one to grant a chosen set of permissions.';

export const EMPTY_READ_ONLY = 'No custom roles yet.';

export const LOADING = 'Loading roles...';

export const READ_ONLY_HINT = 'Only organization owners and admins can change roles.';

export const BASE_HINT = 'Every role has this.';

export const NAME_LABEL = 'Name';

export const DESCRIPTION_LABEL = 'Description';

export const DESCRIPTION_HINT = 'Optional. Shown next to the role when people assign it.';

export const SCOPE_LABEL = 'Applies to';

export const PERMISSIONS_LABEL = 'Permissions';

export const NAME_PLACEHOLDER = 'Release manager';

export const CREATE_TITLE = 'Create role';

export const DONE_LABEL = 'Done';

export const FIXED_SCOPE_HINT = "A role's scope can't change after it is created.";

export const META_SEPARATOR = ' · ';

export const UPGRADE_NOTICE =
  'Custom roles are part of Zone Enterprise. Add a license to compose roles from individual permissions and assign them to members.';

export const LAPSED_NOTICE =
  'Your Zone license has lapsed. Custom roles are still enforced; creating, editing and deleting them needs an active or grace license.';

export function permissionLabel(name: PermissionName): string {
  return PERMISSION_LABELS[name].label;
}

export function permissionDescription(name: PermissionName): string {
  return PERMISSION_LABELS[name].description;
}

function counted(count: number, singular: string, plural: string): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

export function permissionCount(count: number): string {
  return counted(count, 'permission', 'permissions');
}

export function memberCount(count: number): string {
  return counted(count, 'member', 'members');
}
