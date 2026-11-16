import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ApiError } from '../../../../api/ApiError';
import { ENTITLED, ENTRA, OKTA } from '../fixtures';
import { refusalField, refusalMessage } from '../messages';
import { SsoRefusal } from '../SsoRefusal';
import { SSO_ADMIN_CODES, SsoAdminCode } from '../schemas';
import type { ProviderResponse, SsoAdminCode as SsoAdminCodeValue } from '../types';
import {
  DOMAINS_HINT,
  EMPTY_DOMAINS_HINT,
  KEEP_SECRET_PLACEHOLDER,
  VERIFIED_EMAIL_DESCRIPTION,
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

const FIELD_LABELS = {
  display_name: 'Display name',
  issuer: 'Issuer URL',
  client_id: 'Client ID',
  client_secret: 'Client secret',
  allowed_domains: 'Allowed domains',
} as const;

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
      if (target === null) {
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

    for (const code of [SsoAdminCode.ProviderNotFound, SsoAdminCode.ProviderRequired]) {
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
});
