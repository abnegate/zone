# SCIM provisioning

Zone accepts SCIM 2.0 ([RFC 7643](https://www.rfc-editor.org/rfc/rfc7643)
and [RFC 7644](https://www.rfc-editor.org/rfc/rfc7644)) from your identity
provider, so that the provider creates, updates, deactivates and deletes an
organization's members, and pushes groups that you map to organization
roles. This page covers Okta, Microsoft Entra ID and Google Workspace.

SCIM is set up per organization, not per server. Each organization has its
own SCIM base URL and its own tokens, and a token only ever reaches the
organization it was issued for. An organization owner sets it up under
**Settings → Organization → SCIM provisioning**.

SCIM manages membership. People still sign in with a password or through
[single sign-on](SSO.md), which a server admin sets up for the whole server.
Most organizations use both: SSO for sign-in, SCIM to add and remove members.

## Licensing

SCIM provisioning is part of Zone Enterprise; a Zone Team license doesn't
include it. It needs a Zone Enterprise license that verifies and is either
active or in its 30-day grace period after expiry. You install the license
under **Settings → This server → License**.

| License state | SCIM requests | Console |
|---|---|---|
| Enterprise, active or in grace | Everything | Editable |
| Enterprise, lapsed (past grace) | Reads and deprovisioning only | Read-only, with a renewal notice; tokens can still be revoked |
| Team, in any state | Reads and deprovisioning only | An upgrade notice; tokens, groups and sync history already there are shown read-only, and tokens can still be revoked |
| Removed, never installed (Community), or a stored key that doesn't verify | Reads and deprovisioning only | An upgrade notice; tokens, groups and sync history already there are shown read-only, and tokens can still be revoked |

Without the license, an identity provider that already has a token keeps
reaching Zone, but Zone accepts only the requests that take access away:

- `DELETE /Users/{user_id}`;
- `PUT` or `PATCH /Users/{user_id}` that sets `active` to `false`. Zone
  deactivates the member and ignores every other attribute in the request.

Every other write is refused with status 403 and the detail "This needs a
Zone Enterprise license that is active or in its grace period.", in the
[SCIM error format](#errors). That includes creating users, reactivating
them, changing their attributes, and every group write. `GET` requests keep
working.

So if your license lapses, people your identity provider deactivates still
lose access. Nobody is locked out by the lapse itself: members keep their
memberships and roles, and group mappings stay as they are.

In the console, issuing and rotating tokens and changing group mappings need
the license and are refused with `license_required` without it. Revoking a
token never needs a license, so you can always cut off a provider.

## Who can set it up

Setting up SCIM needs the `scim:update` permission, which only organization
owners hold (see [ROLES.md](ROLES.md#organization-permissions)). It can't go
in a custom role. A SCIM token can deactivate admins and map groups to the
admin role, so issuing one is an owner's decision. Admins don't see the
section, and the API answers them with 403.

## Base URL and tokens

The SCIM base URL of an organization is:

```
{APP_BASE_URL}/api/scim/v2/organizations/{org_id}
```

As for the SSO redirect URI, Zone takes only the scheme, host and port of
[`APP_BASE_URL`](CONFIGURATION.md#app_base_url): with
`APP_BASE_URL=https://zone.example.com/zone`, the base URL is
`https://zone.example.com/api/scim/v2/organizations/{org_id}`. The SCIM
provisioning section shows the exact value with a copy button. If you change
`APP_BASE_URL`, update the base URL at your identity provider.

Your identity provider must be able to reach this URL over HTTPS. Okta,
Entra ID and Google call it from their own clouds, so the Zone server has to
be reachable from the internet, or at least from their address ranges.

### Tokens

A token is a bearer token that starts with `zscim_`. The identity provider
sends it on every request:

```http
Authorization: Bearer zscim_…
```

- **Create** a token in the SCIM provisioning section and give it a name,
  such as the provider's. Zone shows the secret once. Copy it straight into
  your identity provider; Zone stores only a hash of it and can't show it
  again. The list shows each token's last four characters, when it was
  created, rotated and last used.
- **Rotate** a token to replace its secret. The old secret stops working at
  once, so paste the new one into your identity provider straight away.
  For a change without any failed requests, create a second token, switch
  the provider to it, and then revoke the first.
- **Revoke** a token to stop it working at once. Revoking never needs a
  license.

An organization can have several tokens, for example one per identity
provider. All of them act on the same members and groups, and share one
[rate limit](#rate-limit).

Zone answers 401 with `WWW-Authenticate: Bearer` when the `Authorization`
header is missing or isn't a bearer token, when the token is wrong, revoked
or rotated out, when it belongs to another organization, and when the
organization no longer exists or is deactivated. An organization ID in the
URL that isn't an ID at all answers 404.

## Okta

Okta adds SCIM provisioning to an app integration you already have. If you
set up single sign-on with Okta as [SSO.md](SSO.md#okta) describes, use that
integration. Okta supports SCIM on SAML integrations and on OIDC
integrations created with the App Integration Wizard. If the SCIM option
doesn't appear, provisioning isn't turned on for your Okta org; ask Okta
support.

Make sure your Okta usernames are email addresses, which is Okta's default.
Zone takes a new member's email from their primary email, or from their
username when it's an address.

### Turn on provisioning

1. In the Okta Admin Console, open the Zone app integration and choose the
   **General** tab.
2. Under **App Settings**, choose **Edit**, set **Provisioning** to
   **SCIM**, and choose **Save**.

### Connect Okta to Zone

1. On the **Provisioning** tab, under **Settings → Integration**, choose
   **Edit**.
2. Set **SCIM connector base URL** to the organization's SCIM base URL.
3. Set **Unique identifier field for users** to `userName`.
4. Under **Supported provisioning actions**, choose **Import New Users and
   Profile Updates**, **Push New Users**, **Push Profile Updates** and
   **Push Groups**.
5. Set **Authentication Mode** to **HTTP Header**, and paste a Zone SCIM
   token into **Authorization**.
6. Choose **Test Connector Configuration**, then **Save**.
7. Under **Settings → To App**, choose **Edit** and turn on **Create
   Users**, **Update User Attributes** and **Deactivate Users**. Leave
   **Sync Password** off: Zone doesn't take passwords over SCIM. Choose
   **Save**.

### Assign people and push groups

1. On the **Assignments** tab, assign the people or groups who should be
   members of the Zone organization. Okta creates each of them in Zone.
2. On the **Push Groups** tab, choose **Push Groups**, find the groups you
   want to map to Zone roles by name, and save. Okta creates them in Zone
   and keeps their members in step. A pushed group's members must also be
   assigned to the app.
3. In Zone, [map the pushed groups](#group-to-role-mapping) to roles.

To bring members who are already in the Zone organization under Okta, run
an import from the **Import** tab. Zone lists every member of the
organization, and a member who isn't linked to SCIM yet has their account
email as `userName`, so Okta can match them by username.

### What Okta sends

- Okta never deletes users. Unassigning someone, or deactivating them in
  Okta, sends `active: false`, which [deactivates](#deactivation) the
  member. Reassigning or reactivating them sends `active: true`, which
  restores their access.
- App integrations made with the App Integration Wizard send profile
  updates as `PUT`, so Okta sends the whole user each time.
- Okta deletes a pushed group in Zone only when you unlink it and choose to
  delete it in the app.

## Microsoft Entra ID

Entra ID connects to Zone as a non-gallery enterprise application. You can
use the same enterprise application as for SSO, or a separate one.

1. Sign in to the [Microsoft Entra admin center](https://entra.microsoft.com)
   as at least an Application Administrator, and go to **Entra ID →
   Enterprise apps**.
2. Choose **New application → Create your own application**, enter a name
   such as Zone, choose **Integrate any other application you don't find in
   the gallery**, and create it.
3. In the app, choose **Provisioning**, then **New configuration**.
4. Set **Tenant URL** to the organization's SCIM base URL followed by
   `?aadOptscim062020`, for example
   `https://zone.example.com/api/scim/v2/organizations/{org_id}?aadOptscim062020`.
5. Put a Zone SCIM token in **Secret Token**. If Entra asks for an
   authentication method, choose bearer token authentication. Don't leave
   the field blank: Entra then sends its own token, which Zone refuses.
6. Choose **Test Connection**, then **Create**.
7. Under **Users and groups**, assign the people and groups who should be
   members, and keep the scope at **Sync only assigned users and groups**.
8. Review **Attribute mapping** (see below), then **Start provisioning**.

Entra runs a provisioning cycle about every 40 minutes, so changes in Entra
reach Zone with that delay. To try a user straight away, use **Provision
on-demand**.

### Why `?aadOptscim062020`

`aadOptscim062020` is a Microsoft flag that makes Entra's `PATCH` requests
follow RFC 7644: `active` is sent as the boolean `false` rather than the
string `"False"`, and removing a group member uses a
`members[value eq "…"]` path. Zone accepts Entra's requests with or without
the flag, but use it: it's the format Microsoft recommends for new
applications, and the one Zone is tested against first. Microsoft notes
that the flag isn't applied to **Provision on-demand**.

### Attribute mapping

Keep Entra's default user mappings, with two checks:

- The **Matching** attribute must be `userName`, which Entra maps from
  `userPrincipalName`. If your users' principal names aren't their email
  addresses, map `userName` from `mail` instead. Don't make an email
  attribute the matching attribute: Zone can't filter on `emails`, so
  Entra's lookups would fail.
- `emails[type eq "work"].value` maps from `mail`. Zone uses it as a new
  member's email.

Zone ignores attributes it doesn't store, such as the job title, phone
numbers, addresses and the enterprise extension, so you don't need to
remove them.

Keep group provisioning (**Provision Microsoft Entra ID Groups**) on if you
want to map groups to roles. Group names must be unique within the
organization, which Entra requires anyway.

### What Entra sends

- Disabling a user, unassigning them, or deleting them (soft delete) sends
  `active: false`, which [deactivates](#deactivation) the member.
- Entra deletes users permanently 30 days after a soft delete, or when you
  delete them permanently yourself. Then it sends `DELETE`, which
  [removes](#deletion) the member.
- Entra's lookups sometimes send unquoted values, such as
  `filter=externalId eq jyoung`. Zone accepts them.

## Google Workspace

Google Workspace can provision users to a custom app only through
**automated user provisioning with custom SCIM profiles**, which Google
describes as a beta available to a limited number of participants. If the
**Autoprovisioning** section doesn't appear on your app's page in the Google
Admin console, your organization doesn't have it.

What Google's custom SCIM profiles do, as Google documents them:

- They hang off a SAML app in the Admin console, which must be set up and
  turned on for users. Setting it up needs a super administrator.
- Google authenticates with a bearer token, which Zone's tokens are.
- They provision users only. Groups limit which users are provisioned, but
  Google doesn't push groups to the app, so you can't
  [map groups to roles](#group-to-role-mapping) from Google. Give
  provisioned members their roles in Zone's console instead.
- When a user's access to the app is turned off, or the user is suspended or
  deleted in Google, Google can suspend them in the app (`active: false`,
  which [deactivates](#deactivation) them in Zone), delete them (`DELETE`,
  which [removes](#deletion) them), or both, after a delay you choose.

### If you have the beta

1. In the Google Admin console, go to **Apps → Web and mobile apps** and
   add a custom SAML app for Zone, with the
   [SAML service provider values](SSO.md#the-saml-service-provider-urls)
   from Zone. Turn it on for the people who should be members. Zone's SSO
   documentation doesn't cover Google as a SAML identity provider; sign in
   with Google over [OIDC](SSO.md#google-workspace) as before.
2. Open the app and, under **Autoprovisioning**, choose **Configure
   auto-provisioning**.
3. Paste a Zone SCIM token into **Access token**, and the organization's
   SCIM base URL into **Endpoint URL**.
4. In the attribute mapping, map `userName` and the primary work email to
   the user's primary email.
5. Optionally choose groups to limit who is provisioned, and choose the
   deprovisioning actions. Choosing **Suspend user's account** keeps the
   member's workspace memberships so they come back on reactivation;
   **Hard delete user's account** removes them.
6. Test with a Google user who isn't a member of the Zone organization yet.
   Google creates, updates and then deletes that user; in Zone the test
   account stays, without a membership.
7. Choose **Finish**, then set **Autoprovisioning** to **Active**.

### Without the beta

- If your directory also lives in Okta or Entra ID, provision Zone from
  there as described above.
- Otherwise, use Google for [single sign-on](SSO.md#google-workspace) and
  manage membership in Zone: invite people from **Settings → Organization →
  Members**, and remove them there when they leave. Removing someone in the
  console stops their access to the organization but doesn't revoke their
  sessions; see [Removing members](ROLES.md#removing-members).

## Users

A SCIM user is a member of the organization. Its `id` is the Zone account's
ID.

Zone stores these attributes: `userName`, `externalId`, `name.givenName`,
`name.familyName`, `displayName`, the primary work email from `emails`, and
`active`. It accepts `name.formatted` but doesn't store it, and ignores
every other attribute, including the enterprise extension and `password`.
`groups` is read-only and lists the SCIM groups the member is in.

`GET /Users` lists every member of the organization, active or not,
including members who were added in the console. A member who isn't linked
to SCIM yet shows their account email as `userName`.

### Creating a user

`POST /Users` adds someone to the organization as a member. Zone takes
their email from the primary email in `emails`, or from `userName` when it's
an email address; with neither, it answers 400 `invalidValue`. It then looks
for an account with that email, ignoring case:

- **A current member** of the organization: 409 `uniqueness`. Okta and
  Entra look users up by `userName` before creating them, so they match an
  existing member instead, and Zone links the member to SCIM at the first
  update that changes one of their attributes. An update that only sets
  `active` deactivates or reactivates the member without linking them.
- **An account that isn't a member**, or a former member: Zone adds the
  account to the organization as a member and links it to SCIM. A former
  member starts afresh: their old workspace memberships and roles don't come
  back. The account's email, name, password and sessions don't change, and
  this organization never signs the account out (see
  [Deactivation](#deactivation)).
- **No account**: Zone creates one, with an unverified email and no
  password anyone knows. It doesn't get a personal organization or
  workspace. The person signs in through [SSO](#single-sign-on), or sets a
  password through password reset if the server sends account mail.

`userName` and `externalId` are unique within the organization, `userName`
ignoring case; a clash answers 409 `uniqueness`.

A `POST` with `active: false` creates the member already deactivated, in
one step, and answers 201 with the deactivated user. It doesn't count
against the member cap, and it deletes the account's invitations, and signs
the account out only when Zone creates it, as [deactivation](#deactivation)
does. A `POST` that matches an
owner whom someone restores while it runs answers 409 `uniqueness` and
leaves the owner as they are.

New members get the built-in member role, unless a
[group mapping](#group-to-role-mapping) gives them another. Add them to
workspaces in the console, or invite them into workspaces with an
invitation.

### Updating a user

`PUT` and `PATCH` change the attributes Zone stores for the link. A `PUT`
replaces them: an attribute it leaves out is cleared, except `active`, which
changes nothing when it's left out.

The account's own email and display name, which the member sees everywhere
in Zone, change only while all of these hold:

- SCIM created the account, and nobody has claimed it yet: it still has the
  password SCIM gave it, it has never signed in, and it isn't linked to any
  SSO identity (see [Single sign-on](#single-sign-on));
- it isn't a server admin;
- it has no membership in any other organization, active or not. A member
  removed in another organization's console still counts; one that
  organization's identity provider deleted doesn't.

Otherwise one organization's identity provider could take over an account
its owner already uses, or rename one that other organizations share. The
SCIM user still shows the new values. A changed email is unverified until
the person verifies it again. An email that another account already has
answers 409 `uniqueness`.

`active: false` [deactivates](#deactivation) the member, and `active: true`
[reactivates](#reactivation) them.

## Group to role mapping

Groups that Okta or Entra push to Zone carry no access on their own. To
make a group grant a role, open **Settings → Organization → SCIM
provisioning**, find the group, and choose a role for it:

- **Unmapped**: the group grants nothing. This is where every pushed group
  starts.
- **Admin** or **Member**: the built-in organization role.
- An organization-scope [custom role](ROLES.md#custom-roles) of the
  organization.

No group can map to the owner role, and you can only map a role whose
permissions you hold in full.

### Which role a member gets

Once any group in the organization is mapped, SCIM decides the organization
role of every member it manages, other than owners. SCIM manages a member
once the identity provider has created or updated them:

1. A member in one or more mapped groups gets the role of one of them,
   chosen in this order:
   1. the role that holds more permissions;
   2. on a tie, a built-in role before a custom role;
   3. then the role whose name comes first, ignoring case;
   4. then the lower role ID.

   So Admin beats any custom organization role, because no custom role can
   hold `roles:update`, and a custom role that holds more than Member beats
   Member.
2. A member in no mapped group gets Member.

Zone works this out for a member whenever SCIM changes something that
affects them:

- they're created, or added to or removed from a group;
- a group they're in is mapped, remapped or unmapped, or a mapped group
  they're in is deleted;
- the custom role a group they're in maps to is deleted;
- they're reactivated.

A group change and the roles it settles are saved together, and every
member a request names is worked out again, even one already in or out of
the group, so a retried request settles roles a failed one left behind.

Some consequences:

- **Unmapping the last mapping** drops its members to Member. So does
  deleting the last mapped group.
- **A deactivated member** is worked out too, whenever a group they're in
  changes: they come back with the role their groups give them when they're
  reactivated, not one a mapping stopped granting while they were away.
- **Editing a mapped custom role's permissions** applies at once, to
  everyone who holds the role. Which group's role wins is settled again on
  each member's next SCIM change.
- **Deleting a mapped custom role**, which Zone allows only once nobody
  holds it, leaves its groups unmapped. Each unmapping is audited as
  `scim_group.unmapped` by whoever deleted the role, and the groups' members
  are worked out again.
- **A role set in the console** for a member SCIM governs lasts until their
  next SCIM change, which applies the mapping again.
- **Owners** are never changed by SCIM, and SCIM never makes anyone an
  owner.
- **Members SCIM doesn't manage**, such as people added in the console whom
  the identity provider has never created or updated, keep their roles.

Group mappings set organization roles only. Workspace memberships and
workspace roles are managed in each workspace.

## Deactivation

`active: false` deactivates a member. Zone:

- suspends their organization membership and every membership they have in
  the organization's workspaces, recording each role and custom role so
  that [reactivation](#reactivation) restores them;
- makes another member owner of any workspace where the deactivated member
  was the only owner, by the rule in
  [Removing members](ROLES.md#removing-members), and records it as
  `workspace.owner_seated` with no actor. If nobody is left to seat, the
  workspace has no owner until an organization owner adds one, and the
  deactivation still succeeds;
- signs the account out on the whole server, revoking every session and
  refresh token, only when both hold:
  - this organization's SCIM created the account, with
    [`POST /Users`](#creating-a-user) finding no account with that email.
    The person claiming it later, by signing in or setting a password,
    doesn't change that;
  - the account isn't an active member of another organization that has at
    least one other active member. A personal organization the person is
    alone in doesn't count.

  Sessions belong to the account rather than to an organization, so an
  account that existed before this organization added it, such as one its
  owner registered or another organization's member, is never signed out by
  this organization's identity provider. When the account is signed out,
  requests are refused at once and open connections close within about 15
  seconds; the person can still sign in and use any other organization they
  belong to. When it isn't, the deactivated membership is what keeps them
  out of this organization: its organization and workspace routes refuse
  them, and their running work stops as below;
- deletes any invitation to the organization still waiting on their
  account email or the email the identity provider gave them, ignoring
  case, and audits each as `invitation.revoked`;
- after that, stops their running chat turns in the organization's
  workspaces, reminder and auto project turns included, and the task runs
  they started there.

The membership change, the sign-out and the invitation deletions happen
together: if any of them fails, none of them does, and the identity
provider's retry starts again. Deactivating a member who is already
inactive signs them out again, by the same rule, and deletes their
invitations again.

The Zone account itself is kept.

Deactivating the organization's last active owner is refused with 409.

## Reactivation

`active: true` on a deactivated member brings them back:

- their organization role comes back, with their custom role if it still
  exists as an organization-scope role of the organization; otherwise they
  get the built-in role they had, or Member if they had a custom role;
- if the organization has mapped groups, the
  [mapping](#which-role-a-member-gets) then decides their role, as for any
  other SCIM change;
- every workspace membership that was suspended comes back with its role
  and custom role, in workspaces that still exist. A workspace custom role
  that was deleted meanwhile, or no longer has workspace scope, comes back
  as viewer. A member who was replaced as a workspace's owner comes back as
  owner too, so the workspace may then have two owners.

Their sessions don't come back; they sign in again.

Reactivation needs the license. Without it, a request that sets
`active: true` is refused with 403.

## Deletion

`DELETE /Users/{user_id}` removes the member from the organization. Zone:

- deletes their organization membership and their memberships in the
  organization's workspaces, seating workspace owners as for deactivation;
- deletes their SCIM link and their membership of the organization's SCIM
  groups;
- signs the account out, deletes their pending invitations and stops their
  work, as for deactivation, by the same rules and in the same single
  step.

The Zone account is kept, along with its memberships of other
organizations. Nothing is kept to restore: if the identity provider creates
the person again, they start afresh as a new member. Their account then
already exists, so Zone adds it as an existing account, and deactivating
them later doesn't sign them out.

Deleting the organization's last active owner is refused with 409.

## Removing members in the console

Removing a member under **Settings → Organization → Members** also suspends
their workspace memberships, seats workspace owners, stops their running
work and deletes their pending invitations, auditing each as
`invitation.revoked` with the remover as the actor, but doesn't revoke their
sessions. The invitations go in the same step as the membership. Adding
them back in the console, or their accepting a new invitation, starts them
afresh, without their old workspace memberships.
Before SCIM, a removed member kept their workspace access; that's no longer
the case. See [Removing members](ROLES.md#removing-members).

If an identity provider still lists someone you removed in the console, it
may add them again on its next sync. Remove people at the identity provider
instead.

## Single sign-on

An account that SCIM created links to the person's first SSO sign-in by
email, over OIDC and SAML alike, whatever the provider's **Require verified
email** setting. That's what lets a SCIM-created account sign in over SAML,
which otherwise never links by email (see
[Accounts](SSO.md#accounts-matching-linking-and-jit-provisioning)).

This applies only while the account is untouched: it still has the password
SCIM gave it, which nobody knows, it has never signed in, and it isn't
linked to any SSO identity. Once the person resets their password or signs
in, the usual SSO rules apply.

Accounts that existed before SCIM added them to the organization follow
the usual SSO rules from the start.

## Rate limit

Each organization can make 600 SCIM requests a minute to each server
replica, across all its tokens. Zone counts a request against the
organization only after its token is accepted, so requests with a wrong
token don't use up the budget.

Zone also limits each client address to 1,200 rejected tokens a minute per
replica, whatever organization they name, so a flood of guessed tokens
can't keep the database busy. Only a token Zone looks up and rejects counts;
accepted requests and requests without a bearer token never do. Once an
address is over that limit, Zone refuses its requests for the rest of the
minute without looking their tokens up, except a request whose token is
one of the active SCIM tokens: Zone keeps their hashes in memory, reloading
them at most every 5 seconds while an address is refused, so an identity
provider sharing an address with a flood, such as everyone behind one
ingress, still gets through. The address is the connecting peer, or behind
a proxy listed in `ZONE_TRUSTED_PROXIES` the client that proxy saw. IPv6
clients are counted by their /64.

Over either limit, Zone answers 429 with `scimType` `tooMany` and a
`Retry-After` header giving the seconds to wait. Okta and Entra slow down
and retry.

## Requests

Every route below is relative to the organization's base URL, and every
route, discovery included, needs a token.

| Method and path | Purpose |
|---|---|
| `GET /ServiceProviderConfig` | What Zone supports. |
| `GET /ResourceTypes`, `GET /ResourceTypes/{name}` | The `User` and `Group` resource types. |
| `GET /Schemas`, `GET /Schemas/{urn}` | The user and group schemas, with only the attributes Zone stores. |
| `GET /Users`, `POST /Users` | List members, or add one. |
| `GET`, `PUT`, `PATCH`, `DELETE /Users/{user_id}` | Read, update, deactivate or reactivate, or remove a member. |
| `GET /Groups`, `POST /Groups` | List groups, or create one. |
| `GET`, `PUT`, `PATCH`, `DELETE /Groups/{group_id}` | Read, update, change the members of, or delete a group. |

Requests and responses use `Content-Type: application/scim+json`, errors
included. `POST` answers 201 with a `Location` header, `GET`, `PUT` and
`PATCH` answer 200 with the resource, and `DELETE` answers 204. An ID that
doesn't exist, or belongs to another organization, answers 404.

`ServiceProviderConfig` says that Zone supports `PATCH` and filtering, with
at most 1000 results a page, and doesn't support bulk operations, sorting,
ETags or password changes. The only authentication scheme is the bearer
token.

### Filters

Zone supports the `eq` operator, and `and` to join comparisons. Operators
and attribute names are case-insensitive, and attribute names may carry the
core schema URN, such as `urn:ietf:params:scim:schemas:core:2.0:User:userName`.

| Resource | Attributes |
|---|---|
| Users | `id`, `userName` (ignoring case), `externalId`, `displayName` |
| Groups | `id`, `displayName`, `externalId`, and membership as `members eq "{user_id}"`, `members.value eq "{user_id}"` or `members[value eq "{user_id}"]` |

Values may be quoted JSON strings, with any JSON escape, or bare words. An
unquoted `null` is refused with `invalidFilter`; quote it to compare with
the text `null`. For example:

```
GET /Users?filter=userName eq "jane@acme.com"
GET /Groups?filter=id eq "{group_id}" and members eq "{user_id}"&attributes=id
```

Zone refuses with 400 `invalidFilter`: `or`, `not`, parentheses, any other
operator (`ne`, `co`, `sw`, `ew`, `gt`, `ge`, `lt`, `le`, `pr`), attributes
not in the table, such as `emails`, and anything it can't read. A filter
that names an attribute of the other resource, such as `members` on users,
matches nothing.

### Paging

Lists are paged with `startIndex`, which starts at 1, and `count`. A
`startIndex` below 1 counts as 1. `count` defaults to 100, and anything
above 1000 counts as 1000; `count=0` returns only `totalResults`. Users are
listed in the order they joined the organization.

`excludedAttributes=members`, or an `attributes` list without `members`,
leaves group members out, which keeps lists of large groups small.

### PATCH

`PATCH` takes a `PatchOp` with `add`, `replace` and `remove` operations, in
any case (Entra sends `Replace`). Operations apply in order. Zone accepts
the forms Okta and Entra send:

- a `path`, such as `active`, `userName`, `externalId`, `displayName`,
  `name.givenName`, `name.familyName`, `emails`,
  `emails[type eq "work"].value`, `emails[primary eq true].value`, or
  `members`;
- no `path`, with an object `value`, such as Okta's `{"active": false}` or
  Entra's `{"displayName": "…", "name.givenName": "…"}`. Keys may be dotted,
  nested (`{"name": {"givenName": "…"}}`) or carry the core schema URN;
- `active` as a boolean, or as the string `"true"` or `"false"` in any case,
  which Entra sends without `?aadOptscim062020`.

For group members:

- `add` with `path` `members` adds the members in `value`;
- `replace` with `path` `members` replaces all members;
- `remove` with `path` `members[value eq "{user_id}"]` removes one member,
  and `remove` with `path` `members` and a `value` list removes those;
- `remove` with `path` `members` and no `value` removes everyone.

A member value that `add`, `replace`, `POST` or `PUT` sets must be the `id`
of a member of this organization, active or not; anything else, including a
value that isn't an ID, answers 400 `invalidValue` and changes nothing.
`remove` is lenient: removing someone who isn't in the group, or a value
that isn't an ID, does nothing.

Zone accepts and ignores paths for attributes it doesn't store, including
the enterprise extension. It refuses with 400: a path it can't read,
including an empty path, an empty sub-attribute such as `name.`, and one
nested deeper than a sub-attribute such as `name.givenName.x`
(`invalidPath`); `remove` without a path (`noTarget`); and removing a
required attribute, `userName`, `active` or `emails` on a user or
`displayName` on a group (`mutability`).

Attribute names in a path-less `value` are case-insensitive; when one
object names an attribute twice in different case, the later one wins.
`replace` of `name` with `null` clears the given and family names. An
email entry whose `value` is `null` is skipped.

`PUT` on a group sets its `displayName` and `externalId`, and replaces its
members only when the request includes `members`.

### Groups

Group `displayName` is required and unique within the organization,
ignoring case and surrounding spaces, and `externalId` is unique when it's
set; a clash answers 409 `uniqueness`. Deleting a group removes it and its
memberships; if it was mapped, its former members' roles are worked out
again.

## Errors

Errors use the SCIM error format:

```json
{
  "schemas": ["urn:ietf:params:scim:api:messages:2.0:Error"],
  "status": "409",
  "scimType": "uniqueness",
  "detail": "…"
}
```

`status` is a string, `scimType` is left out when it doesn't apply, and
`detail` says what went wrong in plain words.

| Status | `scimType` | When |
|---|---|---|
| 400 | `invalidSyntax` | The body isn't valid JSON, or doesn't have the shape of the resource. |
| 400 | `invalidFilter` | See [Filters](#filters). |
| 400 | `invalidValue` | A required value is missing or blank, a user has no email address, a value has the wrong type, or a member value that `add`, `replace`, `POST` or `PUT` sets isn't a member of this organization. |
| 400 | `invalidPath` | A `PATCH` path can't be read, or uses a members filter outside `remove`. |
| 400 | `noTarget` | A `remove` operation has no path. |
| 400 | `mutability` | A `remove` operation names a required attribute. |
| 401 | | No token, a wrong, revoked or rotated-out token, another organization's token, or an organization that no longer exists or is deactivated. |
| 403 | | The [license](#licensing) doesn't allow the write. |
| 404 | | The organization ID in the URL isn't an ID, the user or group doesn't exist or belongs to another organization, or the path isn't a SCIM route. |
| 405 | | The route doesn't take that method. |
| 409 | `uniqueness` | `userName`, `externalId` or a group's `displayName` is taken, the user is already a member, or the new email belongs to another account. |
| 409 | | The request would deactivate or delete the organization's last active owner. |
| 413 | | The request body is larger than Zone accepts. |
| 429 | `tooMany` | The [rate limit](#rate-limit) is used up. |
| 503 | | A user create or reactivation that the [member cap](LICENSING.md#the-community-member-cap) applies to waited more than about 5 seconds for its turn. Nothing changed; the provider retries. |

## Troubleshooting

The SCIM provisioning section shows when the organization's tokens were
last used, and the 20 most recent failed requests: each one's method, route
(such as `/api/scim/v2/organizations/{org_id}/Users/{user_id}`), status,
`scimType`, detail and time. Zone records every failed request whose token
it accepted, and keeps them for 30 days. Of the requests the organization's
[rate limit](#rate-limit) refuses, it records the first in each minute. A request with a wrong or missing token isn't
recorded, because Zone can't tell who sent it; look at the
identity provider's logs instead (Okta's **View Logs**, Entra's
**Provisioning logs**).

| Symptom | What to check |
|---|---|
| The provider's connection test fails with 401 | The token was pasted whole, including `zscim_`, and hasn't been revoked or rotated. The URL is this organization's base URL. For Entra, **Secret Token** isn't blank. |
| The connection test fails with 404 | The base URL ends in a whole organization ID, with no extra path after it other than Entra's `?aadOptscim062020`. An ID of another organization answers 401 instead. |
| Users aren't created, with 403 | The [license](#licensing): Enterprise, active or in grace. |
| Users aren't created, with 400 `invalidValue` | The user has a primary email, or a `userName` that's an email address. |
| Users aren't created, with 409 `uniqueness` | Another member already has that `userName` or `externalId`. |
| Entra's lookups fail with `invalidFilter` | The matching attribute is `userName`, not an email attribute. |
| A group's members don't get its role | The group is mapped. The identity provider has created or updated the members. They aren't owners. Another mapped group they're in may win. |
| Someone removed in Zone keeps coming back | Remove them at the identity provider; it adds people it still assigns. |
| Requests fail with 429 | The provider is sending more than 600 requests a minute for the organization, or more than 1,200 from one address, to one replica. It retries after `Retry-After`. |
| Provisioning stopped after a token change | Paste the new secret into the provider. A rotated secret stops working at once. |

## Audit log

SCIM requests write these entries to the organization's audit log, with no
actor. Every entry has the token's `scim_token_id` and `scim_token_name` in
`new_values`, and no entry ever contains a token's secret. Beyond those:

- user entries hold the attributes the request set (`user_name`,
  `external_id`, `display_name`, `given_name`, `family_name`, `email`) and,
  when the member's role changed, `role` and `custom_role`.
  `scim_user.created` adds `created_account`, true when Zone created the
  account; `scim_user.deactivated` and `scim_user.reactivated` hold
  `active`. `scim_user.deactivated`, `scim_user.deleted` and a
  `scim_user.created` with `active: false` hold `signed_out`, whether the
  account's sessions were revoked (see [Deactivation](#deactivation));
- `invitation.revoked` holds the `email` of an invitation that
  deprovisioning deleted;
- group entries hold the `display_name` and `external_id` the request set,
  `members` with the IDs of the members added or removed, and `roles`, a
  list of `{"user_id", "role", "custom_role"}` for each member whose role
  changed. `scim_group.deleted` holds the group's `display_name`, every
  former member in `members`, and `roles` when the group was mapped;
- `workspace.owner_seated` holds the `user_id` of the member made owner.

| Action | When | Resource |
|---|---|---|
| `scim_user.created` | A user is created, or an existing account is added to the organization | `member` |
| `scim_user.updated` | A user's attributes change | `member` |
| `scim_user.deactivated` | A user is deactivated, or deactivating an inactive user revokes sessions they opened since | `member` |
| `scim_user.reactivated` | A user is reactivated | `member` |
| `scim_user.deleted` | A user is removed from the organization | `member` |
| `scim_group.created` | A group is created | `scim_group` |
| `scim_group.updated` | A group's name, external ID or members change | `scim_group` |
| `scim_group.deleted` | A group is deleted | `scim_group` |
| `workspace.owner_seated` | A member is made a workspace's owner because its only owner was deactivated or deleted | `workspace` |
| `invitation.revoked` | Deactivating or deleting a user deletes an invitation waiting on their email | `invitation` |

Changes in the console write these entries, with the owner who made them as
the actor:

| Action | When | Resource |
|---|---|---|
| `scim_token.created` | A token is created | `scim_token` |
| `scim_token.rotated` | A token is rotated | `scim_token` |
| `scim_token.revoked` | A token is revoked | `scim_token` |
| `scim_group.mapped` | A group is mapped to a role, or its mapping changes | `scim_group` |
| `scim_group.unmapped` | A group's mapping is removed, or the custom role it maps to is deleted | `scim_group` |

## Console API

The SCIM provisioning section uses these routes. Each needs `scim:update`,
so only organization owners get past it; anyone else gets 403.

| Method and path | Purpose | License |
|---|---|---|
| `GET /api/organizations/{org_id}/scim` | The licensing state, the base URL, when tokens were last used, the tokens, the groups with their mappings, and the recent errors. | Never |
| `POST /api/organizations/{org_id}/scim/tokens` | Create a token, with `{"name": "Okta"}`. Answers 201 with the token and its `secret`, which is never shown again. | Needed |
| `POST /api/organizations/{org_id}/scim/tokens/{token_id}/rotations` | Replace a token's secret. Answers 201 with the new `secret`. | Needed |
| `DELETE /api/organizations/{org_id}/scim/tokens/{token_id}` | Revoke a token. Answers 204. | Never |
| `PUT /api/organizations/{org_id}/scim/groups/{group_id}/mapping` | Map a group, with exactly one of `{"role": "admin"}`, `{"role": "member"}` or `{"custom_role_id": "…"}`. Answers 200 with the group. | Needed |
| `DELETE /api/organizations/{org_id}/scim/groups/{group_id}/mapping` | Unmap a group. Answers 200 with the group. | Needed |

`GET` returns:

| Field | Meaning |
|---|---|
| `entitled` | The license allows SCIM: Enterprise, active or in grace. |
| `edition` | `community`, `team` or `enterprise`. |
| `status` | `community`, `active`, `grace` or `lapsed`. |
| `base_url` | The organization's SCIM base URL. |
| `last_synced_at` | When any of the organization's tokens was last used, to the minute, or `null`. |
| `tokens` | Active tokens: `id`, `name`, `hint` (the last four characters), `created_by`, `created_at`, `rotated_at` and `last_used_at`. |
| `groups` | Pushed groups: `id`, `display_name`, `external_id`, `members` (a count), `role`, `custom_role` (`{"id", "name"}` or `null`) and `updated_at`. |
| `errors` | The [recent failed requests](#troubleshooting): `id`, `method`, `path`, `status`, `scim_type`, `detail` and `occurred_at`. |

Refusals carry `{"error": "…", "code": "…"}`:

| Code | Status | Meaning |
|---|---|---|
| `license_required` | 403 | This needs a Zone Enterprise license that is active or in its grace period. Mapping a custom role also needs custom roles, which Enterprise includes. |
| `role_exceeds` | 403 | The mapped role holds a permission you don't hold. |
| `scim_token_name_invalid` | 400 | The token name is empty. |
| `scim_mapping_invalid` | 400 | The body names both a role and a custom role, or neither, or a role that isn't `admin` or `member`. |
| `scim_owner_unmappable` | 400 | A group can't map to the owner role. |
| `scim_token_not_found` | 404 | The token doesn't exist in this organization, or was revoked. |
| `scim_group_not_found` | 404 | The group doesn't exist in this organization. |
| `scim_role_not_found` | 404 | The custom role isn't an organization-scope role of this organization. |
