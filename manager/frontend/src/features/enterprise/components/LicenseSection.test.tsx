import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { ApiError } from '../../../api/ApiError';
import {
  ACTIVE,
  COMMUNITY,
  COMMUNITY_AT_CAP,
  COMMUNITY_OVER_CAP,
  ENFORCEMENT_CLAIM,
  GRACE,
  LAPSED,
  MANUAL_RENEWAL,
  OVER_SEATS,
  RENEWAL_DISABLED,
  RENEWAL_ENDED,
  RENEWAL_UNREACHABLE,
  RENEWED,
  TEAM,
  TEAM_LAPSED,
  UNCOUNTED,
  UNCOUNTED_COMMUNITY,
  UNVERIFIED,
} from '../fixtures';
import { LicenseRefusal } from '../LicenseRefusal';
import { PRICING_URL } from '../pricing';
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

function keyBoxBehindDialog(): HTMLTextAreaElement {
  return screen.getByRole('textbox', { name: 'License key', hidden: true }) as HTMLTextAreaElement;
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

const RENEWAL = 'Renewal';

const RENEWAL_CHECK = 'Last renewal check';

function renewalRows(): HTMLElement[] {
  return screen
    .getAllByRole('term')
    .filter((term) => [RENEWAL, RENEWAL_CHECK].includes(term.textContent ?? ''))
    .map((term) => term.parentElement as HTMLElement);
}

function renewalCopy(): string[] {
  return renewalRows().flatMap((row) => [
    row.textContent ?? '',
    ...[row, ...row.querySelectorAll('*')].flatMap((element) =>
      ['title', 'aria-label'].map((name) => element.getAttribute(name) ?? '')
    ),
  ]);
}

function renewalOutcome(): HTMLElement {
  const row = renewalRows().find((element) => element.textContent?.startsWith(RENEWAL_CHECK));
  return row?.querySelector('.ui-badge') as HTMLElement;
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

    it('shows Community with its members against the cap and an enabled paste box', async () => {
      await loaded(COMMUNITY);

      expect(screen.getByText('Community')).toBeInTheDocument();
      expect(screen.getByText(/No license is installed/)).toBeInTheDocument();
      expect(details()).toEqual({ Members: '2 of 3 members this month' });
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
      expect(details()).toEqual({ Members: '2 of 3 members this month' });
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

    it('shows an active license with its customer, seats and expiry', async () => {
      await loaded(ACTIVE);

      expect(screen.getByText('Enterprise', { selector: '.license-edition-name' })).toBeTruthy();
      expect(screen.getByText('Active')).toBeInTheDocument();
      expect(details()).toEqual({
        Customer: 'Acme Ltd',
        Seats: '12 of 25 seats used this month',
        Expires: 'Nov 1, 2027',
        'Grace ends': 'Dec 1, 2027',
        Renewal: 'Renews automatically',
        'Last renewal check': 'No check recorded yet',
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
      expect(warning).not.toHaveTextContent(ENFORCEMENT_CLAIM);
      expect(details().Expired).toBe('Nov 1, 2027');
    });

    it('warns that a license lapsed and asks for a renewed key while still showing it and its edition', async () => {
      await loaded(LAPSED);

      expect(screen.getByText('Lapsed')).toBeInTheDocument();
      const warning = screen.getByText(/lapsed on/);
      expect(warning).toHaveClass('alert-error');
      expect(warning).toHaveTextContent('lapsed on Dec 1, 2027');
      expect(warning).toHaveTextContent('Paste a renewed license key');
      expect(warning).not.toHaveTextContent(ENFORCEMENT_CLAIM);
      expect(screen.getByText('Enterprise', { selector: '.license-edition-name' })).toBeTruthy();
      expect(details().Customer).toBe('Acme Ltd');
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

    it('describes the plan without naming one edition', async () => {
      await loaded(COMMUNITY);

      const description = screen.getByText(/member usage/);
      expect(description).toHaveTextContent("See this server's plan and member usage");
      expect(description).not.toHaveTextContent('Enterprise');
    });

    it('says new members are refused at the cap while everyone already here keeps access', async () => {
      await loaded(COMMUNITY_AT_CAP);

      expect(details().Members).toBe('3 of 3 members this month');
      const notice = screen.getByRole('status');
      expect(notice).toHaveClass('alert-warning');
      expect(notice).toHaveTextContent('reached the 3 active members Zone Community allows');
      expect(notice).toHaveTextContent(
        'new members are refused until a Team or Enterprise license is added'
      );
      expect(notice).toHaveTextContent('Everyone already here keeps access.');
      expect(screen.getByRole('link', { name: /Buy a license/ })).toHaveAttribute(
        'href',
        PRICING_URL
      );
    });

    it('alerts once over the cap, counting the extra people and promising nobody loses access', async () => {
      await loaded(COMMUNITY_OVER_CAP);

      expect(details().Members).toBe('5 of 3 members this month');
      const alert = screen.getByRole('status');
      expect(alert).toHaveClass('alert-warning');
      expect(alert).toHaveTextContent(
        '5 people were active this month, 2 more than the 3 members Zone Community allows'
      );
      expect(alert).toHaveTextContent('Everyone keeps access');
      expect(alert).not.toHaveTextContent(/block|lock|suspend|disable|lose/i);
      expect(screen.queryByText(/reached the 3 active members/)).not.toBeInTheDocument();
    });

    it('shows an active Team license with seats and no member cap', async () => {
      await loaded(TEAM);

      expect(screen.getByText('Team', { selector: '.license-edition-name' })).toBeTruthy();
      expect(screen.getByText('Active')).toBeInTheDocument();
      expect(details().Seats).toBe('12 of 25 seats used this month');
      expect(details()).not.toHaveProperty('Members');
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
      expect(screen.queryByRole('link', { name: /Buy a license/ })).not.toBeInTheDocument();
    });

    it('keeps the Team edition on a lapsed Team license and counts it against the cap', async () => {
      await loaded(TEAM_LAPSED);

      expect(screen.getByText('Team', { selector: '.license-edition-name' })).toBeTruthy();
      expect(details().Members).toBe('12 of 3 members this month');
      expect(details().Seats).toBe('12 of 25 seats used this month');
      expect(
        screen.getByText(/12 people were active this month, 9 more than the 3 members/)
      ).toHaveClass('alert-warning');
    });

    it('says seat usage is unavailable instead of showing a count it does not have', async () => {
      await loaded(UNCOUNTED);

      expect(details().Seats).toBe('Seat usage unavailable (25 seats licensed)');
      expect(screen.queryByText(/used this month/)).not.toBeInTheDocument();
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
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

  describe('renewal', () => {
    it('shows the date and outcome of the last automatic renewal', async () => {
      await loaded(RENEWED);

      expect(details()[RENEWAL]).toBe('Renews automatically');
      expect(details()[RENEWAL_CHECK]).toBe('Oct 20, 2027 Renewed');
      expect(renewalOutcome()).toHaveClass('ui-badge-success');
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
    });

    it('marks a renewal that could not reach the license server as a warning', async () => {
      await loaded(RENEWAL_UNREACHABLE);

      expect(details()[RENEWAL_CHECK]).toBe('Oct 25, 2027 License server unreachable');
      expect(renewalOutcome()).toHaveClass('ui-badge-warning');
    });

    it('says a subscription the license server will not renew is not renewable, without claiming it ended', async () => {
      await loaded(RENEWAL_ENDED);

      expect(details()[RENEWAL_CHECK]).toBe('Oct 27, 2027 Subscription not renewable');
      expect(renewalOutcome()).toHaveClass('ui-badge-warning');
      expect(screen.queryByText(/ended/i)).not.toBeInTheDocument();
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
    });

    it('says automatic renewal is off on this server without a check row', async () => {
      await loaded(RENEWAL_DISABLED);

      expect(details()[RENEWAL]).toBe('Automatic renewal is off on this server');
      expect(details()).not.toHaveProperty(RENEWAL_CHECK);
      expect(screen.queryByText(/Disabled/)).not.toBeInTheDocument();
    });

    it('asks for a pasted key when the license does not renew by itself', async () => {
      await loaded(MANUAL_RENEWAL);

      expect(details()[RENEWAL]).toBe('Renew by pasting a new key before it expires');
      expect(details()).not.toHaveProperty(RENEWAL_CHECK);
    });

    it('shows no renewal for Community or a stored key that does not verify', async () => {
      for (const response of [COMMUNITY, UNVERIFIED]) {
        const { unmount } = renderSection(response);
        await screen.findByRole('heading', { name: 'License' });

        expect(details()).not.toHaveProperty(RENEWAL);
        expect(details()).not.toHaveProperty(RENEWAL_CHECK);
        unmount();
      }
    });

    for (const [name, response] of [
      ['an unchecked automatic renewal', ACTIVE],
      ['a renewed license', RENEWED],
      ['an unreachable license server', RENEWAL_UNREACHABLE],
      ['automatic renewal turned off', RENEWAL_DISABLED],
      ['manual renewal', MANUAL_RENEWAL],
    ] as const) {
      it(`claims no enforcement for ${name}`, async () => {
        await loaded(response);

        const copy = renewalCopy();
        expect(copy.length).toBeGreaterThan(0);
        for (const text of copy) {
          expect(text).not.toMatch(ENFORCEMENT_CLAIM);
        }
      });
    }
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

    it('shows usage as unavailable when the key is stored but its seats could not be counted', async () => {
      set.mockResolvedValue(UNCOUNTED);
      await loaded(OVER_SEATS);
      expect(screen.getByRole('status')).toHaveTextContent('30 people were active this month');

      paste('payload.signature');
      fireEvent.click(screen.getByRole('button', { name: 'Replace license' }));

      await screen.findByText('License replaced.');
      expect(details().Seats).toBe('Seat usage unavailable (25 seats licensed)');
      expect(screen.queryByText(/people were active this month/)).not.toBeInTheDocument();
      expect(screen.queryByText(/0 of 25/)).not.toBeInTheDocument();
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

  describe('one change at a time', () => {
    function deferred(): { promise: Promise<unknown>; settle: (value: unknown) => void } {
      let settle: (value: unknown) => void = () => {};
      const promise = new Promise((resolve) => (settle = resolve));
      return { promise, settle };
    }

    it('disables removal while a key is saved and offers it again once the save settles', async () => {
      const save = deferred();
      set.mockImplementation(() => save.promise);
      await loaded(ACTIVE);

      paste('payload.signature');
      fireEvent.click(screen.getByRole('button', { name: 'Replace license' }));

      await waitFor(() => expect(removeButton()).toBeDisabled());
      fireEvent.click(removeButton());
      expect(screen.queryByRole('dialog', { name: REMOVE_DIALOG })).not.toBeInTheDocument();

      save.settle(GRACE);

      await screen.findByText('License replaced.');
      expect(removeButton()).toBeEnabled();
      expect(remove).not.toHaveBeenCalled();
    });

    it('disables saving a key while the license is removed', async () => {
      remove.mockImplementation(() => new Promise(() => {}));
      await loaded(ACTIVE);
      paste('payload.signature');

      confirmRemoval();

      await waitFor(() => expect(keyBoxBehindDialog()).toBeDisabled());
      const replace = screen.getByRole('button', { name: 'Replace license', hidden: true });
      expect(replace).toBeDisabled();
      fireEvent.submit(replace.closest('form') as HTMLFormElement);
      expect(set).not.toHaveBeenCalled();
    });

    it('will not confirm a removal while a key is saved', async () => {
      set.mockImplementation(() => new Promise(() => {}));
      await loaded(ACTIVE);
      paste('payload.signature');
      fireEvent.click(removeButton());
      const dialog = screen.getByRole('dialog', { name: REMOVE_DIALOG });

      fireEvent.submit(keyBoxBehindDialog().closest('form') as HTMLFormElement);

      const confirm = within(dialog).getByRole('button', { name: 'Remove license' });
      await waitFor(() => expect(confirm).toBeDisabled());
      fireEvent.click(confirm);
      expect(remove).not.toHaveBeenCalled();
      expect(set).toHaveBeenCalledTimes(1);
    });
  });

  describe('removing the license', () => {
    it('asks for confirmation before removing, then shows Community', async () => {
      remove.mockResolvedValue(COMMUNITY);
      await loaded(ACTIVE);

      fireEvent.click(removeButton());
      const dialog = screen.getByRole('dialog', { name: REMOVE_DIALOG });
      expect(dialog).toHaveTextContent('This server goes back to the Community edition.');
      expect(dialog).not.toHaveTextContent(ENFORCEMENT_CLAIM);
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
      expect(dialog).not.toHaveTextContent('goes back to the Community edition');

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

    it('shows usage as unavailable when the license is removed but seats could not be counted', async () => {
      remove.mockResolvedValue(UNCOUNTED_COMMUNITY);
      await loaded(OVER_SEATS);

      confirmRemoval();

      await screen.findByText(/No license is installed/);
      expect(details()).toEqual({ Members: 'Member usage unavailable (3 members allowed)' });
      expect(screen.queryByRole('status')).not.toBeInTheDocument();
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

describe('LicenseSection buy link', () => {
  const BUY = /Buy a license/;
  const BUY_NAME = 'Buy a license (opens in a new tab)';

  beforeEach(() => {
    get.mockReset();
  });

  const unlicensed: readonly (readonly [string, LicenseResponse])[] = [
    ['a Community install', COMMUNITY],
    ['a stored key that does not verify', UNVERIFIED],
    ['a lapsed license', LAPSED],
  ];

  for (const [name, response] of unlicensed) {
    it(`links ${name} to pricing in a new tab`, async () => {
      await loaded(response);

      const link = screen.getByRole('link', { name: BUY });
      expect(screen.getByRole('link', { name: BUY_NAME })).toBe(link);
      expect(link).toHaveAttribute('href', PRICING_URL);
      expect(link).toHaveAttribute('target', '_blank');
      expect(link.getAttribute('rel')?.split(' ')).toContain('noopener');
    });
  }

  const licensed: readonly (readonly [string, LicenseResponse])[] = [
    ['an active license', ACTIVE],
    ['a license in its grace period', GRACE],
  ];

  for (const [name, response] of licensed) {
    it(`does not offer a purchase for ${name}`, async () => {
      await loaded(response);

      expect(screen.queryByRole('link', { name: BUY })).not.toBeInTheDocument();
    });
  }

  it('does not offer a purchase while the license fails to load', async () => {
    get.mockRejectedValue(
      new ApiError('Failed to load the license: This needs a server admin.', 403)
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <LicenseSection />
      </QueryClientProvider>
    );

    await screen.findByRole('alert');
    expect(screen.queryByRole('link', { name: BUY })).not.toBeInTheDocument();
  });
});
