import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ApiError } from '../../../../api/ApiError';
import { AdministersServerProvider } from '../../components/AdministersServer';
import { ASK_FOR_RENEWAL, RENEW_LINK } from '../../components/LapsedNotice';
import { ASK_FOR_LICENSE, LICENSE_LINK } from '../../components/UpgradeNotice';
import { LICENSE_PATH } from '../../licensePath';
import {
  ACTIVE,
  ACTIVE_EMPTY,
  ACTIVE_READ_ONLY,
  AUDITOR,
  COMMUNITY_EMPTY,
  COMMUNITY_WITH_ROLES,
  GRACE,
  LAPSED,
  ORGANIZATION_ID,
  RELEASE_MANAGER,
} from '../fixtures';
import {
  BASE_HINT,
  DESCRIPTION,
  DESCRIPTION_LABEL,
  EMPTY_EDITABLE,
  EMPTY_READ_ONLY,
  FIXED_SCOPE_HINT,
  LAPSED_NOTICE,
  permissionLabel,
  READ_ONLY_HINT,
  UPGRADE_NOTICE,
} from '../labels';
import { EXCEEDS, NAME_REQUIRED, refusalMessage } from '../messages';
import { RoleRefusal } from '../RoleRefusal';
import { Permission, RoleCode, RoleScope } from '../schemas';
import type { RolesResponse } from '../types';
import { deleteWarning } from './DeleteRoleDialog';

const list = mock();
const create = mock();
const update = mock();
const remove = mock();

mock.module('../api', () => ({ rolesApi: { list, create, update, remove } }));

let RolesSection: typeof import('./RolesSection').RolesSection;

beforeAll(async () => {
  ({ RolesSection } = await import('./RolesSection'));
});

afterAll(() => {
  mock.restore();
});

function renderSection(administersServer = true) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <AdministersServerProvider value={administersServer}>
          <RolesSection organizationId={ORGANIZATION_ID} />
        </AdministersServerProvider>
      </MemoryRouter>
    </QueryClientProvider>
  );
}

async function loaded(response: RolesResponse, administersServer = true) {
  list.mockResolvedValue(response);
  renderSection(administersServer);
  await screen.findByRole('heading', { name: 'Roles' });
}

function role(name: string): HTMLElement {
  const item = screen
    .getAllByRole('listitem')
    .find((candidate) => within(candidate).queryByText(name, { selector: '.roles-item-name' }));
  if (!item) throw new Error(`no role listed as ${name}`);
  return item;
}

function dialog(): HTMLElement {
  return screen.getByRole('dialog');
}

function permission(name: Parameters<typeof permissionLabel>[0]): HTMLElement {
  return within(dialog()).getByRole('checkbox', { name: permissionLabel(name) });
}

function type(label: string, value: string) {
  fireEvent.change(within(dialog()).getByLabelText(label), { target: { value } });
}

function submit(name: string) {
  fireEvent.click(within(dialog()).getByRole('button', { name }));
}

describe('RolesSection', () => {
  beforeEach(() => {
    for (const call of [list, create, update, remove]) {
      call.mockReset();
    }
  });

  describe('states', () => {
    it('shows a loading state until the roles arrive', () => {
      list.mockImplementation(() => new Promise(() => {}));
      renderSection();

      expect(screen.getByText('Loading roles...')).toBeInTheDocument();
    });

    it('offers a retry when the roles fail to load', async () => {
      list
        .mockRejectedValueOnce(new ApiError('Failed to load the roles: Forbidden', 403))
        .mockResolvedValueOnce(ACTIVE);
      renderSection();

      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('Failed to load the roles: Forbidden');
      fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));

      await screen.findByText('Auditor', { selector: '.roles-item-name' });
      expect(list).toHaveBeenCalledTimes(2);
    });

    it('describes what roles are for', async () => {
      await loaded(ACTIVE);

      expect(screen.getByText(DESCRIPTION)).toBeInTheDocument();
    });

    it('lets a screen reader read every listed role in full instead of naming it alone', async () => {
      await loaded(ACTIVE);

      const item = role(RELEASE_MANAGER.name);
      expect(item).not.toHaveAttribute('aria-label');
      expect(item).toHaveTextContent(RELEASE_MANAGER.description ?? '');
    });

    it('shows Community the upgrade notice with a way to the License page and no list', async () => {
      await loaded(COMMUNITY_EMPTY);

      const note = screen.getByRole('note');
      expect(note).toHaveTextContent(UPGRADE_NOTICE);
      expect(within(note).getByRole('link', { name: 'Go to License' })).toHaveAttribute(
        'href',
        LICENSE_PATH
      );
      expect(screen.queryByRole('list')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Create role' })).not.toBeInTheDocument();
    });

    it('asks an organization owner who does not administer the server to get a license installed', async () => {
      await loaded(COMMUNITY_EMPTY, false);

      const note = screen.getByRole('note');
      expect(note).toHaveTextContent(UPGRADE_NOTICE);
      expect(note).toHaveTextContent(ASK_FOR_LICENSE);
      expect(within(note).queryByRole('link', { name: LICENSE_LINK })).not.toBeInTheDocument();
      expect(screen.queryByRole('link')).not.toBeInTheDocument();
    });

    it('asks an organization owner who does not administer the server to get a lapsed license renewed', async () => {
      await loaded(LAPSED, false);

      const status = screen.getByRole('status');
      expect(status).toHaveTextContent(LAPSED_NOTICE);
      expect(status).toHaveTextContent(ASK_FOR_RENEWAL);
      expect(within(status).queryByRole('link', { name: RENEW_LINK })).not.toBeInTheDocument();
      expect(screen.queryByRole('link')).not.toBeInTheDocument();
    });

    it('shows Community the roles that already exist, read-only', async () => {
      await loaded(COMMUNITY_WITH_ROLES);

      expect(screen.getByRole('note')).toHaveTextContent(UPGRADE_NOTICE);
      const item = role('Release manager');
      expect(
        within(item).getByRole('button', { name: 'View Release manager' })
      ).toBeInTheDocument();
      expect(within(item).queryByRole('button', { name: /Edit|Delete/ })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Create role' })).not.toBeInTheDocument();
    });

    it('shows a lapsed license a renewal notice and a read-only list', async () => {
      await loaded(LAPSED);

      const status = screen.getByRole('status');
      expect(status).toHaveTextContent(LAPSED_NOTICE);
      expect(
        within(status).getByRole('link', { name: 'Renew on the License page' })
      ).toHaveAttribute('href', LICENSE_PATH);
      expect(screen.queryByRole('note')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Create role' })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /^Edit / })).not.toBeInTheDocument();
      expect(screen.getAllByRole('button', { name: /^View / })).toHaveLength(2);
    });

    it('keeps the list read-only for someone the server does not let edit', async () => {
      await loaded(ACTIVE_READ_ONLY);

      expect(screen.getByText(READ_ONLY_HINT)).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Create role' })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /^Delete / })).not.toBeInTheDocument();
    });

    it('lets a grace license edit', async () => {
      await loaded(GRACE);

      expect(screen.getByRole('button', { name: 'Create role' })).toBeInTheDocument();
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
    });

    it('invites the first role when there are none', async () => {
      await loaded(ACTIVE_EMPTY);

      expect(screen.getByText(EMPTY_EDITABLE)).toBeInTheDocument();
    });

    it('says plainly that there are none when nothing can be created', async () => {
      await loaded({ ...LAPSED, roles: [] });

      expect(screen.getByText(EMPTY_READ_ONLY)).toBeInTheDocument();
    });
  });

  describe('list', () => {
    it('shows each role with its scope, permission count and member count', async () => {
      await loaded(ACTIVE);

      const release = role('Release manager');
      expect(release).toHaveTextContent('Workspace');
      expect(release).toHaveTextContent('Merges and syncs, nothing else.');
      expect(release).toHaveTextContent('4 permissions · 3 members');

      const auditor = role('Auditor');
      expect(auditor).toHaveTextContent('Organization');
      expect(auditor).toHaveTextContent('2 permissions · 0 members');
    });
  });

  describe('editor', () => {
    it('creates a workspace role from the chosen permissions', async () => {
      const created = { ...RELEASE_MANAGER, id: 'created', name: 'Writer', members: 0 };
      create.mockResolvedValue(created);
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Create role' }));
      type('Name', ' Writer ');
      fireEvent.click(permission(Permission.WorkspaceWrite));
      fireEvent.click(permission(Permission.ProjectSyncsUpdate));
      submit('Create role');

      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(create).toHaveBeenCalledWith(ORGANIZATION_ID, {
        name: 'Writer',
        description: null,
        scope: RoleScope.Workspace,
        permissions: [Permission.WorkspaceWrite, Permission.ProjectSyncsUpdate],
      });
    });

    it('keeps the base permission checked and fixed', async () => {
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Create role' }));

      const base = permission(Permission.WorkspaceRead);
      expect(base).toBeChecked();
      expect(base).toBeDisabled();
      expect(within(dialog()).getByText(BASE_HINT)).toBeInTheDocument();
    });

    it('offers only composable permissions of the chosen scope', async () => {
      await loaded(ACTIVE);
      fireEvent.click(screen.getByRole('button', { name: 'Create role' }));

      expect(
        within(dialog()).queryByRole('checkbox', {
          name: permissionLabel(Permission.WorkspaceDelete),
        })
      ).not.toBeInTheDocument();
      expect(
        within(dialog()).queryByRole('checkbox', {
          name: permissionLabel(Permission.AuditLogsRead),
        })
      ).not.toBeInTheDocument();

      fireEvent.click(within(dialog()).getByRole('radio', { name: /Organization/ }));

      expect(permission(Permission.OrganizationRead)).toBeChecked();
      expect(permission(Permission.AuditLogsRead)).not.toBeChecked();
      for (const reserved of [
        Permission.OrganizationDelete,
        Permission.AdministratorsUpdate,
        Permission.RolesUpdate,
        Permission.WorkspaceWrite,
      ]) {
        expect(
          within(dialog()).queryByRole('checkbox', { name: permissionLabel(reserved) }),
          reserved
        ).not.toBeInTheDocument();
      }
    });

    it('explains how far an invitation reaches', async () => {
      await loaded(ACTIVE);
      fireEvent.click(screen.getByRole('button', { name: 'Create role' }));
      fireEvent.click(within(dialog()).getByRole('radio', { name: /Organization/ }));

      expect(dialog()).toHaveTextContent('member or viewer in any workspace of the organization');
    });

    it('asks for a name before creating', async () => {
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Create role' }));
      submit('Create role');

      expect(within(dialog()).getByText(NAME_REQUIRED)).toBeInTheDocument();
      expect(create).not.toHaveBeenCalled();
    });

    it('shows a taken name on the name field', async () => {
      create.mockRejectedValue(new RoleRefusal({ error: 'Taken', code: RoleCode.NameTaken }, 409));
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Create role' }));
      type('Name', 'Auditor');
      submit('Create role');

      expect(await within(dialog()).findByRole('alert')).toHaveTextContent(
        refusalMessage(RoleCode.NameTaken)
      );
      expect(within(dialog()).getByLabelText('Name')).toHaveAttribute('aria-invalid', 'true');
    });

    it('shows an unaccepted permission under the permissions', async () => {
      create.mockRejectedValue(
        new RoleRefusal({ error: 'No', code: RoleCode.PermissionNotComposable }, 400)
      );
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Create role' }));
      type('Name', 'Writer');
      submit('Create role');

      expect(await within(dialog()).findByRole('alert')).toHaveTextContent(
        refusalMessage(RoleCode.PermissionNotComposable)
      );
    });

    it('shows the upgrade notice when the license is refused', async () => {
      create.mockRejectedValue(
        new RoleRefusal({ error: 'License', code: RoleCode.LicenseRequired }, 403)
      );
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Create role' }));
      type('Name', 'Writer');
      submit('Create role');

      expect(await within(dialog()).findByRole('alert')).toHaveTextContent(UPGRADE_NOTICE);
    });

    it('opens a role with its saved values and a fixed scope', async () => {
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Edit Release manager' }));

      expect(within(dialog()).getByLabelText('Name')).toHaveValue('Release manager');
      expect(within(dialog()).getByLabelText('Description')).toHaveValue(
        'Merges and syncs, nothing else.'
      );
      expect(within(dialog()).queryByRole('radio')).not.toBeInTheDocument();
      expect(within(dialog()).getByText(FIXED_SCOPE_HINT)).toBeInTheDocument();
      expect(permission(Permission.WorkspaceWrite)).toBeChecked();
      expect(permission(Permission.WorkspaceUpdate)).not.toBeChecked();
      expect(within(dialog()).getByRole('button', { name: 'Save' })).toBeDisabled();
    });

    it('saves only what changed', async () => {
      update.mockResolvedValue({ ...RELEASE_MANAGER, description: null });
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Edit Release manager' }));
      type('Description', '');
      submit('Save');

      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(update).toHaveBeenCalledWith(ORGANIZATION_ID, RELEASE_MANAGER.id, {
        description: null,
      });
    });

    it('explains that only an organization owner can widen a held workspace role', async () => {
      update.mockRejectedValue(new RoleRefusal({ error: 'Exceeds', code: RoleCode.Exceeds }, 403));
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Edit Release manager' }));
      fireEvent.click(permission(Permission.WorkspaceUpdate));
      submit('Save');

      expect(await within(dialog()).findByRole('alert')).toHaveTextContent(EXCEEDS);
      expect(dialog()).toBeInTheDocument();
    });

    it('clears a failure once the draft changes', async () => {
      update.mockRejectedValue(new RoleRefusal({ error: 'Exceeds', code: RoleCode.Exceeds }, 403));
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Edit Release manager' }));
      fireEvent.click(permission(Permission.WorkspaceUpdate));
      submit('Save');
      await within(dialog()).findByRole('alert');

      fireEvent.click(permission(Permission.WorkspaceUpdate));

      await waitFor(() => expect(within(dialog()).queryByRole('alert')).toBeNull());
    });

    it('shows a read-only role without a way to save it', async () => {
      await loaded(LAPSED);

      fireEvent.click(screen.getByRole('button', { name: 'View Auditor' }));

      expect(within(dialog()).getByLabelText('Name')).toBeDisabled();
      expect(permission(Permission.AuditLogsRead)).toBeChecked();
      expect(permission(Permission.AuditLogsRead)).toBeDisabled();
      expect(within(dialog()).queryByRole('button', { name: 'Save' })).not.toBeInTheDocument();

      fireEvent.click(within(dialog()).getByRole('button', { name: 'Done' }));
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    });

    it('edits the description in a multi-line field so a long one is never cut off', async () => {
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Edit Release manager' }));

      const description = within(dialog()).getByRole('textbox', { name: DESCRIPTION_LABEL });
      expect(description.tagName).toBe('TEXTAREA');
      expect(description).toHaveValue(RELEASE_MANAGER.description ?? '');
    });

    it('shows a read-only description as text that wraps instead of a clipped field', async () => {
      const description =
        'Merges pull requests, syncs projects with their remotes, and overrides the merge gate when an incident needs it.';
      await loaded({ ...LAPSED, roles: [{ ...RELEASE_MANAGER, description }] });

      fireEvent.click(screen.getByRole('button', { name: `View ${RELEASE_MANAGER.name}` }));

      expect(
        within(dialog()).queryByRole('textbox', { name: DESCRIPTION_LABEL })
      ).not.toBeInTheDocument();
      expect(within(dialog()).getByText(DESCRIPTION_LABEL)).toBeInTheDocument();
      expect(within(dialog()).getByText(description).tagName).toBe('P');
    });

    it('leaves the description out of a read-only role that has none', async () => {
      await loaded(LAPSED);

      fireEvent.click(screen.getByRole('button', { name: 'View Auditor' }));

      expect(within(dialog()).queryByText(DESCRIPTION_LABEL)).not.toBeInTheDocument();
    });

    it('closes on cancel without saving', async () => {
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Create role' }));
      fireEvent.click(within(dialog()).getByRole('button', { name: 'Cancel' }));

      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(create).not.toHaveBeenCalled();
    });
  });

  describe('delete', () => {
    it('confirms before deleting', async () => {
      remove.mockResolvedValue(undefined);
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Delete Auditor' }));

      expect(dialog()).toHaveTextContent(deleteWarning(AUDITOR.name));
      expect(remove).not.toHaveBeenCalled();

      submit('Delete role');

      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(remove).toHaveBeenCalledWith(ORGANIZATION_ID, AUDITOR.id);
    });

    it('asks for the members of a role in use to be reassigned first', async () => {
      remove.mockRejectedValue(
        new RoleRefusal({ error: 'In use', code: RoleCode.InUse, members: 3 }, 409)
      );
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Delete Release manager' }));
      submit('Delete role');

      expect(await within(dialog()).findByRole('alert')).toHaveTextContent(
        'Reassign its 3 members first.'
      );
    });

    it('keeps the role when the delete is cancelled', async () => {
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: 'Delete Auditor' }));
      submit('Cancel');

      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(remove).not.toHaveBeenCalled();
      expect(role('Auditor')).toBeInTheDocument();
    });
  });
});
