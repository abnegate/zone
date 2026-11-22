import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ApiError } from '../../../../api/ApiError';
import { LicenseStatus } from '../../schemas';
import {
  COMMUNITY_EMPTY,
  ENFORCED,
  ENTITLED,
  ENTITLED_EMPTY,
  ENTITLED_WITH_SAML,
  ENTRA,
  KEYCLOAK,
  LAPSED_WITH_PROVIDER,
  OKTA,
  REDIRECT_URI,
  SERVICE_PROVIDER,
  TEAM_ACTIVE,
  TEAM_LAPSED,
} from '../fixtures';
import { refusalMessage } from '../messages';
import { SsoRefusal } from '../SsoRefusal';
import { SsoAdminCode } from '../schemas';
import type { SettingsResponse } from '../types';
import { SERVICE_PROVIDER_TITLE } from './ServiceProviderDetails';
import {
  ENFORCED_READ_ONLY,
  ENFORCEMENT_BLOCKED,
  ENFORCEMENT_DESCRIPTION,
  ENFORCEMENT_LABEL,
} from './SsoEnforcement';
import { LAPSED_NOTICE } from './SsoLapsedNotice';
import { UPGRADE_NOTICE } from './SsoUpgradeNotice';

const get = mock();
const createProvider = mock();
const updateProvider = mock();
const deleteProvider = mock();
const setEnforcement = mock();
const writeText = mock();

mock.module('../api', () => ({
  ssoApi: { get, createProvider, updateProvider, deleteProvider, setEnforcement },
}));

let SsoSection: typeof import('./SsoSection').SsoSection;

beforeAll(async () => {
  ({ SsoSection } = await import('./SsoSection'));
});

afterAll(() => {
  mock.restore();
});

function renderSection() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <SsoSection />
      </MemoryRouter>
    </QueryClientProvider>
  );
}

async function loaded(settings: SettingsResponse) {
  get.mockResolvedValue(settings);
  renderSection();
  await screen.findByRole('heading', { name: 'Single sign-on' });
}

function enforcement(): HTMLElement {
  return screen.getByRole('switch', { name: ENFORCEMENT_LABEL });
}

function provider(name: string): HTMLElement {
  return screen.getByRole('listitem', { name });
}

function providerNames(): string[] {
  return screen
    .queryAllByRole('listitem', { hidden: true })
    .map((item) => item.getAttribute('aria-label') ?? '');
}

describe('SsoSection', () => {
  beforeEach(() => {
    for (const call of [get, createProvider, updateProvider, deleteProvider, setEnforcement]) {
      call.mockReset();
    }
    writeText.mockReset();
    writeText.mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  });

  describe('states', () => {
    it('shows a loading state until the settings arrive', () => {
      get.mockImplementation(() => new Promise(() => {}));
      renderSection();

      expect(screen.getByText('Loading single sign-on...')).toBeInTheDocument();
      expect(screen.queryByRole('switch')).not.toBeInTheDocument();
    });

    it('offers a retry when the settings fail to load', async () => {
      get
        .mockRejectedValueOnce(
          new ApiError('Failed to load single sign-on: This needs a server admin.', 403)
        )
        .mockResolvedValueOnce(ENTITLED);
      renderSection();

      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('Failed to load single sign-on: This needs a server admin.');
      fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));

      await screen.findByRole('listitem', { name: 'Okta' });
      expect(get).toHaveBeenCalledTimes(2);
    });

    it('shows Community the upgrade notice with a way to the License page and nothing to change', async () => {
      await loaded(COMMUNITY_EMPTY);

      expect(screen.getByText(UPGRADE_NOTICE)).toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'Go to License' })).toHaveAttribute(
        'href',
        '/settings/server/license'
      );
      expect(screen.queryByRole('switch')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Add provider' })).not.toBeInTheDocument();
      expect(screen.queryByText(REDIRECT_URI)).not.toBeInTheDocument();
      expect(screen.queryByText(SERVICE_PROVIDER_TITLE)).not.toBeInTheDocument();
      expect(screen.queryByText('Identity providers')).not.toBeInTheDocument();
    });

    it('lists the providers Community still has, read-only', async () => {
      await loaded({ ...COMMUNITY_EMPTY, providers: [OKTA] });

      expect(screen.getByText(UPGRADE_NOTICE)).toBeInTheDocument();
      expect(providerNames()).toEqual(['Okta']);
      expect(within(provider('Okta')).queryByRole('button')).not.toBeInTheDocument();
    });

    it('warns that a lapsed license keeps sign-in working but stops changes, all read-only', async () => {
      await loaded(LAPSED_WITH_PROVIDER);

      const warning = screen.getByRole('status');
      expect(warning).toHaveClass('alert-warning');
      expect(warning).toHaveTextContent(LAPSED_NOTICE);
      expect(within(warning).getByRole('link')).toHaveAttribute('href', '/settings/server/license');
      expect(screen.queryByText(UPGRADE_NOTICE)).not.toBeInTheDocument();
      expect(screen.getByText(ENFORCED_READ_ONLY)).toBeInTheDocument();
      expect(screen.queryByRole('switch')).not.toBeInTheDocument();
      expect(providerNames()).toEqual(['Okta']);
      expect(screen.queryByRole('button', { name: /Edit|Delete|Add provider/ })).toBeNull();
      expect(screen.queryByText(REDIRECT_URI)).not.toBeInTheDocument();
      expect(screen.queryByText(SERVICE_PROVIDER_TITLE)).not.toBeInTheDocument();
    });

    it('shows a Team server the Enterprise upgrade notice, since Team does not include single sign-on', async () => {
      await loaded(TEAM_ACTIVE);

      expect(screen.getByText(UPGRADE_NOTICE)).toBeInTheDocument();
      expect(UPGRADE_NOTICE).toContain('Zone Enterprise');
      expect(screen.queryByText(LAPSED_NOTICE)).not.toBeInTheDocument();
      expect(screen.queryByRole('switch')).not.toBeInTheDocument();
    });

    it('shows a lapsed Team license the upgrade notice instead of asking it to renew', async () => {
      await loaded(TEAM_LAPSED);

      expect(screen.getByText(UPGRADE_NOTICE)).toBeInTheDocument();
      expect(screen.queryByText(LAPSED_NOTICE)).not.toBeInTheDocument();
      expect(providerNames()).toEqual(['Okta']);
    });

    it('treats a grace period as entitled', async () => {
      await loaded({ ...ENTITLED, status: LicenseStatus.Grace });

      expect(screen.getByRole('button', { name: 'Add provider' })).toBeInTheDocument();
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
    });

    it('shows an entitled server with no providers the redirect URI and a blocked requirement', async () => {
      await loaded(ENTITLED_EMPTY);

      expect(screen.getByText(REDIRECT_URI)).toBeInTheDocument();
      expect(screen.getByText(/No identity providers yet/)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Add provider' })).toBeEnabled();
      expect(enforcement()).toBeDisabled();
      expect(enforcement()).toHaveAttribute('aria-checked', 'false');
      expect(screen.getByText(ENFORCEMENT_BLOCKED)).toBeInTheDocument();
      expect(screen.getByText(ENFORCEMENT_DESCRIPTION)).toBeInTheDocument();
      expect(screen.queryByText(UPGRADE_NOTICE)).not.toBeInTheDocument();
    });

    it('lists each provider with its state and actions', async () => {
      await loaded(ENTITLED);

      expect(providerNames()).toEqual(['Okta', 'Microsoft Entra ID']);
      const okta = provider('Okta');
      expect(okta).toHaveTextContent('Enabled');
      expect(within(okta).getByText('OIDC')).toBeInTheDocument();
      expect(okta).toHaveTextContent(OKTA.issuer);
      expect(okta).toHaveTextContent('Domains: acme.com');
      expect(okta).toHaveTextContent('Verified email required');
      const entra = provider('Microsoft Entra ID');
      expect(entra).toHaveTextContent('Disabled');
      expect(entra).toHaveTextContent('Any email domain');
      expect(entra).toHaveTextContent('Provider emails trusted');
      expect(within(okta).getByRole('button', { name: 'Edit Okta' })).toBeInTheDocument();
      expect(within(okta).getByRole('button', { name: 'Delete Okta' })).toBeInTheDocument();
      expect(enforcement()).toBeEnabled();
      expect(screen.queryByText(ENFORCEMENT_BLOCKED)).not.toBeInTheDocument();
    });

    it('blocks the requirement while every provider is disabled', async () => {
      await loaded({ ...ENTITLED, providers: [ENTRA] });

      expect(enforcement()).toBeDisabled();
      expect(screen.getByText(ENFORCEMENT_BLOCKED)).toBeInTheDocument();
    });

    it('shows an enforced server with the requirement on', async () => {
      await loaded(ENFORCED);

      expect(enforcement()).toHaveAttribute('aria-checked', 'true');
      expect(enforcement()).toBeEnabled();
      expect(screen.queryByText(ENFORCED_READ_ONLY)).not.toBeInTheDocument();
    });
  });

  describe('redirect URI', () => {
    it('copies the redirect URI', async () => {
      await loaded(ENTITLED);

      fireEvent.click(screen.getByRole('button', { name: 'Copy redirect URI' }));

      await waitFor(() => expect(writeText).toHaveBeenCalledWith(REDIRECT_URI));
      expect(await screen.findByRole('button', { name: 'Copied redirect URI' })).toBeTruthy();
    });

    it('explains a copy the browser refused', async () => {
      writeText.mockRejectedValue(new Error('denied'));
      await loaded(ENTITLED);

      fireEvent.click(screen.getByRole('button', { name: 'Copy redirect URI' }));

      expect(await screen.findByRole('alert')).toHaveTextContent('Could not copy');
    });
  });

  describe('SAML', () => {
    it('shows the service provider values to copy into the identity provider', async () => {
      await loaded(ENTITLED_EMPTY);

      const card = screen
        .getByText(SERVICE_PROVIDER_TITLE)
        .closest('.settings-card') as HTMLElement;
      expect(card).not.toBeNull();
      for (const label of ['Entity ID', 'ACS URL', 'Metadata URL']) {
        expect(within(card).getByText(label)).toBeInTheDocument();
      }
      expect(within(card).getByText(SERVICE_PROVIDER.acs_url)).toBeInTheDocument();
      expect(within(card).getAllByText(SERVICE_PROVIDER.entity_id)).toHaveLength(2);
    });

    it.each([
      ['entity ID', SERVICE_PROVIDER.entity_id],
      ['ACS URL', SERVICE_PROVIDER.acs_url],
      ['metadata URL', SERVICE_PROVIDER.metadata_url],
    ])('copies the %s', async (name, value) => {
      await loaded(ENTITLED);

      fireEvent.click(screen.getByRole('button', { name: `Copy ${name}` }));

      await waitFor(() => expect(writeText).toHaveBeenCalledWith(value));
      expect(await screen.findByRole('button', { name: `Copied ${name}` })).toBeTruthy();
      expect(screen.getByRole('button', { name: 'Copy redirect URI' })).toBeInTheDocument();
    });

    it('badges each provider with its protocol and shows the SAML entity ID', async () => {
      await loaded(ENTITLED_WITH_SAML);

      expect(providerNames()).toEqual(['Okta', 'Keycloak']);
      expect(within(provider('Okta')).getByText('OIDC')).toBeInTheDocument();
      const keycloak = provider('Keycloak');
      expect(within(keycloak).getByText('SAML')).toBeInTheDocument();
      expect(keycloak).toHaveTextContent(KEYCLOAK.issuer);
      expect(keycloak).toHaveTextContent('Domains: acme.com');
      expect(keycloak).toHaveTextContent('Never joins accounts by email');
      expect(keycloak).not.toHaveTextContent('Provider emails trusted');
    });

    it('opens the SAML edit form without client fields', async () => {
      await loaded(ENTITLED_WITH_SAML);

      fireEvent.click(screen.getByRole('button', { name: 'Edit Keycloak' }));

      const dialog = screen.getByRole('dialog', { name: 'Edit Keycloak' });
      expect(within(dialog).queryByLabelText('Client ID')).toBeNull();
      expect((within(dialog).getByLabelText('Metadata URL') as HTMLInputElement).value).toBe(
        KEYCLOAK.metadata_url
      );
    });
  });

  describe('requiring single sign-on', () => {
    it('asks for confirmation before requiring it', async () => {
      setEnforcement.mockResolvedValue({ required: true });
      await loaded(ENTITLED);

      fireEvent.click(enforcement());
      const dialog = screen.getByRole('dialog', { name: 'Require single sign-on?' });
      expect(setEnforcement).not.toHaveBeenCalled();
      expect(dialog).toHaveTextContent('Everyone except server admins');

      get.mockResolvedValue({ ...ENTITLED, required: true });
      fireEvent.click(within(dialog).getByRole('button', { name: ENFORCEMENT_LABEL }));

      await waitFor(() =>
        expect(screen.queryByRole('dialog', { name: 'Require single sign-on?' })).toBeNull()
      );
      expect(setEnforcement).toHaveBeenCalledWith(true);
      expect(enforcement()).toHaveAttribute('aria-checked', 'true');
    });

    it('leaves it off when the confirmation is cancelled', async () => {
      await loaded(ENTITLED);

      fireEvent.keyDown(enforcement(), { key: 'Enter' });
      const dialog = screen.getByRole('dialog', { name: 'Require single sign-on?' });
      fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));

      await waitFor(() =>
        expect(screen.queryByRole('dialog', { name: 'Require single sign-on?' })).toBeNull()
      );
      expect(setEnforcement).not.toHaveBeenCalled();
      expect(enforcement()).toHaveAttribute('aria-checked', 'false');
    });

    it('keeps the confirmation open with the refusal', async () => {
      setEnforcement.mockRejectedValue(
        new SsoRefusal({ error: 'No provider', code: SsoAdminCode.ProviderRequired }, 409)
      );
      await loaded(ENTITLED);

      fireEvent.click(enforcement());
      const dialog = screen.getByRole('dialog', { name: 'Require single sign-on?' });
      fireEvent.click(within(dialog).getByRole('button', { name: ENFORCEMENT_LABEL }));

      expect(await within(dialog).findByRole('alert')).toHaveTextContent(
        refusalMessage(SsoAdminCode.ProviderRequired)
      );
    });

    it('turns it off straight away', async () => {
      setEnforcement.mockResolvedValue({ required: false });
      await loaded(ENFORCED);
      get.mockResolvedValue({ ...ENFORCED, required: false });

      fireEvent.click(enforcement());

      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
      await waitFor(() => expect(setEnforcement).toHaveBeenCalledWith(false));
      await waitFor(() => expect(enforcement()).toHaveAttribute('aria-checked', 'false'));
    });

    it('shows a refusal to turn it off next to the switch', async () => {
      setEnforcement.mockRejectedValue(
        new SsoRefusal({ error: 'License', code: SsoAdminCode.LicenseRequired }, 403)
      );
      await loaded(ENFORCED);

      fireEvent.click(enforcement());

      expect(await screen.findByRole('alert')).toHaveTextContent(
        refusalMessage(SsoAdminCode.LicenseRequired)
      );
    });
  });

  describe('providers', () => {
    it('adds a provider and lists it', async () => {
      createProvider.mockResolvedValue(OKTA);
      await loaded(ENTITLED_EMPTY);

      fireEvent.click(screen.getByRole('button', { name: 'Add provider' }));
      const dialog = screen.getByRole('dialog', { name: 'Add identity provider' });
      for (const [label, value] of [
        ['Display name', 'Okta'],
        ['Issuer URL', OKTA.issuer],
        ['Client ID', OKTA.client_id],
        ['Client secret', 's3cret'],
        ['Allowed domains', 'acme.com'],
      ]) {
        fireEvent.change(within(dialog).getByLabelText(label), { target: { value } });
      }
      get.mockResolvedValue({ ...ENTITLED_EMPTY, providers: [OKTA] });
      fireEvent.click(within(dialog).getByRole('button', { name: 'Add provider' }));

      await waitFor(() =>
        expect(screen.queryByRole('dialog', { name: 'Add identity provider' })).toBeNull()
      );
      expect(providerNames()).toEqual(['Okta']);
      expect(enforcement()).toBeEnabled();
    });

    it('opens the edit form for the chosen provider', async () => {
      await loaded(ENTITLED);

      fireEvent.click(screen.getByRole('button', { name: 'Edit Microsoft Entra ID' }));

      const dialog = screen.getByRole('dialog', { name: 'Edit Microsoft Entra ID' });
      expect((within(dialog).getByLabelText('Client ID') as HTMLInputElement).value).toBe(
        ENTRA.client_id
      );
    });

    it('asks before deleting a provider, then drops it', async () => {
      deleteProvider.mockResolvedValue(undefined);
      await loaded(ENTITLED);

      fireEvent.click(screen.getByRole('button', { name: 'Delete Okta' }));
      const dialog = screen.getByRole('dialog', { name: 'Delete Okta?' });
      expect(deleteProvider).not.toHaveBeenCalled();
      get.mockResolvedValue({ ...ENTITLED, providers: [ENTRA] });
      fireEvent.click(within(dialog).getByRole('button', { name: 'Delete provider' }));

      await waitFor(() =>
        expect(screen.queryByRole('dialog', { name: 'Delete Okta?' })).toBeNull()
      );
      expect(deleteProvider).toHaveBeenCalledWith(OKTA.id);
      expect(providerNames()).toEqual(['Microsoft Entra ID']);
    });

    it('keeps the delete dialog open with a refusal', async () => {
      deleteProvider.mockRejectedValue(
        new SsoRefusal({ error: 'Required', code: SsoAdminCode.ProviderRequired }, 409)
      );
      await loaded(ENFORCED);

      fireEvent.click(screen.getByRole('button', { name: 'Delete Okta' }));
      const dialog = screen.getByRole('dialog', { name: 'Delete Okta?' });
      fireEvent.click(within(dialog).getByRole('button', { name: 'Delete provider' }));

      expect(await within(dialog).findByRole('alert')).toHaveTextContent(
        refusalMessage(SsoAdminCode.ProviderRequired)
      );
      expect(providerNames()).toEqual(['Okta']);
    });

    it('leaves the provider alone when deletion is cancelled', async () => {
      await loaded(ENTITLED);

      fireEvent.click(screen.getByRole('button', { name: 'Delete Okta' }));
      fireEvent.click(
        within(screen.getByRole('dialog', { name: 'Delete Okta?' })).getByRole('button', {
          name: 'Cancel',
        })
      );

      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
      expect(deleteProvider).not.toHaveBeenCalled();
    });
  });
});
