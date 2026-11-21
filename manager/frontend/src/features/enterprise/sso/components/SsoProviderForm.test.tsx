import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ApiError } from '../../../../api/ApiError';
import { ENTITLED, ENTRA, KEYCLOAK, OKTA, PASTED_SAML } from '../fixtures';
import { METADATA_REQUIRED, refusalField, refusalMessage } from '../messages';
import { SsoRefusal } from '../SsoRefusal';
import { Protocol, SSO_ADMIN_CODES, SsoAdminCode } from '../schemas';
import type { ProviderResponse, SsoAdminCode as SsoAdminCodeValue } from '../types';
import {
  KEEP_METADATA_PLACEHOLDER,
  METADATA_SOURCE_LABEL,
  METADATA_URL_LABEL,
  METADATA_XML_LABEL,
  REFRESH_METADATA_LABEL,
  URL_SOURCE_LABEL,
  XML_SOURCE_LABEL,
} from './SamlFields';
import {
  DOMAINS_HINT,
  EMPTY_DOMAINS_HINT,
  KEEP_SECRET_PLACEHOLDER,
  PROTOCOL_LABEL,
  SAML_DOMAINS_HINT,
  SAML_LINKING_NOTE,
  VERIFIED_EMAIL_DESCRIPTION,
  VERIFIED_EMAIL_LABEL,
} from './SsoProviderForm';
import { UPGRADE_NOTICE } from './SsoUpgradeNotice';

const get = mock();
const createProvider = mock();
const updateProvider = mock();

mock.module('../api', () => ({
  ssoApi: {
    get,
    createProvider,
    updateProvider,
    deleteProvider: mock(),
    setEnforcement: mock(),
  },
}));

let SsoProviderForm: typeof import('./SsoProviderForm').SsoProviderForm;

beforeAll(async () => {
  ({ SsoProviderForm } = await import('./SsoProviderForm'));
});

afterAll(() => {
  mock.restore();
});

const FIELD_LABELS: Readonly<Record<string, string>> = {
  display_name: 'Display name',
  issuer: 'Issuer URL',
  client_id: 'Client ID',
  client_secret: 'Client secret',
  allowed_domains: 'Allowed domains',
};

const SAML_LABELS: Readonly<Record<string, string>> = {
  display_name: 'Display name',
  metadata: METADATA_URL_LABEL,
  allowed_domains: 'Allowed domains',
};

function renderForm(provider?: ProviderResponse) {
  const onClose = mock();
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity }, mutations: { retry: false } },
  });
  queryClient.setQueryData(['sso'], ENTITLED);
  render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <SsoProviderForm provider={provider} onClose={onClose} />
      </MemoryRouter>
    </QueryClientProvider>
  );
  return { onClose };
}

function field(label: string): HTMLInputElement {
  return screen.getByLabelText(label) as HTMLInputElement;
}

function type(label: string, value: string) {
  fireEvent.change(field(label), { target: { value } });
}

function fillNew() {
  type('Display name', ' Okta ');
  type('Issuer URL', 'https://acme.okta.com ');
  type('Client ID', '0oa1b2c3d4');
  type('Client secret', 's3cret');
  type('Allowed domains', 'Acme.com, acme.co.uk, ACME.com');
}

function submit(name: string) {
  fireEvent.click(screen.getByRole('button', { name }));
}

function choose(name: string) {
  fireEvent.click(screen.getByRole('radio', { name }));
}

function fillSaml() {
  choose('SAML 2.0');
  type('Display name', ' Keycloak ');
  type(METADATA_URL_LABEL, ` ${KEYCLOAK.metadata_url} `);
  type('Allowed domains', 'Acme.com');
}

function switchNamed(name: string): HTMLElement {
  return screen.getByRole('switch', { name });
}

function errorOf(label: string): string | null {
  const input = field(label);
  const id = input.getAttribute('aria-describedby');
  const described = id ? document.getElementById(id) : null;
  return input.getAttribute('aria-invalid') === 'true' ? (described?.textContent ?? null) : null;
}

describe('SsoProviderForm', () => {
  beforeEach(() => {
    get.mockReset();
    get.mockResolvedValue(ENTITLED);
    createProvider.mockReset();
    updateProvider.mockReset();
  });

  describe('adding a provider', () => {
    it('starts empty with verified emails required and the provider enabled', () => {
      renderForm();

      expect(screen.getByRole('dialog', { name: 'Add identity provider' })).toBeInTheDocument();
      for (const label of Object.values(FIELD_LABELS)) {
        expect(field(label).value).toBe('');
      }
      expect(field('Client secret')).toHaveAttribute('type', 'password');
      expect(field('Client secret')).not.toHaveAttribute('placeholder');
      expect(switchNamed('Require verified email')).toHaveAttribute('aria-checked', 'true');
      expect(switchNamed('Enabled')).toHaveAttribute('aria-checked', 'true');
    });

    it('posts the contract body with trimmed names and normalized domains, then closes', async () => {
      createProvider.mockResolvedValue(OKTA);
      const { onClose } = renderForm();

      fillNew();
      submit('Add provider');

      await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
      expect(createProvider).toHaveBeenCalledWith({
        display_name: 'Okta',
        issuer: 'https://acme.okta.com',
        client_id: '0oa1b2c3d4',
        client_secret: 's3cret',
        allowed_domains: ['acme.com', 'acme.co.uk'],
        require_verified_email: true,
        enabled: true,
      });
    });

    it('sends the switches as set with the keyboard', async () => {
      createProvider.mockResolvedValue(ENTRA);
      renderForm();

      fillNew();
      fireEvent.keyDown(switchNamed('Require verified email'), { key: ' ' });
      fireEvent.keyDown(switchNamed('Enabled'), { key: 'Enter' });
      expect(switchNamed('Require verified email')).toHaveAttribute('aria-checked', 'false');
      expect(switchNamed('Enabled')).toHaveAttribute('aria-checked', 'false');
      submit('Add provider');

      await waitFor(() => expect(createProvider).toHaveBeenCalledTimes(1));
      expect(createProvider.mock.calls[0][0]).toMatchObject({
        require_verified_email: false,
        enabled: false,
      });
    });

    it('asks for every required field before sending anything', () => {
      renderForm();

      submit('Add provider');

      expect(errorOf('Display name')).toBe(refusalMessage(SsoAdminCode.DisplayNameInvalid));
      expect(errorOf('Issuer URL')).toBe(refusalMessage(SsoAdminCode.IssuerInvalid));
      expect(errorOf('Client ID')).toBe(refusalMessage(SsoAdminCode.ClientIdInvalid));
      expect(errorOf('Client secret')).toBe(refusalMessage(SsoAdminCode.ClientSecretInvalid));
      expect(errorOf('Allowed domains')).toBeNull();
      expect(createProvider).not.toHaveBeenCalled();
    });

    it('closes on Cancel without sending', () => {
      const { onClose } = renderForm();

      submit('Cancel');

      expect(onClose).toHaveBeenCalledTimes(1);
      expect(createProvider).not.toHaveBeenCalled();
    });

    it('explains the verified-email switch for Microsoft Entra ID', () => {
      renderForm();

      expect(screen.getByText(VERIFIED_EMAIL_DESCRIPTION)).toBeInTheDocument();
      expect(VERIFIED_EMAIL_DESCRIPTION).toContain('Microsoft Entra ID');
      expect(VERIFIED_EMAIL_DESCRIPTION).toContain('allowed domains');
      expect(VERIFIED_EMAIL_DESCRIPTION).toContain('never joins an existing Zone account');
    });
  });

  describe('allowed domains hint', () => {
    it('warns that anyone gets an account while no domains are set, and drops it once one is', () => {
      renderForm();

      expect(screen.getByText(EMPTY_DOMAINS_HINT)).toBeInTheDocument();

      type('Allowed domains', 'acme.com');

      expect(screen.queryByText(EMPTY_DOMAINS_HINT)).not.toBeInTheDocument();
      expect(screen.getByText(DOMAINS_HINT)).toBeInTheDocument();

      type('Allowed domains', ' , ');

      expect(screen.getByText(EMPTY_DOMAINS_HINT)).toBeInTheDocument();
    });
  });

  describe('editing a provider', () => {
    it('fills the stored fields and leaves the secret empty with a keep-it placeholder', () => {
      renderForm(OKTA);

      expect(screen.getByRole('dialog', { name: 'Edit Okta' })).toBeInTheDocument();
      expect(field('Display name').value).toBe('Okta');
      expect(field('Issuer URL').value).toBe(OKTA.issuer);
      expect(field('Client ID').value).toBe(OKTA.client_id);
      expect(field('Client secret').value).toBe('');
      expect(field('Client secret')).toHaveAttribute('placeholder', KEEP_SECRET_PLACEHOLDER);
      expect(field('Allowed domains').value).toBe('acme.com');
      expect(screen.queryByText(EMPTY_DOMAINS_HINT)).not.toBeInTheDocument();
    });

    it('keeps Save disabled until something changes', () => {
      renderForm(OKTA);

      expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();

      fireEvent.click(switchNamed('Enabled'));

      expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled();
    });

    it('sends only the changed fields and omits a blank secret', async () => {
      updateProvider.mockResolvedValue({ ...OKTA, display_name: 'Okta SSO', enabled: false });
      const { onClose } = renderForm(OKTA);

      type('Display name', 'Okta SSO');
      fireEvent.click(switchNamed('Enabled'));
      submit('Save');

      await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
      expect(updateProvider).toHaveBeenCalledWith(OKTA.id, {
        display_name: 'Okta SSO',
        enabled: false,
      });
    });

    it('replaces the secret only when a new one is typed', async () => {
      updateProvider.mockResolvedValue(OKTA);
      renderForm(OKTA);

      type('Client secret', 'n3w-s3cret');
      submit('Save');

      await waitFor(() => expect(updateProvider).toHaveBeenCalledTimes(1));
      expect(updateProvider).toHaveBeenCalledWith(OKTA.id, { client_secret: 'n3w-s3cret' });
    });

    it('does not ask for a secret it already holds', async () => {
      updateProvider.mockResolvedValue(OKTA);
      renderForm(OKTA);

      type('Allowed domains', '');
      submit('Save');

      await waitFor(() =>
        expect(updateProvider).toHaveBeenCalledWith(OKTA.id, { allowed_domains: [] })
      );
      expect(errorOf('Client secret')).toBeNull();
    });
  });

  describe('server refusals', () => {
    async function refuse(code: SsoAdminCodeValue, detail = 'Refused by the server') {
      createProvider.mockRejectedValue(new SsoRefusal({ error: detail, code }, 400));
      renderForm();
      fillNew();
      submit('Add provider');
      await waitFor(() => expect(createProvider).toHaveBeenCalledTimes(1));
    }

    for (const code of SSO_ADMIN_CODES) {
      const target = refusalField(code);
      if (target === null || FIELD_LABELS[target] === undefined) {
        continue;
      }
      it(`puts ${code} on ${FIELD_LABELS[target]}`, async () => {
        await refuse(code);

        await waitFor(() =>
          expect(errorOf(FIELD_LABELS[target])).toBe(refusalMessage(code, 'Refused by the server'))
        );
        for (const [name, label] of Object.entries(FIELD_LABELS)) {
          if (name !== target) {
            expect(errorOf(label)).toBeNull();
          }
        }
        expect(screen.queryByText(UPGRADE_NOTICE)).not.toBeInTheDocument();
      });
    }

    for (const code of [
      SsoAdminCode.ProviderNotFound,
      SsoAdminCode.ProviderRequired,
      SsoAdminCode.ProtocolMismatch,
      SsoAdminCode.VerifiedEmailUnsupported,
    ]) {
      it(`shows ${code} for the whole form`, async () => {
        await refuse(code);

        const dialog = screen.getByRole('dialog');
        expect(await within(dialog).findByText(refusalMessage(code))).toHaveAttribute(
          'role',
          'alert'
        );
        for (const label of Object.values(FIELD_LABELS)) {
          expect(errorOf(label)).toBeNull();
        }
      });
    }

    it('shows the upgrade notice when the license is required', async () => {
      await refuse(SsoAdminCode.LicenseRequired);

      expect(await screen.findByText(UPGRADE_NOTICE)).toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'Go to License' })).toHaveAttribute(
        'href',
        '/settings/server/license'
      );
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    });

    it('names the discovery problem the server found on the issuer field', async () => {
      await refuse(
        SsoAdminCode.DiscoveryFailed,
        'The discovery document names a different issuer.'
      );

      await waitFor(() =>
        expect(errorOf('Issuer URL')).toBe(
          "Zone couldn't read this issuer's OpenID Connect configuration. The discovery document names a different issuer."
        )
      );
    });

    it('keeps the typed values after a refusal', async () => {
      await refuse(SsoAdminCode.IssuerInvalid);

      await waitFor(() => expect(errorOf('Issuer URL')).not.toBeNull());
      expect(field('Issuer URL').value.trim()).toBe('https://acme.okta.com');
      expect(field('Client secret').value).toBe('s3cret');
    });

    it('clears the refusal once a field is edited', async () => {
      await refuse(SsoAdminCode.IssuerInvalid);
      await waitFor(() => expect(errorOf('Issuer URL')).not.toBeNull());

      type('Issuer URL', 'https://acme.okta.com/oauth2/default');

      await waitFor(() => expect(errorOf('Issuer URL')).toBeNull());
    });

    it('shows the message of a failure that is not a refusal', async () => {
      createProvider.mockRejectedValue(new ApiError('Failed to add the provider: 500', 500));
      renderForm();
      fillNew();
      submit('Add provider');

      expect(await screen.findByRole('alert')).toHaveTextContent('Failed to add the provider: 500');
    });

    it('maps a refusal on an edit the same way', async () => {
      updateProvider.mockRejectedValue(
        new SsoRefusal({ error: 'Duplicate', code: SsoAdminCode.ProviderDuplicate }, 409)
      );
      renderForm(OKTA);

      type('Client ID', '0oa-other');
      submit('Save');

      await waitFor(() =>
        expect(errorOf('Client ID')).toBe(refusalMessage(SsoAdminCode.ProviderDuplicate))
      );
    });
  });
  describe('choosing the protocol', () => {
    it('offers OpenID Connect and SAML 2.0 when adding, starting on OpenID Connect', () => {
      renderForm();

      const group = screen.getByRole('group', { name: PROTOCOL_LABEL });
      expect(within(group).getByRole('radio', { name: 'OpenID Connect' })).toBeChecked();
      expect(within(group).getByRole('radio', { name: 'SAML 2.0' })).not.toBeChecked();
    });

    it('swaps the client fields and the verified-email switch for SAML fields', () => {
      renderForm();

      choose('SAML 2.0');

      for (const label of ['Issuer URL', 'Client ID', 'Client secret']) {
        expect(screen.queryByLabelText(label)).toBeNull();
      }
      expect(screen.queryByRole('switch', { name: VERIFIED_EMAIL_LABEL })).toBeNull();
      expect(screen.getByText(SAML_LINKING_NOTE)).toBeInTheDocument();
      expect(screen.getByText(SAML_DOMAINS_HINT)).toBeInTheDocument();
      expect(screen.getByRole('group', { name: METADATA_SOURCE_LABEL })).toBeInTheDocument();
      expect(field(METADATA_URL_LABEL).value).toBe('');
      expect(switchNamed('Enabled')).toHaveAttribute('aria-checked', 'true');

      choose('OpenID Connect');

      expect(field('Issuer URL')).toBeInTheDocument();
      expect(switchNamed(VERIFIED_EMAIL_LABEL)).toBeInTheDocument();
      expect(screen.queryByText(SAML_LINKING_NOTE)).toBeNull();
    });

    it('never offers to change the protocol when editing', () => {
      renderForm(KEYCLOAK);

      expect(screen.queryByRole('group', { name: PROTOCOL_LABEL })).toBeNull();
      expect(screen.queryByRole('radio', { name: 'OpenID Connect' })).toBeNull();
      expect(screen.getByText('SAML 2.0')).toBeInTheDocument();
    });

    it('shows an OpenID Connect provider its protocol read-only when editing', () => {
      renderForm(OKTA);

      expect(screen.queryByRole('radio')).toBeNull();
      expect(screen.getByText('OpenID Connect')).toBeInTheDocument();
    });
  });

  describe('adding a SAML provider', () => {
    it('posts the metadata URL, the domains and the protocol, then closes', async () => {
      createProvider.mockResolvedValue(KEYCLOAK);
      const { onClose } = renderForm();

      fillSaml();
      submit('Add provider');

      await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
      expect(createProvider).toHaveBeenCalledWith({
        protocol: Protocol.Saml,
        display_name: 'Keycloak',
        metadata_url: KEYCLOAK.metadata_url,
        allowed_domains: ['acme.com'],
        enabled: true,
      });
    });

    it('posts pasted metadata XML instead of a URL', async () => {
      createProvider.mockResolvedValue(PASTED_SAML);
      renderForm();

      fillSaml();
      choose(XML_SOURCE_LABEL);
      expect(screen.queryByLabelText(METADATA_URL_LABEL)).toBeNull();
      type(METADATA_XML_LABEL, '<EntityDescriptor entityID="http://adfs.acme.com"/>');
      submit('Add provider');

      await waitFor(() => expect(createProvider).toHaveBeenCalledTimes(1));
      expect(createProvider).toHaveBeenCalledWith({
        protocol: Protocol.Saml,
        display_name: 'Keycloak',
        metadata: '<EntityDescriptor entityID="http://adfs.acme.com"/>',
        allowed_domains: ['acme.com'],
        enabled: true,
      });
    });

    it('never sends both sources, only the one chosen', async () => {
      createProvider.mockResolvedValue(KEYCLOAK);
      renderForm();

      fillSaml();
      choose(XML_SOURCE_LABEL);
      type(METADATA_XML_LABEL, '<EntityDescriptor/>');
      choose(URL_SOURCE_LABEL);
      submit('Add provider');

      await waitFor(() => expect(createProvider).toHaveBeenCalledTimes(1));
      expect(createProvider.mock.calls[0][0]).toHaveProperty('metadata_url');
      expect(createProvider.mock.calls[0][0]).not.toHaveProperty('metadata');
    });

    it('asks for the metadata and a domain before sending anything', () => {
      renderForm();

      choose('SAML 2.0');
      type('Display name', 'Keycloak');
      submit('Add provider');

      expect(errorOf(METADATA_URL_LABEL)).toBe(METADATA_REQUIRED);
      expect(errorOf('Allowed domains')).toBe(refusalMessage(SsoAdminCode.DomainRequired));
      choose(XML_SOURCE_LABEL);
      expect(errorOf(METADATA_XML_LABEL)).toBe(METADATA_REQUIRED);
      expect(createProvider).not.toHaveBeenCalled();
    });

    it('names the metadata problem the server found on the metadata field', async () => {
      createProvider.mockRejectedValue(
        new SsoRefusal(
          {
            error: 'The metadata has no signing certificate.',
            code: SsoAdminCode.MetadataInvalid,
          },
          400
        )
      );
      renderForm();
      fillSaml();
      submit('Add provider');

      await waitFor(() =>
        expect(errorOf(METADATA_URL_LABEL)).toBe(
          "Zone couldn't read this identity provider's SAML metadata. The metadata has no signing certificate."
        )
      );
      for (const label of ['Display name', 'Allowed domains']) {
        expect(errorOf(label)).toBeNull();
      }
    });

    it('puts a duplicate entity ID on the metadata field', async () => {
      createProvider.mockRejectedValue(
        new SsoRefusal({ error: 'Duplicate', code: SsoAdminCode.ProviderDuplicate }, 409)
      );
      renderForm();
      fillSaml();
      submit('Add provider');

      await waitFor(() =>
        expect(errorOf(SAML_LABELS.metadata)).toBe(refusalMessage(SsoAdminCode.ProviderDuplicate))
      );
    });

    it('puts a domain refusal on the domains field', async () => {
      createProvider.mockRejectedValue(
        new SsoRefusal({ error: 'Domains', code: SsoAdminCode.DomainRequired }, 400)
      );
      renderForm();
      fillSaml();
      submit('Add provider');

      await waitFor(() =>
        expect(errorOf('Allowed domains')).toBe(refusalMessage(SsoAdminCode.DomainRequired))
      );
    });
  });

  describe('editing a SAML provider', () => {
    it('shows the entity ID, SSO URL and fingerprints read-only and prefills the metadata URL', () => {
      renderForm(KEYCLOAK);

      expect(screen.getByRole('dialog', { name: 'Edit Keycloak' })).toBeInTheDocument();
      expect(screen.getByText(KEYCLOAK.issuer)).toBeInTheDocument();
      expect(screen.getByText(KEYCLOAK.sso_url ?? '')).toBeInTheDocument();
      for (const fingerprint of KEYCLOAK.certificate_fingerprints) {
        expect(screen.getByText(fingerprint)).toBeInTheDocument();
      }
      expect(screen.queryByRole('textbox', { name: KEYCLOAK.issuer })).toBeNull();
      expect(field(METADATA_URL_LABEL).value).toBe(KEYCLOAK.metadata_url);
      expect(field('Allowed domains').value).toBe('acme.com');
      expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
    });

    it('leaves the XML box empty with a keep-it placeholder', () => {
      renderForm(PASTED_SAML);

      expect(screen.getByRole('radio', { name: XML_SOURCE_LABEL })).toBeChecked();
      expect(field(METADATA_XML_LABEL).value).toBe('');
      expect(field(METADATA_XML_LABEL)).toHaveAttribute('placeholder', KEEP_METADATA_PLACEHOLDER);
      expect(screen.queryByRole('switch', { name: REFRESH_METADATA_LABEL })).toBeNull();
    });

    it('sends only a changed metadata URL', async () => {
      updateProvider.mockResolvedValue(KEYCLOAK);
      const { onClose } = renderForm(KEYCLOAK);

      type(METADATA_URL_LABEL, 'https://sso.acme.com/realms/acme/descriptor');
      submit('Save');

      await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
      expect(updateProvider).toHaveBeenCalledWith(KEYCLOAK.id, {
        metadata_url: 'https://sso.acme.com/realms/acme/descriptor',
      });
    });

    it('reads the same metadata URL again to pick up rotated certificates', async () => {
      updateProvider.mockResolvedValue(KEYCLOAK);
      renderForm(KEYCLOAK);

      fireEvent.click(switchNamed(REFRESH_METADATA_LABEL));
      submit('Save');

      await waitFor(() =>
        expect(updateProvider).toHaveBeenCalledWith(KEYCLOAK.id, {
          metadata_url: KEYCLOAK.metadata_url,
        })
      );
    });

    it('sends only pasted XML when the source switches to XML', async () => {
      updateProvider.mockResolvedValue(KEYCLOAK);
      renderForm(KEYCLOAK);

      choose(XML_SOURCE_LABEL);
      expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
      type(METADATA_XML_LABEL, '<EntityDescriptor/>');
      submit('Save');

      await waitFor(() =>
        expect(updateProvider).toHaveBeenCalledWith(KEYCLOAK.id, {
          metadata: '<EntityDescriptor/>',
        })
      );
    });

    it('sends only the changed fields, never a verified-email setting', async () => {
      updateProvider.mockResolvedValue(KEYCLOAK);
      renderForm(KEYCLOAK);

      type('Display name', 'Acme SSO');
      fireEvent.click(switchNamed('Enabled'));
      submit('Save');

      await waitFor(() =>
        expect(updateProvider).toHaveBeenCalledWith(KEYCLOAK.id, {
          display_name: 'Acme SSO',
          enabled: false,
        })
      );
    });

    it('refuses to save with no allowed domains', () => {
      renderForm(KEYCLOAK);

      type('Allowed domains', '');
      submit('Save');

      expect(errorOf('Allowed domains')).toBe(refusalMessage(SsoAdminCode.DomainRequired));
      expect(updateProvider).not.toHaveBeenCalled();
    });
  });
});
