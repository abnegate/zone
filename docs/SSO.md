# Single sign-on

Zone can sign people in through your company's identity provider (IdP) with
OpenID Connect (OIDC) or SAML 2.0. This page covers Okta, Microsoft Entra ID
and Google Workspace over OIDC, and Okta and Microsoft Entra ID over SAML.
Other providers can work too if they meet the requirements for
[OIDC](#what-zone-needs-from-a-provider) or
[SAML](#what-zone-needs-from-a-saml-identity-provider).

Single sign-on applies to the whole server. A server admin configures it
under **Settings → This server → Single sign-on**, and only server admins
see that section.

## Licensing

Single sign-on is part of Zone Enterprise; a Zone Team license doesn't
include it. To add or change a provider, or to turn enforcement on, the
server needs a Zone Enterprise license that verifies and is either active or
in its 30-day grace period after expiry. You install the license under
**Settings → This server → License**.

| License state | Sign-in with SSO | Enforcement | Settings |
|---|---|---|---|
| Enterprise, active or in grace | Works | Works | Editable |
| Enterprise, lapsed (past grace) | Keeps working | Keeps working | Read-only, with a renewal warning |
| Team, in any state | Keeps working for providers already set up | Keeps working if it was on | Read-only, with an upgrade notice |
| Removed, or never installed (Community) | Keeps working for providers already set up | Keeps working if it was on | Read-only, with an upgrade notice |

The table applies to OIDC and SAML providers alike.

Without a Zone Enterprise license that is active or in its grace period, a
change is refused with `license_required`, except for changes that only reduce
what SSO can do, and one that keeps SAML working:

- disabling a provider, with a request that changes nothing else
- deleting a provider
- turning enforcement off
- refreshing a SAML provider's metadata, with a request that changes nothing
  else and metadata that keeps the same entity ID

So if a provider is compromised after your license lapses, you can still
turn enforcement off and disable or delete the provider. The console shows
the settings read-only without a license, so make these changes through the
[API](#api).

The metadata exception exists because a SAML identity provider rotates its
signing certificate from time to time, and Zone only learns about a new
certificate when you refresh the metadata. Without it, a lapsed server would
lose SAML sign-in at the next rotation. If the refreshed metadata names a
different entity ID, the change is refused with `license_required`, because
a new entity ID unlinks every account (see
[Changing the entity ID](#changing-the-entity-id)). See
[Certificate rotation](#certificate-rotation-and-refreshing-metadata).

On Community and Team, the section shows an upgrade notice that links to the
License section, and lists any providers that already exist as read-only.

On Community, the [member cap](LICENSING.md#the-community-member-cap) also
applies to single sign-on, OIDC and SAML alike: once the server has 3 active
members, a first sign-in that would create an account is refused with
`member_cap_reached`. People who already have an account keep signing in,
including, for OIDC, one linked by email.

An SSO user who signs in counts toward licensed seats in the same way as a
user who signs in with a password.

## Before you start

You need:

- A Zone server admin account with a password you can use. This is your
  [break-glass](#enforcement-and-break-glass) account.
- [`APP_BASE_URL`](CONFIGURATION.md#app_base_url) set to the origin your
  browsers use to reach Zone, such as `https://zone.example.com`.
- For OIDC, outbound HTTPS from the Zone server to your identity provider.
  The server, not the browser, fetches the provider's discovery document,
  keys and tokens. For SAML, the server only contacts the provider to fetch
  its metadata when you give a metadata URL; the browser carries every
  sign-in.
- Admin access to your identity provider, so that you can register an
  application.

### The redirect URI

Every OIDC provider needs Zone's redirect URI. It's always:

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

### The SAML service provider URLs

Towards a SAML identity provider, Zone is a service provider (SP) with one
identity for the whole server. Its values are always:

| Value | URL |
|---|---|
| Entity ID | `{APP_BASE_URL}/api/auth/sso/saml/metadata` |
| Assertion Consumer Service (ACS) URL | `{APP_BASE_URL}/api/auth/sso/saml/acs` |
| Metadata URL | `{APP_BASE_URL}/api/auth/sso/saml/metadata` |

The entity ID and the metadata URL are the same address: Zone serves its SP
metadata at its own entity ID. As with the redirect URI, Zone takes only the
scheme, host and port of `APP_BASE_URL`, so with
`APP_BASE_URL=https://zone.example.com/zone` the ACS URL is
`https://zone.example.com/api/auth/sso/saml/acs`. The Single sign-on section
shows all three values with copy buttons, in the **SAML service provider**
card.

`GET {APP_BASE_URL}/api/auth/sso/saml/metadata` needs no sign-in and answers
with `Content-Type: application/samlmetadata+xml`. It describes an
`SPSSODescriptor` with `AuthnRequestsSigned="false"` and
`WantAssertionsSigned="true"`, the persistent and email address NameID
formats, and one HTTP-POST assertion consumer service at the ACS URL. Some
identity providers can import it by URL instead of you typing the values.

If you change `APP_BASE_URL`, all three values change with it, and you have
to update them at every SAML identity provider.

## Adding a provider in Zone

Once you've registered Zone with your identity provider, open **Settings →
This server → Single sign-on** and choose **Add provider**. Under
**Protocol**, choose **OpenID Connect** or **SAML 2.0**. You can't change a
provider's protocol after you add it. For SAML, see
[Adding a SAML provider](#adding-a-saml-provider). For OpenID Connect, fill
in:

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
For SAML providers, the entity ID plays the issuer's part; see
[Changing the entity ID](#changing-the-entity-id).

## Okta

These steps connect Okta over OIDC. For SAML, see
[Set up Okta for SAML](#set-up-okta-for-saml).

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

These steps connect Entra ID over OIDC. For SAML, see
[Set up Entra ID for SAML](#set-up-entra-id-for-saml).

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

## SAML 2.0

Zone implements SAML 2.0 Web Browser SSO as a service provider. Sign-in
always starts at Zone: Zone sends the browser to the identity provider with
an authentication request, and the provider posts a signed response back to
Zone's ACS URL.

### What Zone needs from a SAML identity provider

Any SAML 2.0 identity provider can be used if it:

- publishes metadata with a single `EntityDescriptor` that holds one
  `IDPSSODescriptor` for SAML 2.0, with at least one signing certificate and
  a `SingleSignOnService` for the HTTP-Redirect binding
- serves that single sign-on endpoint over `https`, or from a loopback host
- accepts an unsigned `AuthnRequest` over the HTTP-Redirect binding, and
  answers with a `Response` over the HTTP-POST binding
- signs the assertion, or the response that contains it, with an RSA key of
  at least 2048 bits, using RSA-SHA256, RSA-SHA384 or RSA-SHA512
- sends a NameID that doesn't change for a person and isn't transient
- sends the person's email address in an attribute, or as an email address
  NameID (see [Identity and attributes](#identity-and-attributes))
- doesn't encrypt the assertion

### Adding a SAML provider

Register Zone at your identity provider first, with the
[SAML service provider URLs](#the-saml-service-provider-urls). Then, in
**Settings → This server → Single sign-on**, choose **Add provider**, choose
**SAML 2.0** under **Protocol**, and fill in:

| Field | What to enter |
|---|---|
| Display name | The label on the sign-in button, as in "Sign in with Okta". 1 to 64 characters. |
| Metadata URL or Metadata XML | Either the URL of the identity provider's SAML metadata, or the metadata document itself, pasted in. Give one of the two, not both. |
| Allowed domains | Required for SAML. The email domains that may sign in, such as `acme.com`. |
| Enabled | Whether the provider's button shows on the sign-in page and its sign-ins are accepted. |

There's no client ID, client secret or **Require verified email** for SAML.
Zone reads the identity provider's entity ID, its single sign-on URL and its
signing certificates from the metadata. When you edit a SAML provider, the
console shows those values read-only, with each certificate's SHA-256
fingerprint, so that you can compare them with what your identity provider
shows.

The same SAML entity ID can't be added twice (`sso_provider_duplicate`). An
OIDC provider and a SAML provider may share an issuer string.

### Metadata

Zone reads the metadata once, when you save the provider, and never in the
background. It doesn't check a signature on the metadata itself: it trusts
the metadata because a server admin supplied it, and, for a metadata URL,
because it came over TLS.

When you give a metadata URL, the Zone server fetches it with much the same
rules it uses for an OIDC discovery document:

- The URL must use `https`. Plain `http` is accepted only for a loopback
  host: `localhost`, `127.0.0.0/8` or `::1`. A private or LAN host is fine
  over `https`.
- The URL can't contain credentials or a fragment.
- Zone doesn't follow redirects. Give the final URL.
- The fetch times out after 10 seconds, and the document may be at most
  1 MiB.

In either case, the save fails with `sso_metadata_invalid` and a message
that names the problem when:

- the document isn't well-formed XML encoded as UTF-8, or declares a
  document type (`<!DOCTYPE`), or refers to an entity other than the five
  predefined XML entities. Zone never loads DTDs or external entities.
- its root isn't an `md:EntityDescriptor`. An aggregate
  `md:EntitiesDescriptor` that describes several entities is refused; use
  the metadata of the single identity provider.
- its `entityID` is missing, longer than 1024 characters, or contains
  spaces or control characters. Any URI scheme is fine, so Okta's
  `http://www.okta.com/…` entity IDs work.
- it doesn't have exactly one `IDPSSODescriptor` whose
  `protocolSupportEnumeration` includes SAML 2.0
- it has no `SingleSignOnService` with the HTTP-Redirect binding, or that
  service's `Location` isn't `https` (or a loopback host)
- it has no signing certificate. Zone reads the `ds:X509Certificate` of
  every `KeyDescriptor` whose `use` is `signing` or absent, and ignores keys
  marked `use="encryption"`. Each certificate must hold an RSA key of at
  least 2048 bits.

### Allowed domains and verified email for SAML

A SAML assertion doesn't say whether the identity provider verified the
email address it carries. So for every SAML provider:

- **Require verified email** is always off. A request that asks for it is
  refused with `sso_verified_email_unsupported`.
- At least one allowed domain is required. A provider without one is
  refused with `sso_domain_required`.
- Zone never links a SAML sign-in to an existing Zone account by email.
  Someone whose email already belongs to a Zone account is refused with
  `sso_email_conflict`, as described in [Verified email](#verified-email).
  SAML sign-in either signs in an account already linked to the person's
  NameID or creates a new account.

There's no way to link an existing Zone account to a SAML identity. To move
someone who already has a Zone account to SAML, delete their Zone account so
that SAML creates a new one. Until then, they can keep signing in with a
password, as long as SSO isn't required.

### Identity and attributes

Zone identifies each person by the value of the assertion's `NameID`, the
SAML equivalent of the OIDC `sub`. Configure the identity provider to send a
value that never changes for a person, such as a persistent identifier or an
immutable user ID. A transient NameID is refused with
`sso_assertion_invalid`, because it differs at every sign-in. If the NameID
changes, the person's account is still linked to the old value, so their
next sign-in creates a new account or, if their email already has an
account, is refused with `sso_email_conflict`.

Zone takes the email from the first of these attributes that the assertion
carries, matched by the attribute's `Name`:

1. `email`
2. `http://schemas.xmlsoap.org/ws/2005/05/identity/claims/emailaddress`
3. `urn:oid:0.9.2342.19200300.100.1.3`
4. `mail`

If none of them is present, Zone uses the NameID itself when its format is
`urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress`. An email attribute
with more than one value counts as missing. Without an email, the sign-in
is refused with `sso_email_missing`.

Zone takes the display name from the first of these attributes:

1. `displayName`
2. `http://schemas.microsoft.com/identity/claims/displayname`
3. `urn:oid:2.16.840.1.113730.3.1.241`
4. `name`

Without one, it joins a given name and a surname:

- given name: `firstName`, `givenName`,
  `http://schemas.xmlsoap.org/ws/2005/05/identity/claims/givenname` or
  `urn:oid:2.5.4.42`
- surname: `lastName`, `surname`, `sn`,
  `http://schemas.xmlsoap.org/ws/2005/05/identity/claims/surname` or
  `urn:oid:2.5.4.4`

Zone never uses Entra ID's
`http://schemas.xmlsoap.org/ws/2005/05/identity/claims/name` claim, because
it carries the user principal name rather than a display name.

An account that Zone creates at a SAML sign-in has its email marked as not
verified, because SAML never vouches for it. The person can still set a
password through password reset while SSO isn't required, if the server
sends account mail.

### What Zone checks

Zone accepts a SAML response only at its ACS URL, only over HTTP-POST, and
only as the answer to a sign-in that Zone itself started. Before it uses
anything in the response, Zone checks:

- **The XML.** The response must be well-formed UTF-8 XML 1.0 with no
  document type declaration and no entities beyond the five predefined
  ones.
- **The response.** `Version` is `2.0`. `InResponseTo` equals the ID of the
  request Zone sent for this sign-in. `Destination`, when present, equals
  the ACS URL, and it must be present when the response is signed. `Issuer`,
  when present, equals the provider's entity ID.
- **The status.** A status other than
  `urn:oasis:names:tc:SAML:2.0:status:Success` is refused with
  `sso_sign_in_refused`, before any other check, so that an identity
  provider's error response reaches the person as a refusal rather than an
  invalid assertion.
- **One assertion.** The document contains exactly one `saml:Assertion`,
  and it is a direct child of the response. An `EncryptedAssertion` or
  `EncryptedID` is refused.
- **Signatures.** See [Which signatures Zone accepts](#which-signatures-zone-accepts).
- **The assertion.** It has exactly one `Issuer`, equal to the provider's
  entity ID, and its `IssueInstant` isn't in the future. It has exactly one
  `Subject` with exactly one `NameID`, which isn't transient, and at least
  one `AuthnStatement`.
- **The subject confirmation.** At least one bearer `SubjectConfirmation`
  has a `SubjectConfirmationData` whose `Recipient` equals the ACS URL,
  whose `InResponseTo` equals the request ID and whose `NotOnOrAfter` hasn't
  passed. It may not carry `NotBefore`.
- **The conditions.** The assertion is within its `Conditions` `NotBefore`
  and `NotOnOrAfter`. It has at least one `AudienceRestriction`, and every
  `AudienceRestriction` contains Zone's entity ID. `OneTimeUse` and
  `ProxyRestriction` are allowed. Any other condition is refused, because
  Zone can't honour a condition it doesn't understand.
- **Time.** Every timestamp must be in UTC, written with `Z`, with up to
  nine digits of fractional seconds. Zone allows 60 seconds of clock skew,
  the same leeway it gives OIDC ID tokens.
- **Replay.** Zone remembers each assertion ID per provider until the
  latest `NotOnOrAfter` in the assertion, plus the leeway, and refuses the
  same assertion a second time.

`Issuer`, `Audience` and `NameID` values are trimmed of surrounding
whitespace before they're compared. Every value Zone uses is read from the
assertion whose signature it verified, by walking that element's own
children, never by searching the document. Comments inside a value are
ignored, so a comment injected into a NameID or an email can't shorten it.

Any failed check is refused with `sso_assertion_invalid`. The person sees a
general message, and the server log has the reason, such as "it's meant for
a different service provider". Zone never logs the SAML response itself.

### Which signatures Zone accepts

Zone accepts a signature on the assertion, or on the response that
contains it, or both. At least one must be present, and every signature
present must verify. An unsigned response is refused with
`sso_assertion_invalid`.

A signature counts only when it is a direct child of the response or the
assertion, with at most one per element, and only when its single
`Reference` points at the ID of that same element. Zone verifies:

- the signature algorithm: RSA PKCS#1 v1.5 with SHA-256, SHA-384 or
  SHA-512 (`http://www.w3.org/2001/04/xmldsig-more#rsa-sha256`,
  `…#rsa-sha384` or `…#rsa-sha512`)
- the digest algorithm: SHA-256, SHA-384 or SHA-512
  (`http://www.w3.org/2001/04/xmlenc#sha256`,
  `http://www.w3.org/2001/04/xmldsig-more#sha384` or
  `http://www.w3.org/2001/04/xmlenc#sha512`)
- canonicalization: exclusive XML canonicalization 1.0 without comments,
  `http://www.w3.org/2001/10/xml-exc-c14n#`, optionally with an
  `InclusiveNamespaces` `PrefixList`
- transforms: exactly the enveloped-signature transform followed by
  exclusive canonicalization
- the key: one of the signing certificates from the provider's metadata.
  Zone ignores any `KeyInfo` in the response, so a response can't bring its
  own key. When the metadata lists several certificates, a signature by any
  of them is accepted. Zone doesn't check the certificates' validity dates,
  in line with the SAML metadata interoperability profile, so an expired
  certificate keeps working until you refresh the metadata.

SHA-1, ECDSA, DSA and HMAC signatures are refused, and so are inclusive
canonicalization and canonicalization with comments.

Zone's metadata says `WantAssertionsSigned="true"`, so identity providers
that configure themselves from SP metadata, such as Keycloak, turn on
assertion signing. Zone still accepts a response where only the response is
signed, because that is just as safe here: the assertion Zone reads is
inside the response it verified, and Zone finds it by its place in that
signed element, not by its ID, so a second, forged assertion elsewhere in
the document can't be swapped in. Responses with two assertions are refused
outright.

### Not supported

Zone doesn't support:

- IdP-initiated sign-in. A response that Zone didn't ask for, such as one
  from a tile on the identity provider's dashboard, is refused with
  `sso_state_invalid`. People sign in from Zone's sign-in page.
- encrypted assertions or encrypted NameIDs. Zone has no SP decryption key,
  so leave assertion encryption off at the identity provider.
- single logout. Signing out of Zone doesn't sign the person out of the
  identity provider, and the other way round.
- signed authentication requests. Zone's requests are unsigned, and its
  metadata says `AuthnRequestsSigned="false"`.
- the HTTP-POST binding for requests and the Artifact binding. Zone sends
  requests with HTTP-Redirect and accepts responses with HTTP-POST.
- SHA-1 signatures and digests, and keys other than RSA of at least 2048
  bits
- forced re-authentication (`ForceAuthn`) and passive sign-in (`IsPassive`)

Zone's authentication request asks for no particular NameID format: its
`NameIDPolicy` has `AllowCreate="true"` and no `Format`, so the identity
provider sends the format you configure there.

### Certificate rotation and refreshing metadata

Zone pins the signing certificates it found in the metadata when you saved
the provider. It doesn't fetch the metadata again by itself, so after your
identity provider gets a new signing certificate, refresh the metadata in
Zone:

- In the console, edit the provider, turn on **Read the metadata again**
  under its metadata URL, and save. Saving the same URL with the switch off
  sends nothing, so the certificates stay as they were. Or choose
  **Paste XML** and paste the new metadata. A blank XML box keeps the
  current metadata.
- Through the API, send `PATCH /api/sso/providers/{provider_id}` with
  `{"metadata_url": "…"}` or `{"metadata": "…"}`.

To rotate without failed sign-ins:

1. Add the new certificate at the identity provider without making it
   active yet, if the provider allows that, so that its metadata lists both
   certificates.
2. Refresh the metadata in Zone. Zone now accepts signatures by either
   certificate.
3. Make the new certificate active at the identity provider.
4. Once the old certificate is removed there, refresh the metadata in Zone
   again, so that Zone stops trusting it.

If your identity provider can only publish one certificate at a time,
refresh the metadata in Zone right after you activate the new one. Sign-ins
in between fail with `sso_assertion_invalid`.

A refresh that keeps the same entity ID keeps every account link, and it
works without a license, as described in [Licensing](#licensing).

Any save that changes the entity ID, the single sign-on URL or the pinned
certificates also ends every sign-in still in progress through that
provider, including one the ACS already accepted whose handoff code the
console hasn't redeemed yet: its response was checked against the old
settings. Those people get `sso_state_invalid` and start again.

### Changing the entity ID

The entity ID plays the role of the OIDC issuer: a NameID is only unique
within one identity provider. So when new metadata names a different entity
ID, Zone unlinks every account linked through that provider, in the same
save, and the `sso_provider.updated` audit entry records the number of links
removed as `identities_cleared`.

Because Zone never links a SAML sign-in by email, the people whose accounts
were unlinked can't get back to those accounts through SAML: their next
sign-in is refused with `sso_email_conflict`, and the only way forward is
to delete those Zone accounts so that SAML creates new ones. So avoid
changing a provider's entity ID: at the identity provider, keep using the
same app, rather than creating a new one, when you change its settings.
Without a license, a refresh that changes the entity ID is refused with
`license_required`.

### Set up Okta for SAML

These steps follow Okta's documentation for the Admin Console as of November
2026. Okta's own labels are in bold.

1. In the Okta Admin Console, go to **Applications and Resources →
   Applications** and choose **Create App Integration**. If Okta asks which
   experience to use, choose **Classic experience**.
2. Choose **SAML 2.0** as the sign-in method, then choose **Next**.
3. Give the integration a name, such as Zone, and choose **Next**.
4. Under the SAML settings:
   - **Single sign-on URL**: Zone's ACS URL,
     `{APP_BASE_URL}/api/auth/sso/saml/acs`. Keep **Use this for Recipient
     URL and Destination URL** selected, because Zone checks both.
   - **Audience URI (SP Entity ID)**: Zone's entity ID,
     `{APP_BASE_URL}/api/auth/sso/saml/metadata`.
   - **Default RelayState**: leave it blank. Zone sets the RelayState of
     every sign-in itself.
   - **Name ID format**: leave it at the default, Unspecified, or pick the
     email address format. Don't pick a transient format.
   - **Application username format**: a value that never changes for a
     person, because Okta sends the application username as the NameID and
     Zone links accounts by it. Okta also advises not to build it from
     fields that people can edit themselves.
   - Under **Show Advanced Settings**, keep **Assertion Signature** signed,
     use SHA-256 for **Signature Algorithm** and **Digest Algorithm**, and
     leave **Assertion Encryption** unencrypted.
5. Choose **Next**, choose the option for an internal app, and choose
   **Finish**.
6. On the app's **Sign On** tab, in the **Attribute Statements** section,
   choose **Add expression** three times and add these claims, each with a
   **Name** and an **Expression**:

   | Name | Expression |
   |---|---|
   | `email` | `user.profile.email` |
   | `firstName` | `user.profile.firstName` |
   | `lastName` | `user.profile.lastName` |

   If your org still configures attribute statements in the integration
   wizard, under **Attribute Statements (optional)**, add the same three
   names there instead.
7. On the **Assignments** tab, assign the people or groups who may use
   Zone.
8. On the **Sign On** tab, under the SAML 2.0 sign-on method, find
   **Metadata details** and copy the **Metadata URL**.

In Zone, add a SAML provider with that metadata URL and your email domains.
Okta's entity ID for the app looks like `http://www.okta.com/exk…`. That's
an identifier, not an address, so its `http` scheme is fine.

Okta signs with the certificate that's active on the app's **Sign On** tab,
under **SAML Signing Certificates**. When you generate or activate another
certificate there, refresh the metadata in Zone as described in
[Certificate rotation](#certificate-rotation-and-refreshing-metadata).

Okta always uses the **Single sign-on URL** for sign-ins started from the
Okta dashboard, which Zone refuses (see [Not supported](#not-supported)).
Tell people to sign in from Zone's sign-in page.

### Set up Entra ID for SAML

These steps follow Microsoft's documentation for the Microsoft Entra admin
center as of November 2026. Microsoft's own labels are in bold.

1. Sign in to the [Microsoft Entra admin center](https://entra.microsoft.com)
   as at least a Cloud Application Administrator and go to **Entra ID →
   Enterprise apps → All applications**.
2. Choose **New application**, then **Create your own application**. Enter
   a name, such as Zone, select **Integrate any other application you don't
   find in the gallery (Non-gallery)** and choose **Create**.
3. In the app, under **Manage**, select **Single sign-on**, then **SAML**.
4. In the **Basic SAML Configuration** section, choose **Edit** and set:
   - **Identifier (Entity ID)**: Zone's entity ID,
     `{APP_BASE_URL}/api/auth/sso/saml/metadata`
   - **Reply URL (Assertion Consumer Service URL)**: Zone's ACS URL,
     `{APP_BASE_URL}/api/auth/sso/saml/acs`

   Then choose **Save**.
5. In the **Attributes & Claims** section, choose **Edit**. Open the name
   identifier claim, set **Choose name identifier format** to
   **Persistent** and **Source attribute** to `user.objectid`, and save. A
   user's object ID never changes, unlike their user principal name, which
   Entra sends by default. Keep the default `emailaddress` claim, which
   carries the user's mail attribute.
6. In the **SAML Certificates** section, choose **Edit**. On the **SAML
   Signing Certificate** page, keep **Signing Option** at the default, **Sign
   SAML assertion**, and **Signing Algorithm** at **SHA-256**. Don't set up
   token encryption.
7. Still in the **SAML Certificates** section, copy the **App Federation
   Metadata Url**. Use this per-app URL, which ends in `appid=` and the
   application ID, rather than the tenant's general federation metadata,
   because only the per-app metadata carries the app's signing certificate.
8. Under **Manage → Properties**, set **Assignment required?** to **Yes**
   and choose **Save**, then assign the users or groups who may use Zone.
   Otherwise, anyone in the tenant can sign in.

In Zone, add a SAML provider with that metadata URL and your email domains.
Entra's entity ID is `https://sts.windows.net/{tenant-id}/`, with a trailing
slash.

A user with no mail attribute gets no `emailaddress` claim, and their
sign-in is refused with `sso_email_missing`.

Entra creates each SAML signing certificate for three years and emails the
app's notification addresses 60, 30 and 7 days before it expires. To renew
it, choose **New Certificate** on the **SAML Signing Certificate** page and
save, refresh the metadata in Zone, then make the new certificate active
there and refresh the metadata in Zone again. See
[Certificate rotation](#certificate-rotation-and-refreshing-metadata).

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

### How SAML sign-in works

1. The button and the origin rule are the same as for OIDC, and so are the
   10-minute lifetime and the limit of 10,000 unexpired attempts.
2. Zone sends the browser to the identity provider's single sign-on URL
   with an unsigned `AuthnRequest` over the HTTP-Redirect binding, and a
   `RelayState` that identifies the attempt.
3. The identity provider posts its `SAMLResponse` and the `RelayState` to
   Zone's ACS URL, `{APP_BASE_URL}/api/auth/sso/saml/acs`. The ACS reads at
   most 1 MiB of form body; real responses are 5 to 150 KB. It accepts a
   `RelayState` of at most 80 bytes, the limit the SAML HTTP-POST binding
   sets (Zone's own is 43 characters). A larger body, or a longer, missing
   or empty `RelayState`, gets a redirect to
   `{APP_BASE_URL}/sso/callback?error=sso_state_invalid` with no `state`,
   because Zone can't trust it to name an attempt.
4. The server runs the checks in [What Zone checks](#what-zone-checks) and
   applies the provider's policy (email present, in an allowed domain). It
   does the XML parsing and signature checks on a blocking worker thread,
   so a slow response never holds up other requests. Each attempt admits
   one post: the first post for a `RelayState` claims the attempt before its
   response is read, and any other post for it, even one arriving at the
   same moment, gets `sso_state_invalid` without its response being parsed.
   If a check fails, the claimed attempt is discarded. If the browser drops
   the connection mid-check, the attempt stays claimed and simply expires.
   The server checks at most one response per CPU core at a time, across
   every provider, because parsing holds many times the response's size in
   memory before its signature is known. A post that arrives while every
   check is running isn't queued: its attempt is discarded and the browser
   gets `sso_sign_in_failed`, and the person starts again.
5. On success, the server redirects the browser with `303 See Other` to
   `{APP_BASE_URL}/sso/callback?state=…&code=…`, with a one-time handoff
   code that expires after 2 minutes. On failure, it redirects to
   `{APP_BASE_URL}/sso/callback?state=…&error=…` with one of the codes in
   [Troubleshooting](#troubleshooting). Both redirects carry
   `Cache-Control: no-store` and `Referrer-Policy: no-referrer`.
6. The console hands the state and code to the Zone server, exactly as it
   does for OIDC. The server checks that the code belongs to that attempt,
   finds or creates the Zone account as described below, and signs the user
   in.

The handoff through the console keeps the same protection against login
cross-site request forgery that OIDC has: the sign-in only completes in the
browser that started it.

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

1. **By subject.** Zone remembers the provider's subject (`sub`, or the
   NameID for SAML) for each linked account. A returning user is matched
   on that alone, even if their email has changed at the provider. Zone
   doesn't change the account's email in that case.
2. **By email.** If no account is linked to that subject, and the provider
   has **Require verified email** on, Zone looks for an account with the
   same email, ignoring case. If it finds one, it links that account to the
   subject, and the user signs in to their existing account. This never
   happens for SAML providers, which can't require a verified email.
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

- uses the email from the ID token, marked as verified, or for SAML the
  email from the assertion, marked as not verified
- takes its display name from the provider's `name` claim when there is
  one, or for SAML from the attributes listed in
  [Identity and attributes](#identity-and-attributes)
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

Allowed domains are optional for OIDC providers and required for SAML
providers (`sso_domain_required`).

**Without allowed domains, every identity your provider lets through gets
a Zone account.** That's fine when the provider only admits your own staff,
for example through Okta assignments, Entra's **Assignment required?** or
Google's **Internal** user type. In any other case, set allowed domains.
For Google, always set them, so that `hd` is checked.

## Verified email

This setting exists only for OIDC providers. A SAML provider always behaves
as if it were off; see
[Allowed domains and verified email for SAML](#allowed-domains-and-verified-email-for-saml).

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
- Turning it on needs a Zone Enterprise license that is active or in its
  grace period. Turning it off doesn't.

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
| `sso_provider.updated` | A provider is edited | The changed settings, before and after, plus `client_secret_changed: true` when the secret was replaced, and `identities_cleared` (the number of accounts unlinked) when the issuer, client ID or SAML entity ID changed |
| `sso_provider.deleted` | A provider is deleted | The provider's settings |
| `sso_enforcement.updated` | Enforcement is turned on or off | `required`, before and after |
| `user.sso_linked` | An identity is linked to an existing account, whether or not the sign-in then gets a session | `provider_id`, `subject`, `reclaimed` |
| `user.sso_signed_in` | Someone signs in with SSO | `provider_id`, `subject`, `provisioned`, `linked`, `reclaimed` |
| `user.break_glass_signed_in` | A server admin signs in with a password while SSO is required | None |

For a SAML provider, the provider's settings in `sso_provider.*` entries
are `protocol`, `display_name`, `issuer` (the identity provider's entity
ID), `sso_url`, `metadata_url`, `certificate_fingerprints`,
`allowed_domains`, `require_verified_email` and `enabled`. SAML sign-ins write the same `user.sso_signed_in` entries as OIDC, with the
NameID as `subject`.

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
| `sso_provider_duplicate` | A provider with that issuer and client ID, or with that SAML entity ID, already exists. | Edit the existing provider instead. |
| `sso_provider_not_found` | The provider doesn't exist or is disabled. | The provider is enabled. |
| `sso_provider_required` | Enforcement is on, so the last enabled provider can't be disabled or deleted. | Turn enforcement off first, or enable another provider. |
| `sso_display_name_invalid` | The display name is empty or longer than 64 characters. | Use 1 to 64 characters. |
| `sso_client_id_invalid` | The client ID is empty or longer than 255 characters. | Copy the client ID from the provider again. |
| `sso_client_secret_invalid` | The client secret is empty. | Paste the secret's value. |
| `sso_domain_invalid` | An allowed domain isn't a plain domain name. | Use domains such as `acme.com`, without `@` or `*`. |
| `sso_state_invalid` | The sign-in attempt is unknown, was already used, or is more than 10 minutes old. For SAML, also: the response wasn't started from Zone (IdP-initiated), it was posted twice (including twice at once), the post was over 1 MiB or its `RelayState` over 80 bytes, the handoff code was wrong or more than 2 minutes old, or an admin changed the provider's entity ID, single sign-on URL or certificates before the console redeemed the code. | Start again from Zone's sign-in page, not from the identity provider's dashboard. |
| `sso_metadata_invalid` | Zone couldn't fetch or use the SAML metadata. The message names the problem. | The metadata URL uses `https` and doesn't redirect. The document is a single `EntityDescriptor` with an `IDPSSODescriptor`, an HTTP-Redirect `SingleSignOnService` and an RSA signing certificate of at least 2048 bits. See [Metadata](#metadata). Send either a metadata URL or metadata XML, not both. |
| `sso_assertion_invalid` | The SAML response failed a check: its signature, audience, recipient, destination, issuer, time limits, subject, or it was replayed or encrypted. The person sees a general message and the server log has the reason. | See [What Zone checks](#what-zone-checks). The identity provider uses Zone's exact entity ID and ACS URL, signs with SHA-256 and doesn't encrypt. The metadata in Zone is current after a certificate change. The Zone server's clock is correct. |
| `sso_sign_in_refused` | The identity provider answered with a status other than Success. The server log has the status. | The person is assigned to the app at the identity provider. |
| `sso_sign_in_failed` | Zone couldn't finish a SAML sign-in for a reason on the server's side, such as its database being unavailable, a stored provider setting it can't read, or every SAML response check (one per CPU core) already running. | The server log. Try again. |
| `sso_domain_required` | A SAML provider was saved without allowed domains. | Add at least one allowed domain. |
| `sso_verified_email_unsupported` | A SAML provider was saved with `require_verified_email: true`. | Leave it out or set it to `false`. See [Allowed domains and verified email for SAML](#allowed-domains-and-verified-email-for-saml). |
| `sso_protocol_mismatch` | A request set a field that belongs to the other protocol, such as a client secret on a SAML provider or a metadata URL on an OIDC provider. | Send only the fields for the provider's protocol. See [API](#api). |
| `sso_token_exchange_failed` | The provider's token endpoint refused the code, or its userinfo endpoint failed. The person sees a general message and the server log has the details. | The client secret is correct and hasn't expired. The redirect URI registered at the provider is exactly `{APP_BASE_URL}/sso/callback`. |
| `sso_id_token_invalid` | The ID token failed verification: signature, algorithm, issuer, audience, expiry or nonce, or the userinfo `sub` differed from it. | The Zone server's clock is correct. The client ID matches the provider's application. |
| `sso_email_missing` | Neither the ID token nor userinfo has an `email` claim. For SAML, the assertion has none of the email attributes, has more than one value in it, and its NameID isn't an email address NameID. | The provider puts `email` in the ID token. For Entra over OIDC, add the `email` optional claim. For SAML, see [Identity and attributes](#identity-and-attributes). |
| `sso_email_unverified` | The email isn't marked verified while Require verified email is on. | See [Verified email](#verified-email). |
| `sso_domain_not_allowed` | The email's domain, or the `hd` claim, isn't in the allowed domains. | The provider's allowed domains. |
| `sso_email_conflict` | The account can't be matched safely. | See [Accounts](#accounts-matching-linking-and-jit-provisioning). |
| `sso_required` | Password sign-in, registration or reset was refused because SSO is required. | Sign in with SSO, or as a server admin. |
| `account_disabled` | The Zone account is disabled. | Disabled accounts can't sign in by any method. |
| `license_required` | A change was refused because there's no Zone Enterprise license that is active or in its grace period. | **Settings → This server → License**. |
| `member_cap_reached` | A first sign-in would create an account, but the server runs Community and already has 3 active members. See [the member cap](LICENSING.md#the-community-member-cap). | A Zone Team or Enterprise license under **Settings → This server → License**. |

Zone accepts ID tokens signed with RS256, RS384, RS512, PS256, PS384,
PS512, ES256, ES384 or EdDSA. When the discovery document lists
`id_token_signing_alg_values_supported`, the token's algorithm must also be
in that list. Unsigned tokens (`none`) and tokens signed with a shared
secret (HS256 and the like) are always refused. For the SAML signature
rules, see [Which signatures Zone accepts](#which-signatures-zone-accepts).

A SAML sign-in that fails at the ACS URL reaches the browser as the `error`
parameter of the redirect to `{APP_BASE_URL}/sso/callback`, such as
`error=sso_assertion_invalid`, and the console shows the message for that
code. `sso_metadata_invalid`, `sso_domain_required`,
`sso_verified_email_unsupported` and `sso_protocol_mismatch` only ever
answer admin requests.

## What Zone needs from a provider

This section is about OIDC providers. For SAML, see
[What Zone needs from a SAML identity provider](#what-zone-needs-from-a-saml-identity-provider).

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
| `GET /api/sso` | Licensing state, enforcement, the redirect URI, the SAML service provider URLs and the providers. Never includes secrets. |
| `POST /api/sso/providers` | Add a provider. |
| `PATCH /api/sso/providers/{provider_id}` | Change a provider. Send only the fields you're changing. Leave out `client_secret` to keep the current one, and leave out `metadata_url` and `metadata` to keep the current SAML metadata. |
| `DELETE /api/sso/providers/{provider_id}` | Delete a provider. |
| `PUT /api/sso/enforcement` | Turn enforcement on or off with `{"required": true}` or `{"required": false}`. |

Every change is checked against the license before anything else, except
the ones listed under [Licensing](#licensing).

Two SAML endpoints are public, because the identity provider and the
browser reach them without a Zone session:

| Method and path | Purpose |
|---|---|
| `GET /api/auth/sso/saml/metadata` | Zone's SP metadata, as `application/samlmetadata+xml`. Its URL is also Zone's entity ID. |
| `POST /api/auth/sso/saml/acs` | The ACS. Takes a form of at most 1 MiB with `SAMLResponse` and a `RelayState` of at most 80 bytes, and answers `303` to `{APP_BASE_URL}/sso/callback` with the outcome. |

### Settings

`GET /api/sso` returns `entitled`, `edition`, `status`, `required`,
`redirect_uri`, `service_provider` and `providers`. `service_provider` holds
the [SAML service provider URLs](#the-saml-service-provider-urls):

```json
{
  "entity_id": "https://zone.example.com/api/auth/sso/saml/metadata",
  "acs_url": "https://zone.example.com/api/auth/sso/saml/acs",
  "metadata_url": "https://zone.example.com/api/auth/sso/saml/metadata"
}
```

### Providers

Each provider in `providers`, and the answer to a create or update, has
these fields. Every field is always present; the ones that don't apply to
the provider's protocol are `null` or empty.

| Field | OIDC | SAML |
|---|---|---|
| `id` | The provider's ID. | The provider's ID. |
| `protocol` | `"oidc"` | `"saml"` |
| `display_name` | The button label. | The button label. |
| `issuer` | The issuer URL. | The identity provider's entity ID. |
| `client_id` | The client ID. | `null` |
| `sso_url` | `null` | The identity provider's HTTP-Redirect single sign-on URL. |
| `metadata_url` | `null` | The metadata URL, or `null` when the metadata was pasted. |
| `certificate_fingerprints` | `[]` | The SHA-256 fingerprint of each signing certificate, in uppercase hex separated by colons, such as `3F:A2:…:9C`. |
| `allowed_domains` | The allowed domains. | The allowed domains. |
| `require_verified_email` | Whether a verified email is required. | Always `false`. |
| `enabled` | Whether the provider is enabled. | Whether the provider is enabled. |
| `created_at`, `updated_at` | Timestamps. | Timestamps. |

`POST /api/sso/providers` takes:

| Field | OIDC | SAML |
|---|---|---|
| `protocol` | `"oidc"`, the default when it's left out. | `"saml"` |
| `display_name` | Required. | Required. |
| `issuer`, `client_id`, `client_secret` | Required. | Must be left out or empty, or the request is refused with `sso_protocol_mismatch`. |
| `metadata_url` | Must be left out (`sso_protocol_mismatch`). | Exactly one of `metadata_url` and `metadata` is required; otherwise the request is refused with `sso_metadata_invalid`. |
| `metadata` | Must be left out (`sso_protocol_mismatch`). | The metadata XML, as a string. |
| `allowed_domains` | Optional. | At least one is required (`sso_domain_required`). |
| `require_verified_email` | Optional, `true` when it's left out. | Optional, `false` when it's left out. `true` is refused with `sso_verified_email_unsupported`. |
| `enabled` | Optional, `true` when it's left out. | Optional, `true` when it's left out. |

For example, to add a SAML provider from a metadata URL:

```json
{
  "protocol": "saml",
  "display_name": "Okta",
  "metadata_url": "https://acme.okta.com/app/exk…/sso/saml/metadata",
  "allowed_domains": ["acme.com"]
}
```

`PATCH /api/sso/providers/{provider_id}` takes any of `display_name`,
`issuer`, `client_id`, `client_secret`, `metadata_url`, `metadata`,
`allowed_domains`, `require_verified_email` and `enabled`. On an OIDC
provider, `metadata_url` and `metadata` are refused with
`sso_protocol_mismatch`. On a SAML provider, `issuer`, `client_id` and
`client_secret` are refused the same way, and `metadata_url` or `metadata`,
one of the two, fetches or reads the metadata again and replaces the entity
ID, single sign-on URL and certificates with what it holds. The protocol
can't be changed.
