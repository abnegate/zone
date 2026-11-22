# Roles and permissions

Every check Zone makes about what a person may do in an organization or a
workspace is a check for a permission, such as `members:create` or
`workspace:write`. A role is a set of permissions. Zone has fixed built-in
roles, which every edition has, and with Zone Enterprise an organization can
compose its own custom roles from the same permissions and assign them to its
members.

Zone reads a person's roles from the database on every request; sign-in
tokens don't carry them. Background work, such as task runs, auto projects,
source indexing and agent tools, checks again as the person it runs for, so a
change to a role or an assignment applies from the next check. Checks made
inside a database statement, through the `workspace_permits` and
`check_workspace_membership` SQL functions, are `STABLE`: they read the
statement's snapshot, so a change committed while a statement runs applies
from the next statement, not partway through one.

An organization admin or owner manages custom roles under **Settings →
Organization → Roles**. Anyone who may change members' roles assigns them
under **Settings → Organization → Members** and **Settings → Workspace →
Members**.

## Licensing

Creating, editing and deleting custom roles, and assigning a custom role to a
member, need a Zone Enterprise license that verifies and is either active or
in its 30-day grace period after expiry. A Zone Team license doesn't include
custom roles. You install the license under **Settings → This server →
License**.

| License state | Enforcement | Roles page | Assigning roles |
|---|---|---|---|
| Enterprise, active or in grace | Enforced | Editable by holders of `roles:update` | Built-in and custom roles |
| Lapsed (past grace) | Still enforced | Read-only, with a renewal notice | Built-in roles only |
| Team, in any state | Still enforced | Read-only, with an upgrade notice | Built-in roles only |
| Removed, or never installed (Community) | Still enforced | Read-only, with an upgrade notice | Built-in roles only |

Enforcement never looks at the license. A member who holds a custom role keeps
exactly that role's permissions on every license state, including Community,
until someone gives them another role. This differs from
[agent policy](POLICY.md#licensing), which Community doesn't enforce: a custom
role takes the place of a built-in role, so there is no built-in role Zone
could safely fall back to.

Without an Enterprise license that is active or in grace, every create, edit
and delete of a role, and every custom assignment, is refused with
`license_required`. Reading roles and assigning built-in roles always work, so
on any license an admin can move people off a custom role. Deleting a role needs the license, but a role
nobody holds grants nothing.

A stored license key that doesn't verify counts as no license at all: the
server reports the Community edition and status, roles stay enforced, the
Roles page shows the upgrade notice, and edits and custom assignments are
refused with `license_required`.

## Built-in roles

Organizations have three built-in roles and workspaces have four. Each is a
fixed set of permissions, and each set contains the one below it:

- organization: member, then admin, then owner;
- workspace: viewer, then member, then admin, then owner.

The tables in [Permissions](#permissions) list which built-in roles hold each
permission. A built-in role allows exactly what it allowed before permissions
existed; only the way Zone checks it changed.

Organization and workspace memberships are independent. Being an
organization admin doesn't make you a member of its workspaces, and a
workspace role grants nothing in the organization. The organization reaches
into its workspaces in four places only:

- `policies:update` changes every workspace's agent policy too;
- workspace AI and MCP settings also need organization membership;
- `workspaces:create` creates a workspace, and its creator becomes the
  workspace's owner, with every workspace permission there, including its
  members, AI settings, MCP servers and agent policy;
- `invitations:create` seats invitees in the organization's workspaces (see
  [Invitations](#invitations)).

## Permissions

Each permission is named `resource:action`. A permission belongs to one
scope, organization or workspace, and is checked against your membership of
that scope. Each scope has a base permission that every role of that scope
holds: `organization:read` and `workspace:read`.

A permission is either composable, which means a custom role may include it,
or not. Everything that seats, changes or removes owners and admins is not
composable, and neither is deleting the organization or a workspace.

A composable permission can require another: a custom role that holds it
must hold that one too. Only `spend:update` has a requirement, `spend:read`,
because a budget's answer carries its target's spend, name and email.

### Organization permissions

| Permission | What it allows | Built-in roles | In custom roles |
|---|---|---|---|
| `organization:read` | See the organization and its members, its GitHub connection and repositories, its MCP servers and its agent policy | member, admin, owner | Always, as the base |
| `organization:update` | Change the organization's name and description | admin, owner | Yes |
| `organization:delete` | Delete the organization | owner | No |
| `members:create` | Add a member | admin, owner | Yes |
| `members:update` | Change a member's role | admin, owner | Yes |
| `members:delete` | Remove a member | admin, owner | Yes |
| `administrators:update` | Seat, change and remove admins and owners, and widen a workspace role people hold (see [Escalation](#escalation)) | owner | No |
| `invitations:create` | Invite people by email, including into the organization's workspaces | admin, owner | Yes |
| `invitations:read` | List pending invitations | admin, owner | Yes |
| `invitations:delete` | Revoke an invitation | admin, owner | Yes |
| `workspaces:create` | Create a workspace, and become its owner with every workspace permission in it | admin, owner | Yes |
| `devices:read` | List the organization's devices and read the device policy | admin, owner | Yes |
| `devices:update` | Allow, block or rename a device, and change the device policy | admin, owner | Yes |
| `github:update` | Register and delete the organization's GitHub App, and add, sync and remove its installations | admin, owner | Yes |
| `mcp_servers:update` | Add, change and remove the organization's MCP servers | admin, owner | Yes |
| `ai_settings:read` | Read the organization's AI settings | admin, owner | Yes |
| `ai_settings:update` | Change and clear the organization's AI settings | admin, owner | Yes |
| `agent_logins:update` | Sign the organization's coding agents in and out | admin, owner | Yes |
| `policies:update` | Change the organization's agent policy, and the agent policy of every workspace in it | admin, owner | Yes |
| `audit_logs:read` | Read and download the organization's audit log | admin, owner | Yes |
| `audit:update` | Manage audit streaming destinations, exports and retention | admin, owner | Yes |
| `spend:read` | See the organization's spend reports and budgets, and download spend as CSV (see [SPEND.md](SPEND.md)) | admin, owner | Yes |
| `spend:update` | Create, change and delete the organization's budgets | admin, owner | Yes, with `spend:read` |
| `roles:update` | Create, edit and delete custom roles | admin, owner | No |

### Workspace permissions

| Permission | What it allows | Built-in roles | In custom roles |
|---|---|---|---|
| `workspace:read` | See the workspace and its members, and read its chats, projects, tasks, sources, knowledge, pull requests, releases, analytics, theme, actions, AI settings, MCP servers and agent policy | viewer, member, admin, owner | Always, as the base |
| `workspace:write` | Chat, create and run tasks and projects, change sources, knowledge, documents, the theme and actions, and mount host folders | member, admin, owner | Yes |
| `workspace:update` | Change the workspace's name and description | admin, owner | Yes |
| `workspace:delete` | Delete the workspace | owner | No |
| `workspace_members:create` | Add a member | admin, owner | Yes |
| `workspace_members:update` | Change a member's role | admin, owner | Yes |
| `workspace_members:delete` | Remove a member | admin, owner | Yes |
| `workspace_administrators:update` | Seat, change and remove workspace admins and owners | owner | No |
| `workspace_ai_settings:update` | Change the workspace's AI settings | admin, owner | Yes |
| `workspace_mcp_servers:update` | Change the workspace's MCP server settings | admin, owner | Yes |
| `workspace_policies:update` | Change the workspace's agent policy | admin, owner | Yes |
| `project_syncs:update` | Add and remove a project's sync, and rotate its webhook secret | admin, owner | Yes |
| `pull_requests:override` | Merge a pull request from the Pull Requests page as a GitHub administrator, when the server allows it with [`ZONE_GITHUB_ADMIN_MERGE`](CONFIGURATION.md#zone_github_admin_merge) (off by default) | admin, owner | Yes |

### Server permissions

A few permissions belong to the server, not to an organization, and come from
the server-wide roles `admin`, `user` and `viewer`: `models:create`,
`models:delete`, `pull_requests:update` and `releases:create`. The server
admin flag, which covers the license, single sign-on and server-wide audit
settings, is separate again. None of these can go in a custom role, and an
organization's roles never grant them. Merging a pull request needs
`pull_requests:update` on the server and `workspace:write` in the workspace;
creating a release needs `releases:create` and `workspace:write`.

## Custom roles

A custom role belongs to one organization and has a fixed scope:

- an **organization role** is given to organization members, and holds
  organization permissions;
- a **workspace role** is given to members of any workspace in that
  organization, and holds workspace permissions.

The scope is chosen when the role is created and can't be changed.

A custom role replaces a built-in role; it doesn't add to one. A member who
holds a custom role has the role's permissions plus the base permission of
its scope, and nothing else. Zone stores such a member's built-in role as
`member` in an organization or `viewer` in a workspace, so a custom-role
holder never counts as an admin or owner, whatever permissions the role holds.

The `role` the API reports is a rank, not the stored role (see
[What people see in the console](#what-people-see-in-the-console)). A
workspace role holding every composable workspace permission ranks as
`admin`, yet its holders are still stored as viewers: they don't count
towards the admin floor, and changing their role needs no administrators
permission. No organization role can rank as `admin`, because `roles:update`
isn't composable.

Names are trimmed, must not be empty, and are unique within the organization,
ignoring case. A name can't be `owner`, `admin`, `member` or `viewer`. There
are no length limits.

### What grants nothing

A custom role grants only what is both known and allowed for its scope. These
grant nothing:

- a stored permission Zone doesn't know, for example one removed in a later
  release;
- a stored permission that isn't composable;
- a stored permission of the other scope, such as `members:create` on a
  workspace role;
- a stored permission without the one it requires, such as `spend:update`
  without `spend:read`;
- a membership that refers to a role of another organization or of the wrong
  scope. Such a membership doesn't even get the base permission, so its
  holder is treated as not being a member.

Three layers enforce this: the role API refuses such a permission when a role
is saved, the database refuses to store a permission that is neither
composable nor a base, and the server ignores anything else it reads.

## Escalation

Nobody can hand out more than they hold. The rules below apply to every way a
role is given: changing a member's role, adding a member, reactivating a
former member, and inviting someone.

### Changing a member's role

Changing a member's role needs `members:update` in the organization, or
`workspace_members:update` in the workspace. For a workspace member, your
permissions in that workspace are what count, not your organization role.
A body that names both a role and a custom role, or neither, is refused with
400. Before it looks at the member, Zone checks what you're giving:

- a custom role needs the license, otherwise 403 `license_required`;
- the organization owner role, or the workspace admin or owner role, needs
  `administrators:update` (organization) or `workspace_administrators:update`
  (workspace). Only owners hold these. Otherwise: 403.

Zone then locks the membership and checks, in order:

1. If the member holds the built-in admin or owner role, you need the same
   administrators permission. Otherwise: 403.
2. Your permissions must include every permission the member holds now.
   Otherwise: 403.
3. Giving the organization owner role, or the workspace admin or owner role,
   is checked again under the lock. Otherwise: 403.
4. A custom role must belong to this organization and match the membership's
   scope, otherwise 404 `Role not found`.
5. Your permissions must include every permission of the role you're giving.
   Otherwise: 403 `role_exceeds`.
6. The [owner and admin floors](#owners-and-the-last-owner) apply.

A member holds either a built-in role or a custom role, never both. Giving a
member a built-in role clears their custom role.

### Adding and reactivating members

Adding a member, or reactivating a former one, gives a built-in role only;
give a custom role afterwards. It needs `members:create` or
`workspace_members:create`, and then:

- in an organization, adding an admin or owner needs `administrators:update`;
- in a workspace, adding an admin or owner needs
  `workspace_administrators:update`;
- your permissions must include every permission of the role you're giving,
  otherwise 403 `role_exceeds`. For example, a custom-role holder with
  `workspace_members:create` but not `workspace:write` can add viewers but
  not members.

### Removing members

Removing someone from an organization also takes them out of every workspace
in it, and stops what they have running there: their chat turns (reminder and
automation turns included), the task runs they started, and any invitation to
the organization still waiting on their email. Their sessions aren't revoked,
because they may belong to other organizations.

- If they were a workspace's only owner, another member of the workspace is
  made owner: whoever created the workspace, then an admin, a member or a
  viewer, earliest to join first, and only someone who is still in the
  organization. This is recorded as `workspace.owner_seated`. If nobody is
  left, the workspace has no owner until an organization owner adds one.
- Adding a removed member back, or their accepting a new invitation, starts
  them afresh. Their old workspace memberships and roles don't come back; add
  them to workspaces again.

### Invitations

An invitation names an organization role (`org_role`), and optionally a
workspace role (`workspace_role`) and the workspaces it applies to
(`workspace_ids`). Invitations give built-in roles only.

- Inviting someone as an organization owner, or as a workspace admin or
  owner, needs `administrators:update`.
- Your organization permissions must include every permission of the
  invited `org_role`, otherwise 403 `role_exceeds`. No custom role can hold
  `roles:update`, so a custom-role holder can only invite members.
- The `workspace_role` reaches every workspace listed in the invitation, and
  isn't checked against your permissions in those workspaces. A holder of
  `invitations:create` can seat invitees as workspace members or viewers in
  any workspace of the organization, including one they don't belong to.
  This is the reach organization admins have always had, so give
  `invitations:create` only to people you'd trust to add members to every
  workspace.

### Editing a role

Creating, editing and deleting roles needs `roles:update`, which only
built-in organization admins and owners hold, and the license.

Narrowing a role, so that it holds fewer permissions, is always allowed.
Widening a **workspace** role that active members hold needs
`administrators:update`, so only an organization owner can do it; anyone
else is refused with 403 `role_exceeds`. An organization admin isn't
necessarily an admin of every workspace, so widening a role people already
hold would otherwise raise their access in workspaces the editor doesn't
administer. Widening a workspace role nobody holds, or any organization role,
needs only `roles:update`: an organization role can hold at most what an
organization admin holds.

### Owners and the last owner

- Owners are always built-in. A custom role can't hold the owner-only
  permissions, and its holder is stored as a member or viewer.
- Giving an admin or owner a custom role is a demotion, so it needs the
  administrators permission like any other change to an admin or owner.
- An organization's last owner can't be demoted or removed.
- A workspace's last owner can't be demoted or removed, and neither can its
  last member who is an admin or owner.
- A custom-role holder never counts towards these floors.

### Deleting a role

A role that any active member holds can't be deleted. Zone refuses with 409
`role_in_use` and the number of holders; give them another role first.
Deactivating a member clears their custom role, so a role held only by former
members can be deleted. Deleting an organization deletes its roles with it.

## What people see in the console

The API reports two things about each membership:

- `permissions`: the exact permissions it holds;
- `role`: its rank, the highest built-in role whose permissions it holds in
  full, or `member` (organization) or `viewer` (workspace) when it holds no
  higher one in full. A custom role with `workspace:write`, for example,
  ranks as a workspace member.

The console gates each control on the permission the server checks for it,
read from `permissions`:

| Control | Permission |
|---|---|
| Settings sections and their edit controls | The read or update permission of each section, such as `devices:read` and `devices:update` |
| Adding, changing and removing organization members | `members:create`, `members:update` and `members:delete` |
| Adding, changing and removing workspace members | `workspace_members:create`, `workspace_members:update` and `workspace_members:delete` |
| Changing or removing a built-in admin or owner, and giving an admin or owner role | `administrators:update`, or `workspace_administrators:update` in a workspace |
| The roles offered when changing or adding a member | Only roles, built-in or custom, whose permissions you hold in full |
| The organization role offered in an invitation | Member always; Admin from the admin rank; Owner with `administrators:update` |
| The workspace role offered in an invitation | Viewer and Member always; Admin and Owner with `administrators:update` |
| Signing coding agents in and out | `agent_logins:update`, even where the AI settings are read-only |
| The **Connect GitHub** link | `github:update` |

So the members sections and the invitation form offer only roles the server
accepts. They're stricter in one place: in an organization, only owners are
offered the admin role, when adding a member and when changing one, although
the server lets an admin promote a member to admin.

A workspace member whose custom role ranks as `admin` is treated as a
custom-role holder, not as an admin: anyone who may change workspace roles
and holds that role's permissions can change it.

Two app-wide controls still use the rank: whether a workspace accepts your
writes, and adding sources. Both need the workspace member rank, and a
membership ranks as a member or higher exactly when it holds
`workspace:write`, so the rank and the permission always agree there.

Where the console doesn't know a membership's permissions, every control
falls back to the rank, which is how it behaved before custom roles. The
server checks every request either way.

The **Roles** page appears to holders of `roles:update`. Any organization
member can read the roles API, because whoever changes workspace roles needs
the list to give them. In its editor, choosing a permission also chooses the
one it requires, and clearing a permission clears those that require it, so
choosing **Manage budgets** chooses **See spend**.

## Audit log

| Action | When | Resource | Recorded values |
|---|---|---|---|
| `role.created` | A custom role is created | `role`, with the role's ID | The new `name`, `description`, `scope` and `permissions` |
| `role.updated` | A custom role's name, description or permissions change; an edit that changes none of them isn't recorded | `role`, with the role's ID | The values before and after |
| `role.deleted` | A custom role is deleted | `role`, with the role's ID | The values before |
| `member.role_changed` | A member is given a built-in or custom role | The membership | `user_id`, `role` and `custom_role` |
| `workspace.owner_seated` | A member is made a workspace's owner because its only owner left the organization | `workspace`, with the workspace's ID | The new owner's `user_id` |

## Codes

| Code | Status | Meaning |
|---|---|---|
| `license_required` | 403 | This needs a Zone Enterprise license that is active or in its grace period. See [Licensing](#licensing). |
| `role_exceeds` | 403 | The change would give someone a permission you don't hold, or widen a workspace role people hold without `administrators:update`. |
| `permission_unknown` | 400 | A permission name isn't in the catalogue. |
| `permission_not_composable` | 400 | A permission can't go in a custom role. |
| `permission_out_of_scope` | 400 | A permission belongs to the other scope. |
| `permission_requires` | 400 | A permission needs another that the role doesn't hold, such as `spend:update` without `spend:read`. |
| `name_invalid` | 400 | The name is empty. |
| `name_reserved` | 400 | The name is a built-in role's. |
| `role_name_taken` | 409 | Another role in the organization has this name, ignoring case. |
| `role_in_use` | 409 | Active members hold the role. The response's `members` says how many. |

A refusal without a code carries its reason in `error`, such as
"Organization admin access required".

## API

| Method and path | Purpose |
|---|---|
| `GET /api/organizations/{org_id}/roles` | The permission catalogue, the organization's custom roles and the licensing state. |
| `POST /api/organizations/{org_id}/roles` | Create a custom role. |
| `PATCH /api/organizations/{org_id}/roles/{role_id}` | Edit a custom role. |
| `DELETE /api/organizations/{org_id}/roles/{role_id}` | Delete a custom role. |
| `PATCH /api/organizations/{org_id}/members/{user_id}` | Give an organization member a built-in or custom role. |
| `PATCH /api/workspaces/{workspace_id}/members/{user_id}` | Give a workspace member a built-in or custom role. |

Anyone who isn't an active member of the organization gets 404 from the roles
routes, as if the organization didn't exist. A change to a role is checked in
this order: `roles:update` (403), the license (403 `license_required`), the
body (400 with one of the codes above), then the name (409
`role_name_taken`).

### Reading roles

```json
{
  "entitled": true,
  "editable": true,
  "edition": "enterprise",
  "status": "active",
  "catalogue": [
    { "name": "organization:read", "scope": "organization", "composable": false },
    { "name": "members:create", "scope": "organization", "composable": true },
    { "name": "workspace:write", "scope": "workspace", "composable": true }
  ],
  "roles": [
    {
      "id": "4b0f6c1e-1d7a-4a43-9a52-2f3c1f0b8e11",
      "organization_id": "9d1e2c3b-5a6f-4e7d-8c9b-0a1b2c3d4e5f",
      "name": "Reviewer",
      "description": "Writes in the workspace and merges reviewed work",
      "scope": "workspace",
      "permissions": ["pull_requests:override", "workspace:read", "workspace:write"],
      "members": 3,
      "created_at": "2026-11-17T09:30:00Z",
      "updated_at": "2026-11-17T09:30:00Z"
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `entitled` | The license allows editing: Enterprise, active or in grace. |
| `editable` | The license allows editing and you hold `roles:update`. |
| `edition` | `community`, `team` or `enterprise`. |
| `status` | `community`, `active`, `grace` or `lapsed`. |
| `catalogue` | Every organization and workspace permission, with its scope and whether a custom role may include it. Server permissions aren't listed. |
| `roles` | The organization's custom roles. `permissions` includes the base permission, and `members` counts the active organization and workspace memberships that hold the role. |

### Creating and editing roles

`POST` takes a `name`, an optional `description`, a `scope` and the
`permissions`, and answers 201 with `{"role": …}`. Zone adds the base
permission itself.

```http
POST /api/organizations/{org_id}/roles
Content-Type: application/json

{"name": "Reviewer", "scope": "workspace", "permissions": ["workspace:write", "pull_requests:override"]}
```

A `PATCH` body carries only what you're changing:

- `name`;
- `description`, where `null` clears it;
- `permissions`, which replaces the whole list.

Any other key, including `scope`, is refused with 400. `PATCH` answers 200
with `{"role": …}`, or 404 when the role isn't in the organization.

`DELETE` answers 204, 404, or 409 with
`{"error": "…", "code": "role_in_use", "members": 3}`.

### Giving members roles

The member `PATCH` body names exactly one of a built-in role or a custom
role. A body with both, or neither, is refused with 400.

```http
PATCH /api/workspaces/{workspace_id}/members/{user_id}
Content-Type: application/json

{"custom_role_id": "4b0f6c1e-1d7a-4a43-9a52-2f3c1f0b8e11"}
```

To give a built-in role instead, send `{"role": "member"}`. A built-in role
never needs the license; a custom role does.

Member rows, in lists and in the answers to adding and changing a member,
carry `role` (the rank), `custom_role` (`{"id", "name"}` or `null`) and
`permissions`. The organization and workspace list and get responses carry
the same three fields for your own membership.

Adding a member and inviting someone take built-in roles only, as before.

## Design decisions

- **One permission per check.** There's a permission for each distinct check
  the server makes. Workspace content is `workspace:read` and
  `workspace:write`, because reading versus writing is the only distinction
  the server draws there.
- **Server permissions stay global.** Model management, pull request merging,
  releases and the server admin flag stay with the server-wide roles.
- **Roles are per organization, with a fixed scope.** A role can be given to
  memberships of its scope inside its organization.
- **A custom role replaces a built-in role.** Its holder is stored as a member
  or viewer, so custom-role holders never count as admins or owners.
- **Owner powers aren't composable.** `organization:delete`,
  `administrators:update`, `workspace:delete` and
  `workspace_administrators:update` stay with owners, and `roles:update`
  stays with built-in admins and owners.
- **Widening a held workspace role needs an owner.** Otherwise an
  organization admin could raise people's access in workspaces they don't
  administer. Narrowing is always allowed.
- **A role in use can't be deleted.** Zone refuses rather than guess which
  role its holders should get instead.
- **Adding and inviting give built-in roles only.** Give a custom role
  afterwards. An invitation's `workspace_role` keeps the reach organization
  admins have always had.
- **The license gates editing, never enforcement.** Community and lapsed
  servers keep enforcing existing custom roles.
- **The console gates on permissions.** Settings sections, member and
  invitation controls, agent sign-in and the Connect GitHub link follow the
  permission the server checks. Only whether a workspace accepts writes and
  adding sources still use the rank, where member rank and
  `workspace:write` are the same test. The server is authoritative
  everywhere.
- **Enforcement is Community code.** Permission checks are part of every
  edition. Only the license gate on editing belongs to Zone Enterprise.
