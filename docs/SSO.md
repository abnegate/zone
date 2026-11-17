# Single sign-on

Zone can sign people in through your company's identity provider (IdP) with
OpenID Connect (OIDC). This page covers Okta, Microsoft Entra ID and Google
Workspace. Other OIDC providers can work too if they meet the
[requirements](#what-zone-needs-from-a-provider) below.

Single sign-on applies to the whole server. A server admin configures it
under **Settings → This server → Single sign-on**, and only server admins
see that section.

## Licensing

Single sign-on is part of Zone Team and Zone Enterprise. To add or change a
provider, or to turn enforcement on, the server needs a Team or Enterprise
license that's either active or in its 30-day grace period
after expiry. You install the license under **Settings → This server →
License**.

| License state | Sign-in with SSO | Enforcement | Settings |
|---|---|---|---|
| Team or Enterprise, active or in grace | Works | Works | Editable |
| Lapsed (past grace) | Keeps working | Keeps working | Read-only, with a renewal warning |
| Removed, or never installed (Community) | Keeps working for providers already set up | Keeps working if it was on | Read-only, with an upgrade notice |

Without an active or grace license, a change is refused with
`license_required`, except for changes that only reduce what SSO can do:

- disabling a provider, with a request that changes nothing else
- deleting a provider
- turning enforcement off

So if a provider is compromised after your license lapses, you can still
turn enforcement off and disable or delete the provider. The console shows
the settings read-only without a license, so make these changes through the
[API](#api).

On Community, the section shows an upgrade notice that links to the License
section, and lists any providers that already exist as read-only.

An SSO user who signs in counts toward licensed seats in the same way as a
user who signs in with a password.

## Before you start

You need:

- A Zone server admin account with a password you can use. This is your
  [break-glass](#enforcement-and-break-glass) account.
- [`APP_BASE_URL`](CONFIGURATION.md#app_base_url) set to the origin your
  browsers use to reach Zone, such as `https://zone.example.com`.
- Outbound HTTPS from the Zone server to your identity provider. The
  server, not the browser, fetches the provider's discovery document, keys
  and tokens.
- Admin access to your identity provider, so that you can register an
  application.

### The redirect URI

Every provider needs Zone's redirect URI. It's always:

```
{APP_BASE_URL}/sso/callback
```

Zone takes only the scheme, host and port of `APP_BASE_URL`, because the
console is served at the root of its origin: with
`APP_BASE_URL=https://zone.example.com` or `https://zone.example.com/zone`,
the redirect URI is `https://zone.example.com/sso/callback`. The Single
sign-on section shows the exact value with a copy button. Register it with
your provider exactly as shown, because providers compare it character for
character.

If you change `APP_BASE_URL` later, the redirect URI changes with it, and
you have to update it at every identity provider.

Identity providers generally refuse a plain `http` redirect URI on any host
other than `localhost`, so serve Zone over TLS and use an `https` origin.

Zone requests the scopes `openid email profile`. It uses the authorization
code flow with PKCE (S256), `state` and `nonce`, and authenticates to the
provider's token endpoint with the client secret.

## Adding a provider in Zone

Once you've registered Zone with your identity provider, open **Settings →
This server → Single sign-on**, choose **Add provider** and fill in:

| Field | What to enter |
|---|---|
| Display name | The label on the sign-in button, as in "Sign in with Okta". 1 to 64 characters. |
| Issuer URL | The provider's issuer. See the provider sections below. |
| Client ID | The client or application ID from the provider. |
| Client secret | The client secret from the provider. |
| Allowed domains | Optional. The email domains that may sign in, such as `acme.com`. See [Allowed domains](#allowed-domains). |
| Require verified email | On by default. See [Verified email](#verified-email). |
| Enabled | Whether the provider's button shows on the sign-in page and its sign-ins are accepted. |

When you save, Zone fetches `{issuer}/.well-known/openid-configuration` and
checks that the document's `issuer` value matches the Issuer URL you entered
exactly. If the server can't reach the provider, or the two values differ
(even by a trailing slash), the save fails with `sso_discovery_failed` and a
message that names the problem. The easiest way to get it right is to open
the discovery URL in a browser and copy its `issuer` value.

Zone also checks these rules when you save:

- The issuer must use `https`. Plain `http` is accepted only for a loopback
  host: `localhost`, `127.0.0.0/8` or `::1`. A private or LAN host is fine
  over `https`. The authorization, token and key (`jwks_uri`) endpoints
  that the discovery document lists must follow the same rule, both when
  you save and at every sign-in.
- The issuer can't contain a query string or fragment.
- The same issuer and client ID can't be added twice.
- Each allowed domain must be a plain domain name, without `@` or `*`. Zone
  trims and lowercases each one and removes duplicates.

### The client secret

Zone encrypts the client secret with the server's master key before storing
it. The API never returns it, the console never shows it after you save,
and the audit log never records it.

To rotate a secret, create a new one at the provider, edit the provider in
Zone and paste the new secret. If you leave the client secret field blank
when editing, Zone keeps the current secret. Rotate secrets before the
provider expires them, because an expired secret makes every sign-in fail
with `sso_token_exchange_failed`.

### Changing the issuer or client ID

A provider's subject (`sub`) is only unique within one issuer, and some
providers, such as Entra ID, give each application its own subject for the
same person. So when you change a provider's Issuer URL or client ID, Zone
unlinks every account linked through that provider, in the same save. On
each person's next sign-in, Zone matches them by email again, as described
in [Accounts](#accounts-matching-linking-and-jit-provisioning). The
`sso_provider.updated` audit entry records how many links were removed as
`identities_cleared`.

Changing any other setting, the client secret included, keeps every link.

## Okta

### Register Zone in Okta

1. In the Okta Admin Console, go to **Applications and Resources →
   Applications** and choose **Create App Integration**.
2. Choose **OIDC - OpenID Connect** as the sign-in method and **Web
   Application** as the application type, then choose **Next**.
3. Give the integration a name, such as Zone.
4. Keep the **Authorization Code** grant type.
5. Under **Sign-in redirect URIs**, enter Zone's redirect URI,
   `{APP_BASE_URL}/sso/callback`.
6. Under **Assignments**, choose who may use Zone, for example with **Limit
   access to selected groups**.
7. Choose **Save**.
8. On the **General** tab, under **Client Credentials**, copy the **Client
   ID**. Make sure **Client authentication** is set to **Client secret**,
   then copy the secret.

### Okta values in Zone

- **Issuer URL**: your Okta domain, such as `https://acme.okta.com`. This is
  the issuer of Okta's org authorization server, which Okta recommends for
  single sign-on to OIDC apps.
  - If you use a custom authorization server instead, the issuer is
    `https://acme.okta.com/oauth2/{authorization-server-id}`, for example
    `https://acme.okta.com/oauth2/default`.
  - If your org uses a custom Okta domain, copy the `issuer` from the
    discovery document.
- **Client ID** and **Client secret**: from step 8.

### Okta and verified email

Okta's documentation says that an ID token returned alongside an access
token may not carry every claim for the requested scopes, and Okta's
support articles say that `email_verified` in particular may only be
available from userinfo. Zone handles this as OpenID Connect Core section
5.3 describes: when the ID token has no `email`, or has no
`email_verified` while **Require verified email** is on, Zone asks Okta's
userinfo endpoint for the missing claims. See
[Claims from userinfo](#claims-from-userinfo).

You don't need to turn off **Require verified email** for Okta. Sign in
once with a test user before you roll out.

## Microsoft Entra ID

### Register Zone in Entra ID

1. Sign in to the [Microsoft Entra admin center](https://entra.microsoft.com)
   and go to **Entra ID → App registrations → New registration**.
2. Enter a name, such as Zone.
3. Under **Supported account types**, choose **Single tenant only**.
4. Choose **Register**.
5. On the app's **Overview** page, copy the **Application (client) ID** and
   the **Directory (tenant) ID**.
6. Under **Manage → Authentication**, choose **Add Redirect URI**, then the
   **Web** platform. Enter Zone's redirect URI,
   `{APP_BASE_URL}/sso/callback`, and choose **Configure**. Use the **Web**
   platform, not **Single-page application**, because the Zone server
   redeems the code with a client secret.
7. Under **Manage → Certificates & secrets → Client secrets**, choose **New
   client secret**, pick an expiry and choose **Add**. Copy the secret's
   **Value**, not its **Secret ID**. Entra shows the value only once, and a
   secret lasts at most 24 months.
8. Under **Manage → Token configuration**, choose **Add optional claim**,
   choose the **ID** token type, select **email** and choose **Add**.

To let only selected people sign in, open the app under **Entra ID →
Enterprise apps**, set **Assignment required?** to **Yes** in its
**Properties**, and assign users or groups. Otherwise, anyone in the tenant
can sign in.

### Entra values in Zone

- **Issuer URL**: `https://login.microsoftonline.com/{tenant-id}/v2.0`,
  where `{tenant-id}` is the **Directory (tenant) ID** from step 5, a GUID.
  - Don't use `common` or `organizations` in place of the tenant ID. Their
    discovery documents carry a `{tenantid}` placeholder in the issuer, so
    the issuer check fails.
  - Don't use the tenant's domain name, such as `contoso.onmicrosoft.com`,
    either. Its discovery document names the issuer by the tenant GUID, so
    the issuer check fails there too.
- **Client ID**: the **Application (client) ID**.
- **Client secret**: the secret's **Value** from step 7.

### Entra and verified email

Entra ID doesn't send the `email_verified` claim. Its `email` claim comes
from the user's mail attribute, which Microsoft says isn't guaranteed to be
correct. Without `email_verified`, Zone asks Entra's userinfo endpoint,
which doesn't send it either, and then treats the email as unverified, so
sign-in fails with `sso_email_unverified` while **Require verified email**
is on. There are two ways to handle this:

- **Turn off Require verified email and set Allowed domains.** This is what
  the console recommends. Zone then trusts the email Entra sends to create
  accounts, and the allowed domains limit which addresses can sign in at
  all. It never links an Entra sign-in to an existing Zone account by
  email, so people who already have a Zone account can't use Entra with
  it. Read [Verified email](#verified-email) first.
- **Keep Require verified email on and add the `xms_edov` optional claim**
  to the ID token, in the same way as `email` in step 8. When
  `email_verified` is missing, Zone counts the email as verified if
  `xms_edov` is `true`. Entra sends `true` only when it considers the
  email's domain verified. If your users' sign-ins still fail with
  `sso_email_unverified`, use the first option instead.

A user with no mail attribute gets no `email` claim and can't sign in
(`sso_email_missing`).

## Google Workspace

### Register Zone in Google Cloud

1. In the Google Cloud console, choose or create a project that belongs to
   your Google Workspace organization.
2. Open **Google Auth Platform**. If you're asked to set up the app's
   branding first, do so.
3. Under **Audience**, set the user type to **Internal**. Google then lets
   only members of your organization sign in to Zone.
4. Under **Clients**, choose **Create client** and pick **Web application**
   as the application type.
5. Under **Authorized redirect URIs**, add Zone's redirect URI,
   `{APP_BASE_URL}/sso/callback`. Google requires redirect URIs to use
   `https`.
6. Choose **Create**, and copy the client ID and client secret. Google shows
   the full secret only when the client is created.

### Google values in Zone

- **Issuer URL**: `https://accounts.google.com`
- **Client ID** and **Client secret**: from step 6.
- **Allowed domains**: your Workspace domains, such as `acme.com`. Set
  them, for the reason explained below.
- **Require verified email**: leave it on. Google sends `email_verified`.

Google's ID tokens name their issuer as either `https://accounts.google.com`
or `accounts.google.com`, and Google documents both. For the issuer
`https://accounts.google.com`, Zone accepts either form. Every other
issuer must match the ID token's `iss` exactly.

### Google and the `hd` claim

Google puts the domain of the user's Google Workspace or Cloud organization
in the ID token's `hd` claim. Personal Google accounts don't have one. When
a Google provider has allowed domains, Zone requires `hd` to be one of them,
in addition to checking the email's domain. Personal Gmail accounts and
accounts from other Workspace organizations are therefore refused with
`sso_domain_not_allowed`. List every domain your organization uses, so that
both the `hd` value and your users' email domains are covered.

Zone checks `hd` in the same way for any other provider whose ID token
carries it, as long as allowed domains are set.

Without allowed domains, Zone doesn't check `hd`, and any Google account
that Google lets through gets a Zone account. The **Internal** user type
limits that to your organization, but set allowed domains anyway.

## How sign-in works

1. Enabled providers appear as **Sign in with {display name}** buttons on
   the sign-in page. If someone opens the console at an origin other than
   `APP_BASE_URL`, the button first sends them to `{APP_BASE_URL}/login`,
   because sign-in has to start and finish on the same origin.
2. Zone sends the browser to the provider. Each sign-in attempt can be used
   once and expires after 10 minutes. Starting a sign-in needs no account,
   so Zone keeps at most 10,000 unexpired attempts and answers 429 to more
   until some finish or expire.
3. The provider sends the browser back to `/sso/callback`.
4. The console hands the code to the Zone server. The server redeems it at
   the provider's token endpoint and verifies the ID token:
   - its signature, against the provider's published keys
   - its issuer and audience
   - its expiry and issue time, with 60 seconds of leeway
   - its nonce
5. If the ID token leaves out a claim the policy needs, and the provider
   publishes a userinfo endpoint, Zone fills it in from there. See
   [Claims from userinfo](#claims-from-userinfo).
6. Zone applies the provider's policy (email present, verified, in an
   allowed domain), finds or creates the Zone account as described below,
   and signs the user in.

### Claims from userinfo

Some providers, Okta among them, may leave `email` or `email_verified` out
of the ID token. When the ID token has no `email`, or has no
`email_verified` while **Require verified email** is on, and the discovery
document lists a `userinfo_endpoint`, Zone calls that endpoint with the
access token from the same sign-in. Then:

- The userinfo `sub` must equal the ID token's `sub`. Otherwise the
  sign-in is refused with `sso_id_token_invalid`.
- Userinfo only fills claims the ID token left out. A claim the ID token
  has, even `email_verified: false`, is never replaced.
- `email_verified` from userinfo counts only when userinfo's `email` is the
  address the sign-in uses.
- If the userinfo endpoint can't be reached or refuses the access token,
  the sign-in fails with `sso_token_exchange_failed`.

Entra's `xms_edov` already answers whether the email is verified, so a
token that carries it doesn't trigger a userinfo call. The userinfo
endpoint follows the same `https` rule as the provider's other endpoints.

Device approval and disabled accounts work in the same way as for password
sign-in. A disabled account is refused with `account_disabled`, and a
device that's waiting for approval or blocked is refused with
`device_pending` or `device_blocked`.

Zone doesn't receive deprovisioning events from your identity provider.
Removing someone from the app at the provider stops new SSO sign-ins. A
Zone session they already have keeps working until it ends, which is at
most `SESSION_LIFETIME` seconds (30 days by default) after they signed in:
refreshing a session never takes it past that, so they have to sign in
through the provider again. Disabling or deleting the provider ends every
session signed in through it at once.

## Accounts: matching, linking and JIT provisioning

Each time someone signs in, Zone looks for their account in this order.

1. **By subject.** Zone remembers the provider's subject (`sub`) for each
   linked account. A returning user is matched on that alone, even if
   their email has changed at the provider. Zone doesn't change the
   account's email in that case.
2. **By email.** If no account is linked to that subject, and the provider
   has **Require verified email** on, Zone looks for an account with the
   same email, ignoring case. If it finds one, it links that account to the
   subject, and the user signs in to their existing account.
3. **Just-in-time (JIT) provisioning.** If no account matches, Zone
   creates one.

Linking is by email, so it's only as trustworthy as the email the provider
sends. With **Require verified email** on, the provider must say the email
is verified before Zone links or creates anything.

Email addresses are unique ignoring case: `Jane@acme.com` and
`jane@acme.com` are the same account, and Zone stores new addresses in
lower case.

Zone refuses the sign-in with `sso_email_conflict` when an account has the
email and:

- **Require verified email** is off. Zone never links an existing account,
  admin or not, through a provider that doesn't vouch for the email.
  Otherwise anyone who can change their email at the provider, such as an
  Entra user editing their mail attribute, could take over a colleague's
  account.
- the account is already linked to an identity at any provider. Once an
  account signs in through one provider, another provider can't claim it
  by email. This also happens when the provider starts sending a new
  subject for the same person without the issuer or client ID changing in
  Zone. To link everyone again by email, delete the provider and add it
  again.

An account can be linked to one identity per provider.

### Reclaiming unverified accounts

If the matching Zone account is not a server admin and its email was never
verified, Zone treats the identity provider as the real owner of that email
and takes the account over:

- It replaces the account's password with a random one.
- It marks the email as verified.
- It revokes all of the account's sessions and refresh tokens, and refuses
  any password sign-in that was still in progress with the old password.
- It records `reclaimed: true` in a `user.sso_linked` audit entry. Zone
  writes that entry as soon as the account is taken over, even if the
  sign-in itself is then refused, for example because the device is
  waiting for approval.

This stops someone from registering a colleague's address before SSO is set
up and then keeping access to that account. To use a password again
afterwards, the reclaimed user has to go through password reset, which
needs account mail (see
[`SMTP_HOST`](CONFIGURATION.md#smtp_host-smtp_port-smtp_user-smtp_password-smtp_from-smtp_from_name))
and isn't available to them while SSO is required. On a server without
account mail, a reclaimed user can only get back in through SSO.

Server admin accounts are never reclaimed. An admin whose email isn't
verified still links, as long as **Require verified email** is on, and
keeps their password and sessions.

A local account whose email is already verified is linked without any
other change. Its password keeps working while SSO isn't required.

### JIT-provisioned accounts

A new account created at sign-in:

- uses the email from the ID token, marked as verified
- takes its display name from the provider's `name` claim when there is one
- is a regular user, never a server admin
- gets its own default organization and workspace, as a new registration
  does
- has a random password that nobody knows. While SSO isn't required, the
  user can set a password through password reset, if the server sends
  account mail.

JIT users don't join your company's organization automatically. Invite
them from the organization's **Members** settings.

## Allowed domains

When allowed domains are set, only emails at those domains can sign in or
be provisioned through that provider. Everyone else is refused with
`sso_domain_not_allowed`.

**Without allowed domains, every identity your provider lets through gets
a Zone account.** That's fine when the provider only admits your own staff,
for example through Okta assignments, Entra's **Assignment required?** or
Google's **Internal** user type. In any other case, set allowed domains.
For Google, always set them, so that `hd` is checked.

## Verified email

**Require verified email** is on by default. While it's on, the ID token,
or userinfo when the ID token leaves the claim out, must say the email is
verified: `email_verified` must be `true`, or, when `email_verified` is
missing, Entra's `xms_edov` must be `true`. Otherwise the sign-in is
refused with `sso_email_unverified`.

When you turn it off, Zone trusts whatever email the provider sends when
it creates an account, but it never links a sign-in to an existing account
by email. Someone who signs in for the first time with the email of an
existing Zone account is refused with `sso_email_conflict`. To move such a
person to SSO, either turn the setting on for long enough that they sign in
once (for Entra, with the `xms_edov` claim), or delete their Zone account
so that SSO creates a new one. Always set allowed domains when this setting
is off.

## Enforcement and break-glass

Turning on **Require single sign-on** makes SSO the only way in for
everyone except server admins:

| Action | Non-admins | Server admins |
|---|---|---|
| Sign in with a password | Refused with `sso_required` | Allowed, and recorded as a break-glass sign-in |
| Create an account | Refused with `sso_required` | Refused with `sso_required` |
| Forgot password | Same response as usual, but no reset is created and no mail is sent | Works as usual |
| Reset password | Refused with `sso_required`; the reset link and the password stay unchanged | Works as usual |

While it's on, the sign-in page says that password sign-in is for server
admins, hides the "Create one" link, and labels the password reset link
"Server admin? Forgot password?". The invitation page hides **Create
Account** but keeps sign-in.

A password sign-in is refused with `sso_required` before Zone checks the
password, for every email that isn't a server admin's, including emails
with no account. The answer doesn't show whether the password was right.

A server admin is an active user with the server admin flag, the same rule
the rest of the server uses. Every password sign-in by a server admin while
SSO is required is written to the audit log as
`user.break_glass_signed_in`. That's your record of who went around SSO,
and when.

Rules for turning it on and off:

- You can turn enforcement on only while at least one provider is enabled.
- While it's on, you can't disable or delete the last enabled provider.
  Either change is refused with `sso_provider_required`.
- Turning it on needs an active or grace license. Turning it off doesn't.

Things to know:

- Zone records how each session started: with a password or through a
  provider. While SSO is required, a password session that doesn't belong
  to a server admin can't be refreshed. The refresh is refused with
  `sso_required` and the session is revoked, so someone who signed in with
  a password before you turned enforcement on is signed out within the
  access token lifetime (`JWT_ACCESS_LIFETIME`, 15 minutes by default) and
  has to sign in with SSO.
- Every session ends `SESSION_LIFETIME` seconds after its sign-in (30 days
  by default), however often it's refreshed.
- Before you turn enforcement on, sign in with SSO yourself, and check that
  a server admin can still sign in with a password. If your identity
  provider goes down, that password is the only way in.

### The command line

`zone login` signs in with a password only. While SSO is required, only
server admins can sign in from the CLI. Everyone else sees:

```
Server error: This server requires single sign-on. Sign in from the Zone console in your browser; only server admins can sign in with a password.
```

Single sign-on for the CLI is planned for a later release.

## Audit log

Zone writes these server-wide entries, with no organization, to the audit
log. None of them ever contains a client secret.

| Action | When | Recorded values |
|---|---|---|
| `sso_provider.created` | A provider is added | The provider's settings, and `client_secret_changed: true` |
| `sso_provider.updated` | A provider is edited | The changed settings, before and after, plus `client_secret_changed: true` when the secret was replaced, and `identities_cleared` (the number of accounts unlinked) when the issuer or client ID changed |
| `sso_provider.deleted` | A provider is deleted | The provider's settings |
| `sso_enforcement.updated` | Enforcement is turned on or off | `required`, before and after |
| `user.sso_linked` | An identity is linked to an existing account, whether or not the sign-in then gets a session | `provider_id`, `subject`, `reclaimed` |
| `user.sso_signed_in` | Someone signs in with SSO | `provider_id`, `subject`, `provisioned`, `linked`, `reclaimed` |
| `user.break_glass_signed_in` | A server admin signs in with a password while SSO is required | None |

## Disabling and deleting providers

- **Disabling** a provider hides its button, refuses sign-in through it,
  and revokes every session that was signed in through it. Accounts stay
  linked, so enabling it again restores sign-in.
- **Deleting** a provider removes it, every link to it and every session
  signed in through it. The Zone accounts themselves stay. If you add the
  provider again, people are matched by email again on their next sign-in.

Neither needs a license, so you can always take a compromised provider out
of service through the [API](#api).

## Troubleshooting

These codes appear in API responses, and the console shows a message for
each one.

| Code | Meaning | What to check |
|---|---|---|
| `sso_discovery_failed` | Zone couldn't load or accept the provider's discovery document. When this happens at sign-in, the person sees a general message and the server log has the details. | The Issuer URL matches the document's `issuer` exactly. The Zone server can reach the provider. Every endpoint uses `https` or a loopback host. |
| `sso_issuer_invalid` | The Issuer URL isn't valid. | It's an absolute `https` URL with no query or fragment. `http` is allowed only for loopback hosts. |
| `sso_provider_duplicate` | A provider with that issuer and client ID already exists. | Edit the existing provider instead. |
| `sso_provider_not_found` | The provider doesn't exist or is disabled. | The provider is enabled. |
| `sso_state_invalid` | The sign-in attempt is unknown, was already used, or is more than 10 minutes old. | Start again from the sign-in page. |
| `sso_token_exchange_failed` | The provider's token endpoint refused the code, or its userinfo endpoint failed. The person sees a general message and the server log has the details. | The client secret is correct and hasn't expired. The redirect URI registered at the provider is exactly `{APP_BASE_URL}/sso/callback`. |
| `sso_id_token_invalid` | The ID token failed verification: signature, algorithm, issuer, audience, expiry or nonce, or the userinfo `sub` differed from it. | The Zone server's clock is correct. The client ID matches the provider's application. |
| `sso_email_missing` | Neither the ID token nor userinfo has an `email` claim. | The provider puts `email` in the ID token. For Entra, add the `email` optional claim. |
| `sso_email_unverified` | The email isn't marked verified while Require verified email is on. | See [Verified email](#verified-email). |
| `sso_domain_not_allowed` | The email's domain, or the `hd` claim, isn't in the allowed domains. | The provider's allowed domains. |
| `sso_email_conflict` | The account can't be matched safely. | See [Accounts](#accounts-matching-linking-and-jit-provisioning). |
| `sso_required` | Password sign-in, registration or reset was refused because SSO is required. | Sign in with SSO, or as a server admin. |
| `account_disabled` | The Zone account is disabled. | Disabled accounts can't sign in by any method. |
| `license_required` | A change was refused because there's no active or grace Team or Enterprise license. | **Settings → This server → License**. |

Zone accepts ID tokens signed with RS256, RS384, RS512, PS256, PS384,
PS512, ES256, ES384 or EdDSA. When the discovery document lists
`id_token_signing_alg_values_supported`, the token's algorithm must also be
in that list. Unsigned tokens (`none`) and tokens signed with a shared
secret (HS256 and the like) are always refused.

## What Zone needs from a provider

Any OIDC provider can be used if it:

- publishes a discovery document at
  `{issuer}/.well-known/openid-configuration` whose `issuer` matches the
  issuer you enter
- serves its issuer and endpoints over `https`, or from a loopback host
- supports the authorization code flow, and accepts the client secret with
  `client_secret_basic` or `client_secret_post`. Zone uses
  `client_secret_basic` unless the discovery document lists only
  `client_secret_post`.
- signs ID tokens with one of the algorithms above, and publishes its keys
  at its `jwks_uri`
- puts `email` in the ID token, and `email_verified` too if you keep
  **Require verified email** on, or serves the missing claims from a
  `userinfo_endpoint` listed in its discovery document

## API

Server admins can manage SSO through the API as well as the console. A
user who isn't a server admin gets 403.

| Method and path | Purpose |
|---|---|
| `GET /api/sso` | Licensing state, enforcement, the redirect URI and the providers. Never includes secrets. |
| `POST /api/sso/providers` | Add a provider. |
| `PATCH /api/sso/providers/{provider_id}` | Change a provider. Send only the fields you're changing. Leave out `client_secret` to keep the current one. |
| `DELETE /api/sso/providers/{provider_id}` | Delete a provider. |
| `PUT /api/sso/enforcement` | Turn enforcement on or off with `{"required": true}` or `{"required": false}`. |

Every change is checked against the license before anything else, except
the ones listed under [Licensing](#licensing) that only reduce what SSO can
do.

## SAML

Zone doesn't support SAML 2.0 in this release. Native SAML is planned for a
later release.

If your identity provider only speaks SAML, put an OIDC bridge in front of
it. The bridge signs users in through your SAML provider and acts as an
OIDC provider towards Zone, so you add it to Zone like any other OIDC
provider.

- **Keycloak** (recommended): add your SAML provider as a SAML identity
  provider in a Keycloak realm, then register Zone as an OIDC client in
  that realm.
- **Authentik**: add your SAML provider as a SAML source, and register Zone
  with an OIDC provider and application that use that source.
- Dex also has a SAML connector, but Dex's own documentation marks it as
  unmaintained, so we don't recommend it.

Whichever bridge you use, set it up so that:

- the `sub` claim comes from the SAML provider's persistent NameID, or
  another attribute that never changes for a user. Zone links accounts by
  provider and subject, so when the `sub` changes, the person's account is
  still linked to the old one and their sign-in is refused with
  `sso_email_conflict`.
- `email` and `email_verified` are passed through to the ID token. Without
  `email_verified`, turn **Require verified email** off and set allowed
  domains, as described in [Verified email](#verified-email).
- the bridge's OIDC client for Zone has Zone's redirect URI,
  `{APP_BASE_URL}/sso/callback`, registered.

Then add a provider in Zone with the bridge's issuer, such as
`https://keycloak.acme.com/realms/acme` for Keycloak, and the client ID and
secret of the bridge's OIDC client.
