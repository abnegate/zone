import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ApiError } from '../../../../api/ApiError';
import { chooseSelect, selectOptionValues } from '../../../../test/select';
import { AdministersServerProvider } from '../../components/AdministersServer';
import { RENEW_LINK } from '../../components/LapsedNotice';
import { LICENSE_LINK } from '../../components/UpgradeNotice';
import { AUDITOR, ACTIVE as ROLES_ACTIVE } from '../../roles/fixtures';
import {
  ACTIVE,
  ACTIVE_EMPTY,
  BASE_URL,
  COMMUNITY,
  COMPLIANCE,
  CONFLICT,
  CONTRACTORS,
  ENGINEERING,
  ENTRA,
  GRACE,
  ISSUED,
  LAPSED,
  OKTA,
  ORGANIZATION_ID,
  ROTATED,
  SECRET,
  TEAM_WITH_TOKENS,
} from '../fixtures';
import {
  CREATE_LABEL,
  DESCRIPTION,
  hintLabel,
  LAPSED_NOTICE,
  mappingLabel,
  NAME_REQUIRED,
  NEVER_SYNCED,
  NEVER_USED,
  NO_ERRORS,
  NO_GROUPS,
  NO_TOKENS_EDITABLE,
  REVOKE_CONFIRM,
  ROTATE_CONFIRM,
  revokeWarning,
  rotateWarning,
  secretTitle,
  TITLE,
  UPGRADE_NOTICE,
} from '../labels';
import { customRoleValue, UNMAPPED } from '../mapping';
import { refusalMessage } from '../messages';
import { ScimRefusal } from '../ScimRefusal';
import { MappedRole, ScimCode } from '../schemas';
import type { ScimSettings } from '../types';

const settings = mock();
const createToken = mock();
const rotateToken = mock();
const revokeToken = mock();
const mapGroup = mock();
const unmapGroup = mock();
const listRoles = mock();
const writeText = mock();

const CALLS = [settings, createToken, rotateToken, revokeToken, mapGroup, unmapGroup, listRoles];

mock.module('../api', () => ({
  scimApi: { settings, createToken, rotateToken, revokeToken, mapGroup, unmapGroup },
}));

mock.module('../../roles/api', () => ({
  rolesApi: { list: listRoles, create: mock(), update: mock(), remove: mock() },
}));

let ScimSection: typeof import('./ScimSection').ScimSection;

beforeAll(async () => {
  ({ ScimSection } = await import('./ScimSection'));
});

afterAll(() => {
  mock.restore();
});

function renderSection(): QueryClient {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <AdministersServerProvider value>
          <ScimSection organizationId={ORGANIZATION_ID} />
        </AdministersServerProvider>
      </MemoryRouter>
    </QueryClientProvider>
  );
  return queryClient;
}

async function loaded(response: ScimSettings): Promise<QueryClient> {
  settings.mockResolvedValue(response);
  const queryClient = renderSection();
  await screen.findByRole('heading', { name: TITLE, level: 2 });
  return queryClient;
}

function item(name: string): HTMLElement {
  const found = screen
    .getAllByRole('listitem')
    .find((candidate) => within(candidate).queryByText(name, { selector: '.scim-item-name' }));
  if (!found) throw new Error(`nothing listed as ${name}`);
  return found;
}

function dialog(): HTMLElement {
  return screen.getByRole('dialog');
}

function secretCard(): HTMLElement | null {
  return screen.queryByRole('region', { name: secretTitle(ISSUED.token.name) });
}

beforeEach(() => {
  for (const call of [...CALLS, writeText]) {
    call.mockReset();
  }
  listRoles.mockResolvedValue(ROLES_ACTIVE);
  writeText.mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
});

describe('ScimSection', () => {
  describe('states', () => {
    it('shows a loading state until the settings arrive', () => {
      settings.mockImplementation(() => new Promise(() => {}));
      renderSection();

      expect(screen.getByText('Loading SCIM provisioning...')).toBeInTheDocument();
    });

    it('offers a retry when the settings fail to load', async () => {
      settings
        .mockRejectedValueOnce(new ApiError('Failed to load SCIM provisioning: Forbidden', 403))
        .mockResolvedValueOnce(ACTIVE);
      renderSection();

      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('Failed to load SCIM provisioning: Forbidden');
      fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));

      await screen.findByText(OKTA.name, { selector: '.scim-item-name' });
      expect(settings).toHaveBeenCalledTimes(2);
    });

    it('describes what SCIM provisioning does', async () => {
      await loaded(ACTIVE);

      expect(screen.getByText(DESCRIPTION)).toBeInTheDocument();
    });
  });

  describe('license gating', () => {
    it('offers an upgrade on Community and shows nothing that needs a license', async () => {
      await loaded(COMMUNITY);

      const notice = screen.getByRole('note');
      expect(notice).toHaveTextContent(UPGRADE_NOTICE);
      expect(within(notice).getByRole('link', { name: LICENSE_LINK })).toBeInTheDocument();
      expect(screen.queryByText(BASE_URL)).toBeNull();
      expect(screen.queryByRole('button', { name: CREATE_LABEL })).toBeNull();
      expect(listRoles).not.toHaveBeenCalled();
    });

    it('lets a Team organization with tokens left over revoke them, and nothing else', async () => {
      await loaded(TEAM_WITH_TOKENS);

      expect(screen.getByRole('note')).toHaveTextContent(UPGRADE_NOTICE);
      expect(screen.queryByRole('button', { name: CREATE_LABEL })).toBeNull();
      expect(
        within(item(OKTA.name)).queryByRole('button', { name: `Rotate ${OKTA.name}` })
      ).toBeNull();
      expect(
        within(item(OKTA.name)).getByRole('button', { name: `Revoke ${OKTA.name}` })
      ).toBeEnabled();
      expect(
        screen.getByRole('combobox', { name: mappingLabel(ENGINEERING.display_name) })
      ).toBeDisabled();
    });

    it('keeps a lapsed organization read-only but lets it revoke tokens', async () => {
      await loaded(LAPSED);

      const notice = screen.getByRole('status');
      expect(notice).toHaveTextContent(LAPSED_NOTICE);
      expect(within(notice).getByRole('link', { name: RENEW_LINK })).toBeInTheDocument();
      expect(screen.queryByRole('note')).toBeNull();
      expect(screen.queryByRole('button', { name: CREATE_LABEL })).toBeNull();
      expect(screen.queryByRole('button', { name: `Rotate ${OKTA.name}` })).toBeNull();
      expect(screen.getByRole('button', { name: `Revoke ${OKTA.name}` })).toBeEnabled();
      for (const group of ACTIVE.groups) {
        expect(
          screen.getByRole('combobox', { name: mappingLabel(group.display_name) })
        ).toBeDisabled();
      }
      expect(listRoles).not.toHaveBeenCalled();
    });

    it('shows no notice in the grace period and keeps everything editable', async () => {
      await loaded(GRACE);

      expect(screen.queryByRole('note')).toBeNull();
      expect(screen.queryByRole('status')).toBeNull();
      expect(screen.getByRole('button', { name: CREATE_LABEL })).toBeEnabled();
    });
  });

  describe('base URL', () => {
    it('shows the base URL and copies it', async () => {
      await loaded(ACTIVE);

      expect(screen.getByText(BASE_URL)).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Copy base URL' }));

      await waitFor(() => expect(writeText).toHaveBeenCalledWith(BASE_URL));
      expect(await screen.findByRole('button', { name: 'Copied base URL' })).toBeInTheDocument();
    });
  });

  describe('tokens', () => {
    it('lists each token with its hint, when it was used and when it was rotated', async () => {
      await loaded(ACTIVE);

      const okta = item(OKTA.name);
      expect(okta).toHaveTextContent(hintLabel(OKTA.hint));
      expect(okta).toHaveTextContent('Last used');
      expect(okta).toHaveTextContent('Rotated');
      const entra = item(ENTRA.name);
      expect(entra).toHaveTextContent(hintLabel(ENTRA.hint));
      expect(entra).toHaveTextContent(NEVER_USED);
      expect(entra).not.toHaveTextContent('Rotated');
    });

    it('invites a first token when there are none', async () => {
      await loaded(ACTIVE_EMPTY);

      expect(screen.getByText(NO_TOKENS_EDITABLE)).toBeInTheDocument();
      expect(screen.getByText(NO_GROUPS)).toBeInTheDocument();
      expect(screen.getByText(NEVER_SYNCED)).toBeInTheDocument();
      expect(screen.getByText(NO_ERRORS)).toBeInTheDocument();
    });

    it('asks for a name before creating a token', async () => {
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: CREATE_LABEL }));
      fireEvent.submit(
        within(dialog()).getByRole('textbox', { name: 'Name' }).closest('form') as HTMLFormElement
      );

      expect(await within(dialog()).findByText(NAME_REQUIRED)).toBeInTheDocument();
      expect(createToken).not.toHaveBeenCalled();
    });

    it('shows a created token secret once, copies it and forgets it on dismiss', async () => {
      createToken.mockResolvedValue(ISSUED);
      const queryClient = await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: CREATE_LABEL }));
      fireEvent.change(within(dialog()).getByRole('textbox', { name: 'Name' }), {
        target: { value: ` ${ISSUED.token.name} ` },
      });
      fireEvent.submit(
        within(dialog()).getByRole('textbox', { name: 'Name' }).closest('form') as HTMLFormElement
      );

      const card = await waitFor(() => {
        const found = secretCard();
        if (!found) throw new Error('no secret shown');
        return found;
      });
      expect(createToken).toHaveBeenCalledWith(ORGANIZATION_ID, ISSUED.token.name);
      expect(screen.queryByRole('dialog')).toBeNull();
      expect(within(card).getByText(SECRET)).toBeInTheDocument();
      fireEvent.click(within(card).getByRole('button', { name: 'Copy token secret' }));
      await waitFor(() => expect(writeText).toHaveBeenCalledWith(SECRET));

      fireEvent.click(within(card).getByRole('button', { name: 'Dismiss' }));

      expect(secretCard()).toBeNull();
      expect(document.body.textContent).not.toContain(SECRET);
      const cachedQueries = queryClient
        .getQueryCache()
        .getAll()
        .map((query) => query.state.data);
      expect(JSON.stringify(cachedQueries)).not.toContain(SECRET);
      const cachedMutations = queryClient
        .getMutationCache()
        .getAll()
        .map((mutation) => mutation.state.data);
      expect(JSON.stringify(cachedMutations)).not.toContain(SECRET);
    });

    it('explains a refused token in the dialog', async () => {
      createToken.mockRejectedValue(
        new ScimRefusal({ error: 'Enterprise', code: ScimCode.LicenseRequired }, 403)
      );
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: CREATE_LABEL }));
      fireEvent.change(within(dialog()).getByRole('textbox', { name: 'Name' }), {
        target: { value: 'Okta' },
      });
      fireEvent.submit(
        within(dialog()).getByRole('textbox', { name: 'Name' }).closest('form') as HTMLFormElement
      );

      expect(await within(dialog()).findByRole('alert')).toHaveTextContent(
        refusalMessage(ScimCode.LicenseRequired)
      );
      expect(secretCard()).toBeNull();
    });

    it('rotates a token only after a confirm and shows the new secret once', async () => {
      rotateToken.mockResolvedValue(ROTATED);
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: `Rotate ${OKTA.name}` }));
      expect(within(dialog()).getByText(rotateWarning(OKTA.name))).toBeInTheDocument();
      expect(rotateToken).not.toHaveBeenCalled();
      fireEvent.click(within(dialog()).getByRole('button', { name: ROTATE_CONFIRM }));

      const card = await screen.findByRole('region', { name: secretTitle(OKTA.name) });
      expect(rotateToken).toHaveBeenCalledWith(ORGANIZATION_ID, OKTA.id);
      expect(within(card).getByText(SECRET)).toBeInTheDocument();
      expect(screen.queryByRole('dialog')).toBeNull();
    });

    it('still shows a rotated secret when the section left while the rotation was pending', async () => {
      let finish: (issued: typeof ROTATED) => void = () => {};
      rotateToken.mockImplementation(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          })
      );
      settings.mockResolvedValue(ACTIVE);
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
      });
      const page = (shown: boolean) => (
        <QueryClientProvider client={queryClient}>
          <MemoryRouter>
            <AdministersServerProvider value>
              {shown && <ScimSection organizationId={ORGANIZATION_ID} />}
            </AdministersServerProvider>
          </MemoryRouter>
        </QueryClientProvider>
      );
      const view = render(page(true));
      fireEvent.click(await screen.findByRole('button', { name: `Rotate ${OKTA.name}` }));
      fireEvent.click(within(dialog()).getByRole('button', { name: ROTATE_CONFIRM }));
      await waitFor(() => expect(rotateToken).toHaveBeenCalled());

      view.rerender(page(false));
      await act(async () => {
        finish(ROTATED);
        await new Promise((resolve) => setTimeout(resolve, 20));
      });
      view.rerender(page(true));

      const card = await screen.findByRole('region', { name: secretTitle(OKTA.name) });
      expect(within(card).getByText(SECRET)).toBeInTheDocument();
      fireEvent.click(within(card).getByRole('button', { name: 'Dismiss' }));
      expect(screen.queryByRole('region', { name: secretTitle(OKTA.name) })).toBeNull();
    });

    it('cancels a rotation without rotating', async () => {
      await loaded(ACTIVE);

      fireEvent.click(screen.getByRole('button', { name: `Rotate ${OKTA.name}` }));
      fireEvent.click(within(dialog()).getByRole('button', { name: 'Cancel' }));

      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
      expect(rotateToken).not.toHaveBeenCalled();
    });

    it('revokes a token only after a confirm', async () => {
      revokeToken.mockResolvedValue(undefined);
      settings.mockResolvedValueOnce(ACTIVE).mockResolvedValue({ ...ACTIVE, tokens: [ENTRA] });
      renderSection();
      await screen.findByText(OKTA.name, { selector: '.scim-item-name' });

      fireEvent.click(screen.getByRole('button', { name: `Revoke ${OKTA.name}` }));
      expect(within(dialog()).getByText(revokeWarning(OKTA.name))).toBeInTheDocument();
      fireEvent.click(within(dialog()).getByRole('button', { name: REVOKE_CONFIRM }));

      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
      expect(revokeToken).toHaveBeenCalledWith(ORGANIZATION_ID, OKTA.id);
      expect(screen.queryByText(OKTA.name, { selector: '.scim-item-name' })).toBeNull();
    });

    it('forgets a shown secret when its token is revoked', async () => {
      createToken.mockResolvedValue(ISSUED);
      revokeToken.mockResolvedValue(undefined);
      settings
        .mockResolvedValueOnce(ACTIVE)
        .mockResolvedValue({ ...ACTIVE, tokens: [...ACTIVE.tokens, ISSUED.token] });
      renderSection();
      await screen.findByText(OKTA.name, { selector: '.scim-item-name' });
      fireEvent.click(screen.getByRole('button', { name: CREATE_LABEL }));
      fireEvent.change(within(dialog()).getByRole('textbox', { name: 'Name' }), {
        target: { value: ISSUED.token.name },
      });
      fireEvent.submit(
        within(dialog()).getByRole('textbox', { name: 'Name' }).closest('form') as HTMLFormElement
      );
      await waitFor(() => expect(secretCard()).not.toBeNull());

      fireEvent.click(screen.getByRole('button', { name: `Revoke ${ISSUED.token.name}` }));
      fireEvent.click(within(dialog()).getByRole('button', { name: REVOKE_CONFIRM }));

      await waitFor(() => expect(secretCard()).toBeNull());
    });
  });

  describe('group mappings', () => {
    it('offers unmapped, admin, member and the organization custom roles', async () => {
      await loaded(ACTIVE);
      await waitFor(() => expect(listRoles).toHaveBeenCalledWith(ORGANIZATION_ID));

      await waitFor(() =>
        expect(selectOptionValues(mappingLabel(CONTRACTORS.display_name))).toEqual([
          UNMAPPED,
          MappedRole.Admin,
          MappedRole.Member,
          customRoleValue(AUDITOR.id),
        ])
      );
    });

    it('shows each group with its member count and current mapping', async () => {
      await loaded(ACTIVE);

      expect(item(ENGINEERING.display_name)).toHaveTextContent('42 members');
      expect(item(COMPLIANCE.display_name)).toHaveTextContent('1 member');
      expect(
        screen.getByRole('combobox', { name: mappingLabel(COMPLIANCE.display_name) })
      ).toHaveTextContent(AUDITOR.name);
      expect(
        screen.getByRole('combobox', { name: mappingLabel(CONTRACTORS.display_name) })
      ).toHaveTextContent('Unmapped');
    });

    it('maps a group to the chosen role', async () => {
      mapGroup.mockResolvedValue({ ...CONTRACTORS, role: MappedRole.Admin });
      await loaded(ACTIVE);

      chooseSelect(mappingLabel(CONTRACTORS.display_name), 'Admin');

      await waitFor(() =>
        expect(mapGroup).toHaveBeenCalledWith(ORGANIZATION_ID, CONTRACTORS.id, {
          role: MappedRole.Admin,
        })
      );
    });

    it('maps a group to a custom role', async () => {
      mapGroup.mockResolvedValue({
        ...CONTRACTORS,
        custom_role: { id: AUDITOR.id, name: AUDITOR.name },
      });
      await loaded(ACTIVE);
      await waitFor(() => expect(listRoles).toHaveBeenCalled());

      await waitFor(() => chooseSelect(mappingLabel(CONTRACTORS.display_name), /Auditor/));

      await waitFor(() =>
        expect(mapGroup).toHaveBeenCalledWith(ORGANIZATION_ID, CONTRACTORS.id, {
          custom_role_id: AUDITOR.id,
        })
      );
    });

    it('unmaps a group', async () => {
      unmapGroup.mockResolvedValue({ ...ENGINEERING, role: null });
      await loaded(ACTIVE);

      chooseSelect(mappingLabel(ENGINEERING.display_name), 'Unmapped');

      await waitFor(() => expect(unmapGroup).toHaveBeenCalledWith(ORGANIZATION_ID, ENGINEERING.id));
      expect(mapGroup).not.toHaveBeenCalled();
    });

    it('explains a refused mapping next to its group', async () => {
      mapGroup.mockRejectedValue(
        new ScimRefusal({ error: 'Gone', code: ScimCode.GroupNotFound }, 404)
      );
      await loaded(ACTIVE);

      chooseSelect(mappingLabel(CONTRACTORS.display_name), 'Member');

      expect(await within(item(CONTRACTORS.display_name)).findByRole('alert')).toHaveTextContent(
        refusalMessage(ScimCode.GroupNotFound)
      );
    });
  });

  describe('sync', () => {
    it('shows when the identity provider last called and its recent errors', async () => {
      await loaded(ACTIVE);

      expect(screen.getByText('Last request')).toBeInTheDocument();
      expect(screen.getByText(`${CONFLICT.method} ${CONFLICT.path}`)).toBeInTheDocument();
      expect(screen.getByText(CONFLICT.detail)).toBeInTheDocument();
      expect(screen.getAllByText(String(CONFLICT.status)).length).toBeGreaterThan(0);
    });
  });
});
