import { createHash, randomBytes } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { Browser, Page } from '@playwright/test';
import {
  api,
  enabled,
  evidenceDir,
  expect,
  logLines,
  logMark,
  record,
  shot,
  signIn,
  sql,
  stamp,
  state,
  test,
  tokenFor,
  type Tenant,
} from './rig';

/**
 * Rows 1 to 7: auth and account, driven through the real registration, login,
 * verification, reset, sessions and invitation pages against the rig.
 *
 * The rig has no SMTP relay (the server's transport is implicit TLS with a
 * verified certificate, which no local catcher satisfies), so the verification
 * and reset rows take the documented fallback: the server's own token row is
 * read from the database, and because the table holds only the SHA-256 of the
 * token, the row's hash is replaced with the hash of a token this lane knows.
 * Every other step is the console's own.
 *
 * The account row 1 registers is written to a file, because Playwright
 * restarts the worker after a failure and module state does not survive that.
 */

interface Account {
  email: string;
  password: string;
  displayName: string;
  userId: string;
}

const accountFile = join(evidenceDir, 'auth-account.json');

function savedAccount(): Account {
  if (!existsSync(accountFile))
    throw new Error('row 1 has not registered an account yet');
  return JSON.parse(readFileSync(accountFile, 'utf8')) as Account;
}

const port = process.env.ZONE_LIVE_PORT ?? '4179';

async function signInAs(
  page: Page,
  email: string,
  password: string,
): Promise<void> {
  await page.goto('/login');
  await page.evaluate(() => {
    localStorage.removeItem('manager_access_token');
    localStorage.removeItem('manager_refresh_token');
    localStorage.removeItem('manager_user');
  });
  await page.goto('/login');
  await page.getByLabel('Email').fill(email);
  await page.getByLabel('Password').fill(password);
  await page.getByRole('button', { name: /sign in|log in/i }).click();
}

function knownToken(): { token: string; hash: string } {
  const token = randomBytes(32).toString('hex');
  return { token, hash: createHash('sha256').update(token).digest('hex') };
}

test.describe('auth and account', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 300_000 });

  test('1: a new account registers and signs in', async ({ page }) => {
    const s = stamp();
    const fresh = {
      email: `pass-${s}@zone.test`,
      password: `Passw0rd!${s}`,
      displayName: `Pass User ${s}`,
    };
    const mark = logMark();
    await page.goto('/register');
    await page.getByLabel('Display Name').fill(fresh.displayName);
    await page.getByLabel('Email').fill(fresh.email);
    await page.getByLabel('Password', { exact: true }).fill(fresh.password);
    await page.getByLabel('Confirm Password').fill(fresh.password);
    await shot(page, '01-register-form');
    await page.getByRole('button', { name: 'Create Account' }).click();
    await expect(page).not.toHaveURL(/\/register/, { timeout: 30_000 });
    await expect(page).not.toHaveURL(/\/login/);
    await shot(page, '01-registered-landing');

    const rows = sql(
      `select id, email_verified from users where email = '${fresh.email}'`,
    );
    expect(rows, 'the account exists in the database').toHaveLength(1);
    const userId = rows[0].split('|')[0];
    writeFileSync(accountFile, JSON.stringify({ ...fresh, userId }, null, 2));

    await page.locator('.logout-btn').click();
    await expect(page).toHaveURL(/\/login/, { timeout: 30_000 });
    await signInAs(page, fresh.email, fresh.password);
    await expect(page).not.toHaveURL(/\/login/, { timeout: 30_000 });
    await page.waitForFunction(() =>
      Boolean(localStorage.getItem('manager_access_token')),
    );
    await shot(page, '01-fresh-account-signed-in');
    record(1, {
      result: 'WORKS',
      user_id: userId,
      email: fresh.email,
      verified_at_registration: rows[0].split('|')[1],
      log: logLines(mark, /Created default organization/).slice(-1),
      screenshots: [
        '01-register-form.png',
        '01-registered-landing.png',
        '01-fresh-account-signed-in.png',
      ],
    });
  });

  test('2: email verification with the token the server stored', async ({
    page,
  }) => {
    const account = savedAccount();

    // Registration creates no token; the console's verification banner and its
    // resend button are what a person would use, so the token comes from there.
    await signInAs(page, account.email, account.password);
    await expect(page).not.toHaveURL(/\/login/, { timeout: 30_000 });
    await expect(page.locator('.logout-btn')).toBeVisible({ timeout: 30_000 });
    const bodyText = await page.locator('body').innerText();
    const bannerText =
      (await page
        .locator('[class*="verification-banner"]')
        .first()
        .innerText()
        .catch(() => '')) || '';
    await shot(page, '02-after-registration-banner');
    const resend = page.getByRole('button', { name: /resend/i }).first();
    let tokenPath = '';
    if (await resend.isVisible().catch(() => false)) {
      await resend.click();
      await page.waitForTimeout(2_000);
      tokenPath = 'console banner: resend button';
    } else {
      const sent = await api('POST', '/api/auth/resend-verification', {
        body: { email: account.email },
      });
      tokenPath = `no resend control visible; POST /api/auth/resend-verification -> ${sent.status}`;
    }
    await shot(page, '02-after-resend');
    const stored = sql(
      `select id from email_verification_tokens where user_id = '${account.userId}' order by created_at desc limit 1`,
    );
    expect(
      stored,
      'the server created a verification token on resend',
    ).toHaveLength(1);

    // The emailed link the server would have sent points at /verify, which the
    // console does not route; recorded by visiting it.
    await page.goto('/verify?token=not-a-real-token');
    await page.waitForTimeout(1_500);
    const verifyPathUrl = page.url();
    await shot(page, '02-verify-path-the-mail-would-use');

    const { token, hash } = knownToken();
    sql(
      `update email_verification_tokens set token_hash = '${hash}' where id = '${stored[0]}'`,
    );
    const verifyResponse = api('POST', '/api/auth/verify-email', {
      body: { token: 'peek-only-invalid' },
    });

    await page.goto(`/verify-email?token=${token}`);
    await page.waitForTimeout(4_000);
    const outcome = await page
      .locator('.auth-page, main, body')
      .first()
      .innerText();
    await shot(page, '02-verify-email-outcome');
    const verified = sql(
      `select email_verified from users where id = '${account.userId}'`,
    );
    const consoleSaysVerified = /Email Verified/.test(outcome);
    record(2, {
      result: consoleSaysVerified ? 'WORKS' : 'FAILS',
      cause: consoleSaysVerified
        ? undefined
        : 'product: the server verified the address and answered {"message":"Email verified successfully"} but the console shows Verification Failed because its schema expects a boolean `success`',
      path: `no SMTP: ${tokenPath}; token row read from email_verification_tokens, hash replaced with a known token`,
      console_mentions_verification_after_registration: /verif/i.test(bodyText),
      banner_text: bannerText,
      verify_link_path_lands_on: verifyPathUrl,
      console_outcome: outcome.replace(/\s+/g, ' ').slice(0, 300),
      db: `users.email_verified = ${verified[0]}`,
      invalid_token_api_status: (await verifyResponse).status,
      screenshots: [
        '02-after-registration-banner.png',
        '02-after-resend.png',
        '02-verify-path-the-mail-would-use.png',
        '02-verify-email-outcome.png',
      ],
    });
    expect(verified[0], 'the server marked the address verified').toBe('t');
    expect(
      consoleSaysVerified,
      `the console shows: ${outcome.slice(0, 200)}`,
    ).toBe(true);
  });

  test('3: login, logout, a wrong password refused, and a session that survives a reload', async ({
    page,
  }) => {
    await signIn(page);
    await shot(page, '03-signed-in');
    await page.reload();
    await expect(page).not.toHaveURL(/\/login/, { timeout: 30_000 });
    await expect(page.locator('.logout-btn')).toBeVisible({ timeout: 30_000 });
    await shot(page, '03-after-reload');

    await page.locator('.logout-btn').click();
    await expect(page).toHaveURL(/\/login/, { timeout: 30_000 });
    expect(
      await page.evaluate(() => localStorage.getItem('manager_access_token')),
    ).toBeNull();
    await shot(page, '03-logged-out');

    await signInAs(page, state.owner.email, `${state.owner.password}-wrong`);
    const refusal = page.locator('[data-sonner-toast], .auth-error').first();
    await expect(refusal).toBeVisible({ timeout: 30_000 });
    const refusalText = await refusal.innerText();
    await expect(page).toHaveURL(/\/login/);
    await shot(page, '03-wrong-password-refused');
    const wrong = await api('POST', '/api/auth/login', {
      body: {
        email: state.owner.email,
        password: `${state.owner.password}-wrong`,
      },
    });
    record(3, {
      result: 'WORKS',
      refusal_text: refusalText,
      api_wrong_password_status: wrong.status,
      screenshots: [
        '03-signed-in.png',
        '03-after-reload.png',
        '03-logged-out.png',
        '03-wrong-password-refused.png',
      ],
    });
  });

  test('4: the sessions page lists sessions and revoking the others signs a second browser out', async ({
    page,
    browser,
  }) => {
    const other = await (browser as Browser).newContext();
    const second = await other.newPage();
    await second.goto(`http://localhost:${port}/login`);
    await signIn(second);
    await expect(second.locator('.logout-btn')).toBeVisible();
    const secondToken =
      (await second.evaluate(() =>
        localStorage.getItem('manager_access_token'),
      )) ?? '';
    const secondRefresh =
      (await second.evaluate(() =>
        localStorage.getItem('manager_refresh_token'),
      )) ?? '';

    await signIn(page);
    await page.goto('/sessions');
    await page.waitForTimeout(4_000);
    const pageText = await page.locator('main').innerText();
    const table = page.locator('table.sessions-table');
    const listed = await table.isVisible().catch(() => false);
    const rowsListed = listed ? await table.locator('tbody tr').count() : 0;
    await shot(page, '04-sessions-page');
    const token = await tokenFor(state.owner);
    const raw = await api('GET', '/api/auth/sessions', { token });
    const apiSessions = ((raw.body as { sessions?: unknown[] }).sessions ?? [])
      .length;

    let revokeOutcome = '';
    if (listed && rowsListed >= 2) {
      await page
        .getByRole('button', { name: 'Revoke All Other Sessions' })
        .click();
      const dialog = page.getByRole('dialog');
      await expect(dialog).toContainText('Revoke All Other Sessions');
      await dialog.getByRole('button', { name: 'Confirm' }).click();
      await expect(page.locator('[data-sonner-toast]')).toContainText(
        /revoked/i,
        { timeout: 30_000 },
      );
      revokeOutcome = 'revoked from the console';
    } else {
      // The console cannot list sessions, so the revoke button has nothing to
      // act on; the server's own revoke is exercised so the row still says
      // what the API does for the second browser.
      const revoked = await api('DELETE', '/api/auth/sessions', { token });
      revokeOutcome = `console listed nothing; DELETE /api/auth/sessions -> ${revoked.status}`;
    }
    await shot(page, '04-sessions-after-revoke');

    const active = sql(
      `select count(*) from sessions where user_id = '${state.owner.user.id}' and revoked_at is null`,
    ).join(',');
    const withAccess = await api('GET', '/api/auth/sessions', {
      token: secondToken,
    });
    const refreshed = await api('POST', '/api/auth/refresh', {
      body: { refresh_token: secondRefresh },
    });
    await second.goto(`http://localhost:${port}/chats`);
    await second.waitForTimeout(3_000);
    const secondUrl = second.url();
    await second.screenshot({
      path: join(evidenceDir, '04-second-browser-after-revoke.png'),
    });
    await other.close();

    const signedOut = /\/login/.test(secondUrl) || withAccess.status === 401;
    record(4, {
      result: listed && rowsListed >= 2 && signedOut ? 'WORKS' : 'FAILS',
      cause: listed
        ? undefined
        : 'product: /sessions renders "No active sessions found" with a validation error, because the console schema requires user_id and location that GET /api/auth/sessions does not send',
      sessions_page_text: pageText.replace(/\s+/g, ' ').slice(0, 400),
      sessions_listed_in_console: rowsListed,
      sessions_from_api: apiSessions,
      revoke: revokeOutcome,
      active_sessions_in_db_after: active,
      second_browser_access_token_status_after_revoke: withAccess.status,
      second_browser_refresh_status_after_revoke: refreshed.status,
      second_browser_url_after_revoke: secondUrl,
      screenshots: [
        '04-sessions-page.png',
        '04-sessions-after-revoke.png',
        '04-second-browser-after-revoke.png',
      ],
    });
    expect(
      listed,
      `the sessions page lists sessions: ${pageText.slice(0, 200)}`,
    ).toBe(true);
    expect(signedOut, 'the second browser is signed out').toBe(true);
  });

  test('5: forgot password, then reset with the token, then sign in with the new one', async ({
    page,
  }) => {
    const account = savedAccount();
    await page.goto('/forgot-password');
    await page.getByLabel('Email').fill(account.email);
    await page.getByRole('button', { name: /send|reset/i }).click();
    await page.waitForTimeout(4_000);
    const forgotOutcome = (
      await page.locator('.auth-page, main, body').first().innerText()
    ).replace(/\s+/g, ' ');
    await shot(page, '05-forgot-password-sent');

    const stored = sql(
      `select id from password_reset_tokens where user_id = '${account.userId}' and used_at is null and expires_at > now() order by created_at desc limit 1`,
    );
    expect(stored, 'the server created a reset token').toHaveLength(1);
    const { token, hash } = knownToken();
    sql(
      `update password_reset_tokens set token_hash = '${hash}' where id = '${stored[0]}'`,
    );

    const newPassword = `${account.password}-new`;
    await page.goto(`/reset-password?token=${token}`);
    await page.getByLabel('New Password').fill(newPassword);
    await page.getByLabel('Confirm Password').fill(newPassword);
    await shot(page, '05-reset-form');
    await page.getByRole('button', { name: /reset|set/i }).click();
    await page.waitForTimeout(4_000);
    const outcome = await page
      .locator('.auth-page, main, body')
      .first()
      .innerText();
    await shot(page, '05-reset-outcome');

    const oldRefused = await api('POST', '/api/auth/login', {
      body: { email: account.email, password: account.password },
    });
    const newAccepted = await api('POST', '/api/auth/login', {
      body: { email: account.email, password: newPassword },
    });
    const used = sql(
      `select used_at is not null from password_reset_tokens where id = '${stored[0]}'`,
    );
    let signedIn = false;
    if (newAccepted.status === 200) {
      await signInAs(page, account.email, newPassword);
      await expect(page).not.toHaveURL(/\/login/, { timeout: 30_000 });
      signedIn = true;
      await shot(page, '05-signed-in-with-new-password');
      writeFileSync(
        accountFile,
        JSON.stringify({ ...account, password: newPassword }, null, 2),
      );
    }
    const consoleSaysSent = /Check your email/.test(forgotOutcome);
    const consoleSaysReset = /Password Reset Successful/.test(outcome);
    record(5, {
      result:
        consoleSaysSent &&
        consoleSaysReset &&
        signedIn &&
        oldRefused.status === 401
          ? 'WORKS'
          : 'FAILS',
      cause:
        consoleSaysSent && consoleSaysReset
          ? undefined
          : `product: the console reports a failure where the server succeeded (forgot: "${forgotOutcome.slice(0, 160)}"; reset: "${outcome.replace(/\s+/g, ' ').slice(0, 160)}")`,
      forgot_page_outcome: forgotOutcome.slice(0, 200),
      path: 'no SMTP: token row read from password_reset_tokens, hash replaced with a known token',
      console_outcome: outcome.replace(/\s+/g, ' ').slice(0, 200),
      old_password_status: oldRefused.status,
      new_password_status: newAccepted.status,
      token_marked_used: used[0],
      screenshots: [
        '05-forgot-password-sent.png',
        '05-reset-form.png',
        '05-reset-outcome.png',
        '05-signed-in-with-new-password.png',
      ],
    });
    expect(newAccepted.status).toBe(200);
    expect(oldRefused.status).toBe(401);
    expect(consoleSaysSent, forgotOutcome.slice(0, 200)).toBe(true);
    expect(consoleSaysReset, outcome.slice(0, 200)).toBe(true);
  });

  test('6: an invitation is sent, accepted at /invitations, and the member appears with the role', async ({
    page,
    browser,
  }) => {
    // An earlier run may already have made the second tenant a member, and the
    // server refuses to invite a member, so the membership is removed first.
    const ownerToken = await tokenFor(state.owner);
    const removed = await api(
      'DELETE',
      `/api/organizations/${state.owner.organization.id}/members/${state.intruder.user.id}`,
      { token: ownerToken },
    );
    // The API soft-deletes the row and a soft-deleted member still counts as
    // "already a member" for a new invitation, so the row is cleared outright.
    sql(
      `delete from organization_members where organization_id = '${state.owner.organization.id}' and user_id = '${state.intruder.user.id}'`,
    );
    sql(
      `delete from workspace_members where workspace_id = '${state.owner.workspace.id}' and user_id = '${state.intruder.user.id}'`,
    );
    // Invitations are unique per address and organization, accepted ones
    // included, so an earlier run's row would make a new invitation a 409.
    const earlierInvitations = sql(
      `delete from invitations where organization_id = '${state.owner.organization.id}' and email = '${state.intruder.email}' returning accepted_at is not null`,
    );
    await signIn(page);
    await page.goto('/org-settings');
    await page.getByRole('tab', { name: 'Invitations' }).click();
    page.on('dialog', (dialog) => dialog.accept());
    // A pending invitation for the same address from an earlier run blocks a new
    // one (the table is unique per email and organization), so it is revoked first.
    const pending = page.locator('table.invitations-table tbody tr', {
      hasText: state.intruder.email,
    });
    let revokedEarlier = 'none pending';
    if (await pending.count()) {
      await pending.getByRole('button', { name: 'Revoke' }).click();
      await expect(pending).toHaveCount(0, { timeout: 30_000 });
      revokedEarlier = 'revoked from the console';
    } else {
      const list = await api(
        'GET',
        `/api/organizations/${state.owner.organization.id}/invitations`,
        { token: ownerToken },
      );
      const rows = Array.isArray(list.body)
        ? (list.body as { id: string; email: string }[])
        : [];
      for (const row of rows.filter((r) => r.email === state.intruder.email)) {
        const gone = await api(
          'DELETE',
          `/api/organizations/${state.owner.organization.id}/invitations/${row.id}`,
          { token: ownerToken },
        );
        revokedEarlier = `console table showed no pending row; DELETE ${row.id} -> ${gone.status}`;
      }
    }
    const tableText = (await page.locator('main').innerText()).replace(
      /\s+/g,
      ' ',
    );
    await page.getByRole('button', { name: 'Invite Member' }).click();
    const modal = page.locator('.modal-content[role="dialog"]');
    await expect(modal).toBeVisible();
    await modal.locator('#email').fill(state.intruder.email);
    await modal.locator('#org-role').selectOption('member');
    await modal
      .locator('#workspace')
      .selectOption({ label: state.owner.workspace.name });
    await modal.locator('#workspace-role').selectOption('member');
    await shot(page, '06-invite-form');
    const created = page.waitForResponse(
      (r) =>
        /\/api\/organizations\/[^/]+\/invitations$/.test(r.url()) &&
        r.request().method() === 'POST',
    );
    await modal.getByRole('button', { name: 'Send Invitation' }).click();
    const response = await created;
    expect(response.status()).toBe(201);
    const invitation = (await response.json()) as {
      id: string;
      token?: string;
    };
    expect(
      invitation.token,
      'the create response carries the token the mail would',
    ).toBeTruthy();
    await expect(page.locator('table.invitations-table')).toContainText(
      state.intruder.email,
      {
        timeout: 30_000,
      },
    );
    await shot(page, '06-invitation-pending');

    const other = await (browser as Browser).newContext();
    const second = await other.newPage();
    await second.goto(`http://localhost:${port}/login`);
    await signIn(second, state.intruder);
    await second.goto(`/invitations?token=${invitation.token}`);
    await second.waitForTimeout(4_000);
    const acceptText = await second.locator('body').innerText();
    await second.screenshot({
      path: join(evidenceDir, '06-invitation-accept-page.png'),
    });
    const intruderToken = await tokenFor(state.intruder);
    const detail = await api('GET', `/api/invitations/${invitation.token}`, {
      token: intruderToken,
    });
    let acceptedVia = '';
    if (
      await second
        .getByRole('button', { name: 'Accept Invitation' })
        .isVisible()
        .catch(() => false)
    ) {
      await second.getByRole('button', { name: 'Accept Invitation' }).click();
      await expect(second).toHaveURL(/\/org-settings/, { timeout: 30_000 });
      acceptedVia = 'console-accept: Accept Invitation';
    } else {
      // The page could not render the invitation, so the server's accept is
      // called directly to give the later rows the membership they need.
      const accepted = await api(
        'POST',
        `/api/invitations/${invitation.token}/accept`,
        { token: intruderToken },
      );
      acceptedVia = `console showed no Accept button; POST /api/invitations/{token}/accept -> ${accepted.status}`;
    }
    await other.close();

    await page.getByRole('tab', { name: 'Members' }).click();
    await page.waitForTimeout(3_000);
    const membersText = (await page.locator('main').innerText()).replace(
      /\s+/g,
      ' ',
    );
    const row = page.locator('table.members-table tbody tr', {
      hasText: state.intruder.email,
    });
    const rowShown = (await row.count()) > 0;
    const badge = rowShown
      ? await row.locator('.role-badge').innerText()
      : 'no row carries the member email';
    const memberRows = await page
      .locator('table.members-table tbody tr')
      .count();
    await shot(page, '06-member-listed');
    const membership = sql(
      `select role from organization_members where organization_id = '${state.owner.organization.id}' and user_id = '${state.intruder.user.id}'`,
    );
    const consoleAccepted = acceptedVia.startsWith('console-accept');
    record(6, {
      result: consoleAccepted && rowShown ? 'WORKS' : 'FAILS',
      cause: consoleAccepted
        ? rowShown
          ? undefined
          : 'product: the member is in the database but the Members table does not show them by email'
        : 'product: /invitations?token= shows "Invalid Invitation" with a validation error, because the console schema requires workspace_name and invited_by_email that GET /api/invitations/{token} does not send',
      invitation_id: invitation.id,
      earlier_membership_removed_status: removed.status,
      members_tab_text: membersText.slice(0, 300),
      member_rows_rendered: memberRows,
      member_row_with_email_rendered: rowShown,
      earlier_pending_invitation: revokedEarlier,
      earlier_invitation_rows_cleared_by_sql: earlierInvitations,
      note: 'The server refuses a second invitation for an address that was invited before, even after the member was removed (unique email per organization); the earlier row was cleared directly for this run.',
      invitations_tab_before_invite: tableText.slice(0, 200),
      accept_page_text: acceptText.replace(/\s+/g, ' ').slice(0, 300),
      invitation_api_status: detail.status,
      invitation_api_keys: Object.keys(
        (detail.body as Record<string, unknown>) ?? {},
      ),
      accepted_via: acceptedVia,
      member_badge: badge,
      membership_role_in_db: membership.join(','),
      screenshots: [
        '06-invite-form.png',
        '06-invitation-pending.png',
        '06-invitation-accept-page.png',
        '06-member-listed.png',
      ],
    });
    expect(membership.join(',')).toBe('member');
    expect(consoleAccepted, acceptText.slice(0, 200)).toBe(true);
  });

  test('7: a member without the role is refused the page and the API call', async ({
    page,
  }) => {
    const membership = sql(
      `select role from organization_members where organization_id = '${state.owner.organization.id}' and user_id = '${state.intruder.user.id}'`,
    );
    expect(membership, 'row 6 made the second tenant a member').toEqual([
      'member',
    ]);
    const asMember: Tenant = {
      ...state.intruder,
      organization: state.owner.organization,
      workspace: state.owner.workspace,
    };
    await signIn(page, asMember);
    await page.goto('/org-settings');
    await page.waitForTimeout(4_000);
    const url = page.url();
    const text = await page.locator('main, body').first().innerText();
    const orgShown = await page
      .locator('.sidebar, aside')
      .first()
      .innerText()
      .catch(() => '');
    await shot(page, '07-member-at-org-settings');

    const token = await tokenFor(state.intruder);
    const settings = await api(
      'GET',
      `/api/organizations/${state.owner.organization.id}/settings/ai`,
      { token },
    );
    const rename = await api(
      'PATCH',
      `/api/organizations/${state.owner.organization.id}`,
      {
        token,
        body: { name: 'Hijacked by a member' },
      },
    );
    const promote = await api(
      'PATCH',
      `/api/organizations/${state.owner.organization.id}/members/${state.intruder.user.id}`,
      { token, body: { role: 'owner' } },
    );
    const name = sql(
      `select name from organizations where id = '${state.owner.organization.id}'`,
    );
    const pageRefused =
      /\/unauthorized/.test(url) || /Access Denied/.test(text);
    const apiRefused =
      [401, 403].includes(rename.status) && [401, 403].includes(promote.status);
    record(7, {
      result: pageRefused && apiRefused ? 'WORKS' : 'FAILS',
      cause: pageRefused
        ? undefined
        : 'product: a member of the organization opens /org-settings and sees its settings; only the API refuses the writes',
      url_after_visit: url,
      page_text: text.replace(/\s+/g, ' ').slice(0, 300),
      sidebar_context: orgShown.replace(/\s+/g, ' ').slice(0, 120),
      read_settings_status: settings.status,
      rename_status: rename.status,
      promote_status: promote.status,
      organization_name_after: name[0],
      screenshots: ['07-member-at-org-settings.png'],
    });
    expect(
      apiRefused,
      `rename=${rename.status} promote=${promote.status}`,
    ).toBe(true);
    expect(name[0]).not.toContain('Hijacked');
    expect(pageRefused, `the page was not refused: ${url}`).toBe(true);
  });
});
