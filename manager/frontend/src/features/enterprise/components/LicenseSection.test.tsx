import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { ApiError } from '../../../api/ApiError';
import { ACTIVE, COMMUNITY, GRACE, LAPSED, OVER_SEATS, UNVERIFIED } from '../fixtures';
import { LicenseRefusal } from '../LicenseRefusal';
import { LICENSE_REFUSAL_CODES, UNVERIFIED_CODES } from '../schemas';
import { refusalMessage, unverifiedReason } from '../status';
import type { LicenseResponse } from '../types';

const get = mock();
const set = mock();
const remove = mock();

mock.module('../api', () => ({ licenseApi: { get, set, remove } }));

let LicenseSection: typeof import('./LicenseSection').LicenseSection;

beforeAll(async () => {
  ({ LicenseSection } = await import('./LicenseSection'));
});

afterAll(() => {
  mock.restore();
});

function renderSection(response: LicenseResponse) {
  get.mockResolvedValue(response);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <LicenseSection />
    </QueryClientProvider>
  );
}

async function loaded(response: LicenseResponse) {
  renderSection(response);
  await screen.findByRole('heading', { name: 'License' });
}

function keyBox(): HTMLTextAreaElement {
  return screen.getByRole('textbox', { name: 'License key' }) as HTMLTextAreaElement;
}

function paste(text: string) {
  fireEvent.change(keyBox(), { target: { value: text } });
}

const REMOVE_DIALOG = 'Remove the license?';

function removeButton(): HTMLElement {
  return screen.getByRole('button', { name: 'Remove license' });
}

function confirmRemoval(): HTMLElement {
  fireEvent.click(removeButton());
  const dialog = screen.getByRole('dialog', { name: REMOVE_DIALOG });
  fireEvent.click(within(dialog).getByRole('button', { name: 'Remove license' }));
  return dialog;
}

function details(): Record<string, string> {
  const entries: Record<string, string> = {};
  for (const term of screen.getAllByRole('term', { hidden: true })) {
    entries[term.textContent ?? ''] = term.nextElementSibling?.textContent ?? '';
  }
  return entries;
}

describe('LicenseSection', () => {
  beforeEach(() => {
    get.mockReset();
    set.mockReset();
    remove.mockReset();
  });

  describe('states', () => {
    it('shows a loading state until the license arrives', () => {
      get.mockImplementation(() => new Promise(() => {}));
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(
        <QueryClientProvider client={queryClient}>
          <LicenseSection />
        </QueryClientProvider>
      );

      expect(screen.getByText('Loading license...')).toBeInTheDocument();
      expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    });

    it('shows Community with the seats used and an enabled paste box', async () => {
      await loaded(COMMUNITY);

      expect(screen.getByText('Community')).toBeInTheDocument();
      expect(screen.getByText(/No license is installed/)).toBeInTheDocument();
      expect(details()).toEqual({ Seats: '12 seats used this month' });
      expect(keyBox()).toBeEnabled();
      expect(screen.getByRole('button', { name: 'Add license' })).toBeDisabled();
      expect(screen.queryByRole('button', { name: 'Remove license' })).not.toBeInTheDocument();
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
    });

    it('warns that a stored key no longer verifies instead of saying none is installed', async () => {
      await loaded(UNVERIFIED);

      expect(screen.queryByText(/No license is installed/)).not.toBeInTheDocument();
      const warning = screen.getByRole('status');
      expect(warning).toHaveClass('alert-warning');
      expect(warning).toHaveTextContent(
        "A license key is stored, but this version of Zone can't verify it"
      );
      expect(warning).toHaveTextContent('signed with a key this release no longer trusts');
      expect(warning).toHaveTextContent('Paste a renewed key.');
      expect(screen.getByText('Community', { selector: '.license-edition-name' })).toBeTruthy();
      expect(details()).toEqual({ Seats: '12 seats used this month' });
      expect(removeButton()).toBeEnabled();
      expect(screen.getByRole('button', { name: 'Replace license' })).toBeDisabled();
    });

    for (const code of UNVERIFIED_CODES) {
      it(`gives the reason a stored key failed with ${code}`, async () => {
        await loaded({ ...UNVERIFIED, unverified: { code } });

        expect(screen.getByRole('status')).toHaveTextContent(unverifiedReason(code));
        expect(removeButton()).toBeInTheDocument();
      });
    }

    it('shows an active license with its customer, tier, seats and expiry', async () => {
      await loaded(ACTIVE);

      expect(screen.getByText('Enterprise', { selector: '.license-edition-name' })).toBeTruthy();
      expect(screen.getByText('Active')).toBeInTheDocument();
      expect(details()).toEqual({
        Customer: 'Acme Ltd',
        Tier: 'Enterprise',
        Seats: '12 of 25 seats used this month',
        Expires: 'Nov 1, 2027',
        'Grace ends': 'Dec 1, 2027',
      });
      expect(screen.getByRole('button', { name: 'Replace license' })).toBeInTheDocument();
      expect(removeButton()).toBeInTheDocument();
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
    });

    it('warns during the grace period with the date it ends', async () => {
      await loaded(GRACE);

      expect(screen.getByText('Grace period')).toBeInTheDocument();
      const warning = screen.getByRole('status');
      expect(warning).toHaveClass('alert-warning');
      expect(warning).toHaveTextContent('grace period ends Dec 1, 2027');
      expect(details().Expired).toBe('Nov 1, 2027');
    });

    it('warns that a lapsed license stopped configuration while still showing it and its edition', async () => {
      await loaded(LAPSED);

      expect(screen.getByText('Lapsed')).toBeInTheDocument();
      const warning = screen.getByRole('status');
      expect(warning).toHaveClass('alert-error');
      expect(warning).toHaveTextContent('Configuring Enterprise features has stopped');
      expect(warning).toHaveTextContent('Sign-in and existing policies keep working');
      expect(screen.getByText('Team', { selector: '.license-edition-name' })).toBeTruthy();
      expect(details().Customer).toBe('Acme Ltd');
      expect(details().Tier).toBe('Team');
    });

    it('notes extra seats softly without blocking anyone', async () => {
      await loaded(OVER_SEATS);

      const notice = screen.getByRole('status');
      expect(notice).toHaveTextContent(
        '30 people were active this month, 5 more than the 25 seats'
      );
      expect(notice).toHaveTextContent('Everyone keeps access');
      expect(details().Seats).toBe('30 of 25 seats used this month');
    });

    it('offers a retry when the license fails to load', async () => {
      get.mockRejectedValueOnce(
        new ApiError('Failed to load the license: This needs a server admin.', 403)
      );
      get.mockResolvedValueOnce(COMMUNITY);
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(
        <QueryClientProvider client={queryClient}>
          <LicenseSection />
        </QueryClientProvider>
      );

      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('Failed to load the license: This needs a server admin.');
      fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));

      await screen.findByRole('textbox', { name: 'License key' });
      expect(get).toHaveBeenCalledTimes(2);
    });
  });

  describe('adding a key', () => {
    it('keeps the button disabled while the pasted text is only whitespace', async () => {
      await loaded(COMMUNITY);

      paste('  \n ');

      expect(screen.getByRole('button', { name: 'Add license' })).toBeDisabled();
    });

    it('sends the trimmed key and shows the license that comes back', async () => {
      set.mockResolvedValue(ACTIVE);
      await loaded(COMMUNITY);

      paste('\n  eyJwYXlsb2FkIjp0cnVlfQ\n.c2lnbmF0dXJl  \n');
      fireEvent.click(screen.getByRole('button', { name: 'Add license' }));

      await screen.findByText('License added.');
      expect(set).toHaveBeenCalledWith('eyJwYXlsb2FkIjp0cnVlfQ\n.c2lnbmF0dXJl');
      expect(details().Customer).toBe('Acme Ltd');
      expect(keyBox().value).toBe('');
      expect(screen.getByRole('button', { name: 'Replace license' })).toBeDisabled();
      expect(get).toHaveBeenCalledTimes(1);
    });

    it('disables the form while the key is checked', async () => {
      set.mockImplementation(() => new Promise(() => {}));
      await loaded(COMMUNITY);

      paste('payload.signature');
      fireEvent.click(screen.getByRole('button', { name: 'Add license' }));

      await waitFor(() => expect(keyBox()).toBeDisabled());
      expect(screen.getByRole('button', { name: 'Add license' })).toBeDisabled();
      expect(set).toHaveBeenCalledTimes(1);
    });

    it('says the license was replaced when one was already installed', async () => {
      set.mockResolvedValue(GRACE);
      await loaded(ACTIVE);

      paste('payload.signature');
      fireEvent.click(screen.getByRole('button', { name: 'Replace license' }));

      await screen.findByText('License replaced.');
      expect(screen.getByText('Grace period')).toBeInTheDocument();
    });

    it('replaces a stored key that no longer verifies and drops the warning', async () => {
      set.mockResolvedValue(ACTIVE);
      await loaded(UNVERIFIED);

      paste('payload.signature');
      fireEvent.click(screen.getByRole('button', { name: 'Replace license' }));

      await screen.findByText('License replaced.');
      expect(screen.queryByText(/can't verify it/)).not.toBeInTheDocument();
      expect(details().Customer).toBe('Acme Ltd');
    });

    for (const code of LICENSE_REFUSAL_CODES) {
      it(`explains a ${code} refusal next to the key`, async () => {
        set.mockRejectedValue(new LicenseRefusal({ error: 'Refused by the server', code }));
        await loaded(COMMUNITY);

        paste('payload.signature');
        fireEvent.click(screen.getByRole('button', { name: 'Add license' }));

        const alert = await screen.findByRole('alert');
        expect(alert).toHaveTextContent(refusalMessage(code));
        expect(keyBox()).toHaveAttribute('aria-invalid', 'true');
        expect(keyBox().getAttribute('aria-describedby')).toContain(alert.id);
        expect(keyBox().value).toBe('payload.signature');
        expect(screen.getByText('Community')).toBeInTheDocument();
      });
    }

    it('shows the server message for a failure that is not a refusal', async () => {
      set.mockRejectedValue(
        new ApiError('Failed to save the license: This needs a server admin.', 403)
      );
      await loaded(COMMUNITY);

      paste('payload.signature');
      fireEvent.click(screen.getByRole('button', { name: 'Add license' }));

      expect(await screen.findByRole('alert')).toHaveTextContent(
        'Failed to save the license: This needs a server admin.'
      );
    });

    it('clears the refusal once the key is edited', async () => {
      set.mockRejectedValue(
        new LicenseRefusal({ error: 'Refused', code: LICENSE_REFUSAL_CODES[0] })
      );
      await loaded(COMMUNITY);
      paste('payload.signature');
      fireEvent.click(screen.getByRole('button', { name: 'Add license' }));
      await screen.findByRole('alert');

      paste('payload.signature2');

      await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument());
      expect(keyBox()).not.toHaveAttribute('aria-invalid');
    });
  });

  describe('removing the license', () => {
    it('asks for confirmation before removing, then shows Community', async () => {
      remove.mockResolvedValue(COMMUNITY);
      await loaded(ACTIVE);

      fireEvent.click(removeButton());
      const dialog = screen.getByRole('dialog', { name: REMOVE_DIALOG });
      expect(dialog).toHaveTextContent('configuring Enterprise features stops');
      expect(remove).not.toHaveBeenCalled();

      fireEvent.click(within(dialog).getByRole('button', { name: 'Remove license' }));

      await waitFor(() =>
        expect(screen.queryByRole('dialog', { name: REMOVE_DIALOG })).not.toBeInTheDocument()
      );
      expect(remove).toHaveBeenCalledTimes(1);
      expect(screen.getByText(/No license is installed/)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Add license' })).toBeInTheDocument();
    });

    it('removes a stored key that no longer verifies, explaining the server stays on Community', async () => {
      remove.mockResolvedValue(COMMUNITY);
      await loaded(UNVERIFIED);

      fireEvent.click(removeButton());
      const dialog = screen.getByRole('dialog', { name: REMOVE_DIALOG });
      expect(dialog).toHaveTextContent("deletes the stored key this version of Zone can't verify");
      expect(dialog).toHaveTextContent('stays on the Community edition');
      expect(dialog).not.toHaveTextContent('configuring Enterprise features stops');

      fireEvent.click(within(dialog).getByRole('button', { name: 'Remove license' }));

      await waitFor(() =>
        expect(screen.queryByRole('dialog', { name: REMOVE_DIALOG })).not.toBeInTheDocument()
      );
      expect(remove).toHaveBeenCalledTimes(1);
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
      expect(screen.getByText(/No license is installed/)).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Remove license' })).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Add license' })).toBeInTheDocument();
    });

    it('keeps the unverified warning when removing the stored key fails', async () => {
      remove.mockRejectedValue(new ApiError('Failed to remove the license: 500', 500));
      await loaded(UNVERIFIED);

      const dialog = confirmRemoval();

      expect(await within(dialog).findByRole('alert')).toHaveTextContent(
        'Failed to remove the license: 500'
      );
      expect(
        screen.getByText(/A license key is stored, but this version of Zone can't verify it/)
      ).toBeInTheDocument();
    });

    it('leaves the license alone when the removal is cancelled', async () => {
      await loaded(ACTIVE);

      fireEvent.click(removeButton());
      fireEvent.click(
        within(screen.getByRole('dialog', { name: REMOVE_DIALOG })).getByRole('button', {
          name: 'Cancel',
        })
      );

      await waitFor(() =>
        expect(screen.queryByRole('dialog', { name: REMOVE_DIALOG })).not.toBeInTheDocument()
      );
      expect(remove).not.toHaveBeenCalled();
      expect(details().Customer).toBe('Acme Ltd');
    });

    it('keeps the dialog open with the error when removal fails', async () => {
      remove.mockRejectedValue(new ApiError('Failed to remove the license: 500', 500));
      await loaded(ACTIVE);

      const dialog = confirmRemoval();

      expect(await within(dialog).findByRole('alert')).toHaveTextContent(
        'Failed to remove the license: 500'
      );
      expect(details().Customer).toBe('Acme Ltd');
    });
  });
});
