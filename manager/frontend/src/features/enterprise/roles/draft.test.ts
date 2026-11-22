import { describe, expect, it } from 'bun:test';
import {
  changed,
  choices,
  createRequest,
  draftOf,
  emptyDraft,
  nameMissing,
  rescoped,
  toggled,
  updateRequest,
} from './draft';
import { AUDITOR, CATALOGUE, RELEASE_MANAGER } from './fixtures';
import { Permission, ROLE_BASE, RoleScope } from './schemas';

describe('choices', () => {
  it('offers only the composable permissions of the scope, in catalogue order', () => {
    const offered = choices(CATALOGUE, RoleScope.Workspace).map((entry) => entry.name);

    expect(offered).toEqual([
      Permission.WorkspaceWrite,
      Permission.WorkspaceUpdate,
      Permission.WorkspaceMembersCreate,
      Permission.WorkspaceMembersUpdate,
      Permission.WorkspaceMembersDelete,
      Permission.WorkspaceAiSettingsUpdate,
      Permission.WorkspaceMcpServersUpdate,
      Permission.WorkspacePoliciesUpdate,
      Permission.ProjectSyncsUpdate,
      Permission.PullRequestsOverride,
    ]);
  });

  it('never offers the base or an owner-only permission', () => {
    const offered = choices(CATALOGUE, RoleScope.Organization).map((entry) => entry.name);

    expect(offered).toHaveLength(20);
    expect(offered).toContain(Permission.SpendRead);
    expect(offered).toContain(Permission.SpendUpdate);
    for (const reserved of [
      Permission.OrganizationRead,
      Permission.OrganizationDelete,
      Permission.AdministratorsUpdate,
      Permission.RolesUpdate,
    ]) {
      expect(offered).not.toContain(reserved);
    }
  });
});

describe('drafts', () => {
  it('starts empty, as a workspace role', () => {
    expect(emptyDraft()).toEqual({
      name: '',
      description: '',
      scope: RoleScope.Workspace,
      permissions: [],
    });
  });

  it('reads a role without its base', () => {
    expect(draftOf(RELEASE_MANAGER)).toEqual({
      name: 'Release manager',
      description: 'Merges and syncs, nothing else.',
      scope: RoleScope.Workspace,
      permissions: [
        Permission.WorkspaceWrite,
        Permission.ProjectSyncsUpdate,
        Permission.PullRequestsOverride,
      ],
    });
    expect(draftOf(AUDITOR).description).toBe('');
  });

  it('toggles a permission on and off', () => {
    const on = toggled([Permission.WorkspaceWrite], Permission.WorkspaceUpdate, true);
    expect(on).toEqual([Permission.WorkspaceWrite, Permission.WorkspaceUpdate]);
    expect(toggled(on, Permission.WorkspaceWrite, false)).toEqual([Permission.WorkspaceUpdate]);
    expect(toggled(on, Permission.WorkspaceUpdate, true)).toEqual(on);
  });

  it('ticks the permission a chosen one requires and unticks what requires a cleared one', () => {
    const on = toggled([Permission.AuditLogsRead], Permission.SpendUpdate, true);
    expect(on).toEqual([Permission.AuditLogsRead, Permission.SpendRead, Permission.SpendUpdate]);
    expect(toggled(on, Permission.SpendRead, false)).toEqual([Permission.AuditLogsRead]);
    expect(toggled(on, Permission.SpendUpdate, false)).toEqual([
      Permission.AuditLogsRead,
      Permission.SpendRead,
    ]);
    expect(toggled([], Permission.SpendRead, true)).toEqual([Permission.SpendRead]);
  });

  it('drops the permissions another scope does not hold when the scope changes', () => {
    const draft = {
      ...emptyDraft(),
      permissions: [Permission.WorkspaceWrite, Permission.ProjectSyncsUpdate],
    };

    expect(rescoped(draft, RoleScope.Organization, CATALOGUE)).toEqual({
      ...draft,
      scope: RoleScope.Organization,
      permissions: [],
    });
    expect(rescoped(draft, RoleScope.Workspace, CATALOGUE)).toEqual(draft);
  });

  it('needs a name that is more than spaces', () => {
    expect(nameMissing({ ...emptyDraft(), name: '   ' })).toBe(true);
    expect(nameMissing({ ...emptyDraft(), name: 'Auditor' })).toBe(false);
  });
});

describe('createRequest', () => {
  it('trims the text, nulls an empty description and orders the permissions as the catalogue does', () => {
    expect(
      createRequest(
        {
          name: '  Auditor ',
          description: '  ',
          scope: RoleScope.Organization,
          permissions: [Permission.AuditUpdate, Permission.AuditLogsRead],
        },
        CATALOGUE
      )
    ).toEqual({
      name: 'Auditor',
      description: null,
      scope: RoleScope.Organization,
      permissions: [Permission.AuditLogsRead, Permission.AuditUpdate],
    });
  });

  it('leaves the base for the server to add', () => {
    const request = createRequest({ ...emptyDraft(), name: 'Reader' }, CATALOGUE);
    expect(request.permissions).not.toContain(ROLE_BASE[RoleScope.Workspace]);
  });
});

describe('updateRequest', () => {
  it('is empty while nothing changed', () => {
    const request = updateRequest(RELEASE_MANAGER, draftOf(RELEASE_MANAGER), CATALOGUE);

    expect(request).toEqual({});
    expect(changed(request)).toBe(false);
  });

  it('sends only what changed', () => {
    const draft = { ...draftOf(RELEASE_MANAGER), description: '' };

    expect(updateRequest(RELEASE_MANAGER, draft, CATALOGUE)).toEqual({ description: null });
  });

  it('treats the same permissions in another order as unchanged', () => {
    const draft = {
      ...draftOf(RELEASE_MANAGER),
      permissions: [...draftOf(RELEASE_MANAGER).permissions].reverse(),
    };

    expect(changed(updateRequest(RELEASE_MANAGER, draft, CATALOGUE))).toBe(false);
  });

  it('sends the whole new permission set and a trimmed name', () => {
    const draft = {
      ...draftOf(RELEASE_MANAGER),
      name: ' Releases ',
      permissions: [Permission.PullRequestsOverride, Permission.WorkspaceWrite],
    };

    expect(updateRequest(RELEASE_MANAGER, draft, CATALOGUE)).toEqual({
      name: 'Releases',
      permissions: [Permission.WorkspaceWrite, Permission.PullRequestsOverride],
    });
  });

  it('adds a description to a role that had none', () => {
    const draft = { ...draftOf(AUDITOR), description: 'Reads the log' };

    expect(updateRequest(AUDITOR, draft, CATALOGUE)).toEqual({ description: 'Reads the log' });
  });
});
