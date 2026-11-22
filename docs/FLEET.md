# Runner fleet policy

A runner fleet policy controls which Zone server processes may take agent
work. A server admin can cordon or revoke a runner and set a minimum Zone
version, and an organization or workspace can require runner labels for its
unattended work. Zone checks the policy on the server, inside the same
transaction that claims the work, every time a task run, a chat turn, a
reminder turn or an auto-project drive is claimed. The console only shows
what the server already enforces.

Server admins see the fleet under **Settings → This server → Runners**.
Runner labels are set in the **Runner placement** card on the **Agent
policy** pages, under **Settings → Organization → Agent policy** and
**Settings → Workspace → Agent policy**.

## What a runner is

A runner is one Zone server process: one per container or pod. Every server
process registers itself as a runner when it starts, on every edition, and
there is nothing to install or connect. When Helm runs several server
replicas, each replica is a runner, and the fleet is the set of server
processes sharing one database.

A runner is not:

- **`zone-runner`.** That binary is a local command subprocess the server
  spawns over stdio. It never claims work and doesn't appear in the fleet.
- **A device.** Devices are the clients people sign in from.
- **A remote deployment runner.** [DEPLOY.md §6](DEPLOY.md#6-runners) plans
  remote `zone-runner connect` runners for deployment jobs, stored in a
  `runners` table. Fleet runners are stored in `fleet_runners`, so that name
  stays free; those remote runners could later join this fleet as a second
  kind.

## Identity

A runner registers with:

- a **name**: [`ZONE_RUNNER_NAME`](CONFIGURATION.md#zone_runner_name), or
  else the `HOSTNAME` environment variable, or else `/etc/hostname`, or else
  `zone-` followed by 8 random hex digits;
- a **per-process instance ID**, new every time the process starts;
- its **version**, the version of the build it runs;
- its **labels**, from
  [`ZONE_RUNNER_LABELS`](CONFIGURATION.md#zone_runner_labels), such as
  `region=eu,gpu`.

The server reads both variables once at startup. It registers before it
starts any worker, then heartbeats every 15 seconds. Each heartbeat
registers again, so a runner whose database was briefly unreachable at boot
registers once it can. A runner counts as **connected** while it was seen in
the last 60 seconds.

A server whose `ZONE_RUNNER_LABELS` doesn't parse refuses to start, so a
misconfigured server can't boot into the wrong placement. Its message begins:

```text
Refusing to start: ZONE_RUNNER_LABELS is invalid (fleet_label_invalid).
```

### Names carry state

State belongs to the name, not the process. A cordoned or revoked name stays
cordoned or revoked when its process restarts, and a new process registering
under the name takes over its row: the row's instance ID, version and labels
become the new process's, and `started_at` restarts.

That has consequences for how names are chosen:

- **Names should be unique.** The default host name is unique per container
  or pod. If two live processes share a name, they share its state, and the
  row shows whichever registered last. Sharing a name never bypasses
  placement: under enforcement a claim needs the row to hold the claiming
  process's own instance ID, so a process whose name another took over is
  refused until its next heartbeat (every 15 seconds) registers it again,
  and is then judged on its own version and labels.
- **A Kubernetes Deployment mints new pod names on every rollout.** The new
  pods register as new runners, and a cordon or revoke on an old pod's name
  doesn't carry over to them. The old names go offline and, unless revoked,
  are [forgotten](#removing-runners-and-retention) after 7 days. Set
  `ZONE_RUNNER_NAME` when you want a stable name.
- **Without a host name**, the random `zone-…` name changes on every start,
  so state never carries over.

When a runner registers under a name that another process held within the
last 60 seconds, Zone logs a warning. A fast restart triggers it once, as the
new process takes the name over from the old one, and that is expected. If it
repeats, two servers share the name: give each its own `ZONE_RUNNER_NAME`.

## Licensing

Fleet policy is part of Zone Enterprise; a Zone Team license doesn't include
it. Changing it needs a Zone Enterprise license that verifies and is either
active or in its 30-day grace period after expiry.

| License state | Enforcement | Changes |
|---|---|---|
| Enterprise, active or in grace | Enforced | Allowed, for the people listed [below](#who-can-see-and-change-what) |
| Enterprise, lapsed (past grace) | Still enforced | Refused with `license_required` |
| A stored key that no longer verifies | Still enforced | Refused with `license_required` |
| Team, in any state | Not enforced | Refused with `license_required` |
| Removed, or never installed (Community) | Not enforced | Refused with `license_required` |

As with [agent policy](POLICY.md#licensing), a lapsed license never loosens
the fleet. Every change is refused until the license is renewed, including
activating a runner and clearing the minimum version. With a minimum version
set, the only way out of a lapsed license is to renew it or remove it.

Without enforcement, nothing in the fleet policy applies: every runner takes
every kind of work, whatever its state, version or labels, and no runner
picks up another's runs. Zone keeps the saved states, minimum version and
labels, and enforces them again as soon as a Zone Enterprise license is
installed.

If Zone can't read the license when a runner claims work, it treats fleet
policy as enforced and logs a warning. A fleet with nothing set still admits
everything, since every runner starts active, with no minimum and no labels
required.

Every edition writes a little more than it did before fleet policy:
registration at boot, the heartbeat every 15 seconds, and the daily retention
sweep. Task runs, chat leases and auto-project claims also record the runner
that claimed them. Community servers write the server-scope audit entries
`runner.registered` and `runner.removed` too (see [Audit log](#audit-log)).

## Who can see and change what

| What | Who can see it | Who can change it |
|---|---|---|
| The runners and the minimum version | Server admins, on every edition | Server admins |
| An organization's runner labels | Every active member of the organization | Holders of `policies:update` |
| A workspace's runner labels | Members of the workspace, and holders of `policies:update` in the organization | Holders of `workspace_policies:update` in the workspace, and of `policies:update` in the organization |
| The labels the fleet's runners carry, with counts | Whoever may change an organization's or workspace's runner labels | Each runner's own `ZONE_RUNNER_LABELS` |

Runner labels are edited by the people who edit agent policy, with the same
permissions (see [ROLES.md](ROLES.md)). Someone who can see them but not
change them sees them read-only. Someone who isn't a member gets 404, as if
the organization or workspace didn't exist.

The fleet belongs to the server, not to an organization. Organization and
workspace responses carry label names and counts only, never a runner's name,
ID, version or run count. The labels runners carry are the whole server's, so
on a server several organizations share, every organization's label editors
see the label names any runner carries. Someone who may only read the labels
gets `available: []`. Keep anything you wouldn't show every organization out
of label names.

## Runner states

A server admin sets each runner's state. The states take their names from
kubectl's cordon.

| State | Task runs | Reminder turns and auto-project drives | Chat turns | Work it already holds |
|---|---|---|---|---|
| `active` | Claims new runs | Claims them | Serves them | Continues |
| `cordoned` | Claims no new runs; they are [handed off](#what-happens-to-a-task-run) | Claims none | Serves them | Continues to the end |
| `revoked` | Claims none | Claims none | Refuses them | Ends at once, below |

Cordon is how to take a replica out without losing work: it finishes what it
has and keeps serving chats. Revoke is quarantine. In the same transaction
that revokes a runner, Zone:

- stops each of its live task runs as a person's stop would: the run is
  cancelled with "The runner this task was running on was revoked from the
  fleet.", and its task is paused. The task stays reserved for the run while
  its owner is alive to tear it down, so no second run starts on the same
  branch. It is released when the owner lets go or, if the owner is gone,
  once 60 seconds pass without its heartbeat. **A paused task needs a person
  to start it again.**
- expires its chat leases, so its chat turns stop;
- ends its auto-project claims, so its drives stop at once: their next
  write or renewal is refused. Each project it was driving stays
  unclaimable for one claim lease (15 minutes), so no other runner drives it
  while the revoked process may still be finishing work outside the
  database. After that, another runner may claim it.

Its reminder claims stop renewing, so each firing it held lapses and is
offered to another runner.

Activating a runner returns it to `active`. A revoked runner can also be
moved to `cordoned` through the [API](#api).

On the **Runners** page, each runner's row offers **Cordon** and **Revoke**
while it is active, **Activate** and **Revoke** while it is cordoned, and
**Activate** while it is revoked. Each opens a dialog that names the runner
and what the change does, confirmed with **Cordon runner**, **Revoke
runner** or **Activate runner**. Revoke's dialog says that the runner's live
task runs stop and its chats end; on the runner that served the page, it
also says your own chats on it end and to reload the page to reach another
server. Without a Zone Enterprise license that is active or in grace, the
page is read-only and offers none of these. With a stored key that no longer
verifies, the page also says that the fleet policy is still enforced and
that changing it is blocked until a server admin replaces or removes the
key.

## Minimum version

A runner below the minimum version claims no task runs, reminder turns,
auto-project drives or chat turns. Task runs it admits go to a current
runner, or fail with a message that names the minimum. This is the lever for upgrades
and security fixes: set the minimum to the release you upgraded to, and the
replicas still on the old one stop taking work.

A runner reports its version as `MAJOR.MINOR.PATCH[-pre-release][+build]`,
such as `1.4.0` or `1.5.0-rc.1`. A minimum must be a plain
`MAJOR.MINOR.PATCH`; anything else, such as `1.4`, `v1.4.0` or
`1.4.0-rc.1`, is refused with `fleet_version_invalid`. Versions compare as
semantic versions: a pre-release of the minimum is below it, and a
pre-release of a later release is above it, so a minimum of `1.4.0` refuses
`1.4.0-rc.1` and admits `1.4.1-rc.1`. Build metadata is ignored.

**Every build reports 0.1.0 today.** A runner's version is the version of the
`zone_server` crate, and every crate in the workspace takes its version from
`[workspace.package] version` in `runner/Cargo.toml`, which is `0.1.0`. Until
releases bump that version, every runner reports `0.1.0`: a minimum of
`0.1.0` or lower refuses nothing, and anything higher refuses every runner,
which the [lockout guard](#the-lockout-guard) catches. The minimum version
becomes useful once releases bump it.

## Placement

An organization and each of its workspaces can require labels. Unattended
work in a workspace (task runs, reminder turns and auto-project drives) needs
an active runner that carries every label the organization requires and every
label the workspace requires, together.

- **Labels combine as a union.** A workspace's own labels add to its
  organization's, so a workspace can only narrow where its work runs, never
  widen it.
- **An empty list requires nothing.** `[]` clears a requirement. This is the
  opposite of agent policy, where `[]` allows nothing.
- **Labels come from the work's own workspace.** Zone always reads the
  requirement from the workspace the work belongs to and that workspace's
  organization, never from anything the request names, so one organization's
  labels never place another's work. An auto project with no workspace is
  judged on the runner's state and version alone.

A label is 1 to 63 characters of lowercase letters, digits and `.`, `_`,
`-`, `=` and `/`, starting with a letter or digit. Uppercase is refused, so
`GPU` is invalid and `gpu` is not. Anything else is refused with
`fleet_label_invalid`. Labels are plain strings: `region=eu` is one label,
and Zone gives the `=` no meaning. A runner carries exactly the labels its
own `ZONE_RUNNER_LABELS` lists, and they change only when it restarts with
new ones.

The **Runner placement** card shows the required labels. In a workspace it
shows the organization's labels as **Required by the organization**, locked,
beside **Required by this workspace**, and the union as **In effect**. It
says how many connected, active and current runners carry every required
label. When that count is 0, the card warns that no active runner on the
minimum version carries the labels, so the work waits until one does. Only
someone who may change the labels also sees, under **Labels runners carry**,
the labels the fleet's runners carry, with how many connected runners that
aren't revoked carry each (see
[Who can see and change what](#who-can-see-and-change-what)).

With a lapsed license, or a stored key that no longer verifies, the card
stays on the **Agent policy** pages and shows the requirement read-only, with
a notice that labels are still enforced and changing them is blocked until
the license is renewed or a server admin replaces or removes the key.

### Placement, not isolation

Labels decide where honest runners take unattended work. They are not a
boundary:

- **Interactive chats ignore labels.** A chat turn runs on whichever runner
  serves the chat, labelled or not. Labels are therefore not data residency:
  `region=eu` keeps a workspace's task runs on EU runners, not its chats.
- Every runner holds the same database credentials and can read every
  organization's data. See [What fleet policy doesn't cover](#what-fleet-policy-doesnt-cover).

### Why labels aren't part of agent policy

Labels have their own storage, apart from the agent policy, for two reasons:

- they sit behind a different license feature (fleet policy, not agent
  policy), so whether they can be changed and whether they apply is decided
  apart from the agent policy;
- they combine differently. Agent policy fields intersect, and `[]` allows
  nothing; labels add up, and `[]` requires nothing.

## What happens to a task run

1. **Claim.** The runner that admits a task run claims it. Under enforcement
   the claim succeeds only if the runner is active, current and carries the
   labels the run's workspace requires. The run records the runner that
   claimed it, on every edition.
2. **Hand-off.** If the runner may not take the run, it looks for another
   connected runner that may. When there is one, the run is left for it.
3. **Pickup.** Every 5 seconds, each runner under enforcement looks for runs
   it may take that are at least 2 seconds old and still unclaimed, oldest
   first, and claims them even when it is busy, up to a queue as deep as
   its execution slots: a claimed run waits on that runner for a free slot,
   heartbeating, as any queued run does. If the run is still unclaimed about
   30 seconds after it was admitted, the runner that left it fails it with
   `fleet_no_runner`, as in the next step.
4. **No runner.** When no connected runner may take the run, it fails at
   once, and Zone records `fleet.run_refused` in the run's organization. The
   message names what a runner would need: "No runner in this server's fleet
   can take this run. It needs an active runner on Zone 1.4.0 or later
   labelled gpu, region=eu." The version appears only when a minimum is set,
   and the labels only when the workspace requires some. The fail only
   touches a run nobody has claimed, so a runner that picked the run up
   meanwhile keeps it.

The check applies when a run is claimed. A run parked on a question keeps
its runner when it resumes; the task's next run is checked again.

### Reminder turns and auto projects

A runner doesn't claim a reminder turn or an auto-project drive it may
not take, and another runner that may takes it on its next pass. Work that
**no** runner can take stays owed: a reminder firing waits, and an auto
project isn't driven. Zone sends no notice and writes no audit entry for
this. The warning in the **Runner placement** card when no runner matches is
the hint. Only task runs fail with `fleet_no_runner`.

A reminder turn whose chat refuses it, for example on a revoked runner, gives
its claim back without counting an attempt, so refusals can't use up the
attempts a firing has. Nobody is watching an unattended turn, so the refusal
isn't posted into the chat.

## The lockout guard

A change that reduces which runners may take work is refused with 409
`fleet_no_runner_left` when it would take the connected runners that are
active and at or above the minimum version from some to none. Labels aren't
counted. The guarded changes are:

- cordoning an active runner;
- revoking an active or cordoned runner;
- setting a minimum version where there was none, or raising it.

A guarded change strands nothing more when the fleet is already left with no
such runner, or when it touches a runner that isn't counted, such as an
offline runner or one below the minimum, so it needs no confirming. Revoking
a dead offline runner, or cordoning an outdated one, goes through at once.

Activating a runner, moving a revoked runner to `cordoned`, and clearing or
lowering the minimum never need confirming. When the server refuses a change
this way, the **Runners** page opens a **Leave no runner able to take
work?** dialog. **Do it anyway** sends the same request again with
`confirm: true`, which skips the guard.

Each guarded change holds the fleet policy row locked while it counts, so
two admins can't each leave the other's runner as the last one.

## Guarantees under races

- **A claim racing a revoke** is either refused, or commits first and is then
  stopped by the revoke. No live task run, unexpired chat lease or auto-project
  claim is ever left on a revoked runner.
- **Two runners picking up one run**: exactly one claims it.
- **A run failed for lack of a runner** is never one another runner had
  already picked up.
- **Label requirements and the minimum version apply at claim time.** A
  change applies to claims that start after it is saved; work in flight
  continues. Only revoke ends live work.

**The revoke window.** A revoked runner's process may keep its agent running,
and so keep calling the model, until it next checks its claim: for a task
run, up to one heartbeat interval (15 seconds). None of its fenced writes
land in that time: the run is already cancelled, its chat leases have
expired and its auto-project claims have ended, so its progress, results
and messages are refused.

## Removing runners and retention

With the license, a server admin can remove a runner that isn't connected:
the **Runners** page offers **Remove** only on offline runners, confirmed
with **Remove runner**. A connected runner is refused with 409
`fleet_runner_online`, since its next heartbeat would only register it
again; stop its server first, or cordon or revoke it instead. An offline
runner that still holds live work, a running or waiting task run or an
unexpired chat lease, is refused with 409 `fleet_runner_busy`, as retention
spares it too: remove it once that work ends, or revoke it to end the work
now.

Once a day, Zone forgets runners that haven't been seen for 7 days and hold
no live work: no running or waiting task run and no unexpired chat lease.
The first sweep runs 5 minutes after a server starts, so a fleet that
redeploys every day still sweeps. Each removal is audited as
`runner.removed` with no actor. Revoked runners are never forgotten, so a
quarantined name stays quarantined.

"State belongs to the name" lasts only until the row is removed. **Removing a
revoked runner and then starting its server again registers the name afresh,
as `active`.** To keep a revoked name out, leave its row in place.

## What fleet policy doesn't cover

- **Anyone who holds the database credentials or controls a runner's
  binary.** Fleet policy governs honest runners: drift, upgrades, placement
  and quarantine. Every replica holds the database credentials, and a
  compromised process can ignore its own revocation. It isn't a security
  boundary.
- **Approval before a runner may work.** There is no allowlist. Approval
  wouldn't be a boundary, since every replica holds the database credentials,
  and a Deployment mints new pod names on each rollout, so every rollout
  would stall waiting for it.
- **Restricting a runner to some organizations.** A runner serves every
  organization on the server.
- **Per-runner capabilities** such as agents, models and tools. Agent policy
  governs those. To place work on runners that have something, give them a
  label, such as `agents=claude`.
- **Stale runners.** Nothing revokes them automatically. The minimum version
  covers stale code, and retention forgets runners that went away.
- **Health checks.** A refused runner keeps answering HTTP, `/health`
  included; it only refuses work.
- **Housekeeping.** Background sweeps, audit deliveries and other upkeep run
  on every runner whatever its state. They are idempotent, and a compromised
  process would ignore its revocation anyway.

## What people see

| Where | What happens |
|---|---|
| Chat | On a revoked, outdated or unregistered runner the turn doesn't start, and the chat shows the refusal's message. Over the WebSocket this is a `denied` frame, `{"type": "denied", "message": "…", "code": "fleet_runner_revoked"}`, whose `code` is `fleet_runner_revoked`, `fleet_runner_outdated` or `fleet_runner_unregistered`; a REST request that writes a chat message gets 503 with `{"error": "…", "code": "…"}` and the same codes. Reloading the page reaches another server. |
| Task runs | A run no runner can take fails with the `fleet_no_runner` message. A revoked runner's run is cancelled with "The runner this task was running on was revoked from the fleet." |
| Reminders and auto projects | Work no runner can take waits, with no message. |
| Runners page | The minimum version, and each runner's name, version with an **Outdated** badge, labels, state, **Serving** or why it isn't, **Connected** or when it was last seen, its live runs, and a **This server** badge on the runner that served the page. |
| Agent policy pages | The **Runner placement** card: the required labels, how many runners can take the work, and a warning when none can; to someone who may change the labels, also the labels runners carry. With a lapsed license or a stored key that no longer verifies, the card is read-only and says the labels are still enforced. |
| Server log | A warning when a name is contested, when a runner starts in a state or version that takes no unattended work, and when a run fails for lack of a runner. A run left for another runner is logged too. |

## Audit log

Changes to the fleet are recorded with no organization, in the server scope
(see [AUDIT.md](AUDIT.md#retention)). Label changes and refused runs are
recorded in the organization's audit log. Registration and retention run on
every edition, so Community servers record `runner.registered` and
`runner.removed` in the server scope too.

| Action | Scope | When | Resource | Recorded values |
|---|---|---|---|---|
| `fleet_policy.updated` | Server | The minimum version is set, changed or cleared | `fleet_policy` | `minimum_version`, before and after |
| `runner.registered` | Server | A name registers for the first time, or again after its row was removed; recorded by Zone, with no actor | `runner`, with the runner's ID | `name`, `version` and `labels` |
| `runner.updated` | Server | A server admin changes a runner's state | `runner`, with the runner's ID | `name` and `state`, before and after |
| `runner.removed` | Server | A server admin removes a runner, or retention forgets one, with no actor | `runner`, with the runner's ID | `name`, `version`, `labels` and `state` before |
| `runner_labels.updated` | Organization | An organization's or workspace's labels are saved and changed | `organization_runner_labels` or `workspace_runner_labels`, with the organization's or workspace's ID | `labels`, before and after |
| `fleet.run_refused` | The run's organization | A task run fails because no runner can take it; recorded by Zone, with no actor | `task`, with the task's ID | `code`, `minimum` and `labels` |

Saving without a change writes no entry.

## Codes

Refusals of work. A task run a runner is refused is handed off or fails as
`fleet_no_runner`, so these messages read as a chat or an operator sees them.
`GET /api/runners` reports the state and version codes for each runner as
`code`. {version}, {minimum}, {missing} and {labels} stand for the values
involved, with labels listed comma-separated, and a part in square brackets
appears only when it applies.

| Code | Message |
|---|---|
| `fleet_runner_unregistered` | This Zone server hasn't registered as a runner in its fleet, so it takes no work until it does. |
| `fleet_runner_cordoned` | This runner is cordoned, so it takes no new unattended work. |
| `fleet_runner_revoked` | This Zone server was revoked from its fleet, so it can't run chats. Reload the page to reach another server. |
| `fleet_runner_outdated` | This Zone server runs Zone {version}, below its fleet's minimum of {minimum}, so it can't run chats. Reload the page to reach another server. |
| `fleet_labels_missing` | This runner lacks labels this work requires: {missing}. |
| `fleet_no_runner` | No runner in this server's fleet can take this run. It needs an active runner[ on Zone {minimum} or later][ labelled {labels}]. |

Refusals of a change:

| Code | Status | Meaning |
|---|---|---|
| `license_required` | 403 | This needs a Zone Enterprise license that is active or in its grace period. See [Licensing](#licensing). |
| `fleet_body_invalid` | 400 | The request body can't be read: it isn't valid JSON, lacks a key the request needs, has a key it doesn't take, or has a value of the wrong type. |
| `fleet_version_invalid` | 400 | The minimum isn't a plain `MAJOR.MINOR.PATCH`. |
| `fleet_label_invalid` | 400 | A label isn't 1 to 63 lowercase letters, digits or `.`, `_`, `-`, `=` and `/`, starting with a letter or digit. |
| `fleet_state_invalid` | 400 | The state isn't `active`, `cordoned` or `revoked`. |
| `fleet_runner_online` | 409 | The runner is connected, so it can't be removed. |
| `fleet_runner_busy` | 409 | The runner is offline but still holds live work, a running or waiting task run or an unexpired chat lease, so it can't be removed yet. |
| `fleet_no_runner_left` | 409 | The change would leave no runner able to take work. Send it again with `confirm: true` to make it anyway. |

## API

| Method and path | Who | Purpose |
|---|---|---|
| `GET /api/fleet-policy` | Server admins | The minimum version and licensing state. |
| `PATCH /api/fleet-policy` | Server admins | Set or clear the minimum version. |
| `GET /api/runners` | Server admins | Every runner, with its state and whether it is serving. |
| `PATCH /api/runners/{runner_id}` | Server admins | Change a runner's state. Answers the runner. |
| `DELETE /api/runners/{runner_id}` | Server admins | Remove a runner that isn't connected and holds no live work. Answers 204. |
| `GET /api/organizations/{org_id}/runner-labels` | [See above](#who-can-see-and-change-what) | The labels the organization requires. |
| `PATCH /api/organizations/{org_id}/runner-labels` | [See above](#who-can-see-and-change-what) | Replace them. |
| `GET /api/organizations/{org_id}/workspaces/{ws_id}/runner-labels` | [See above](#who-can-see-and-change-what) | A workspace's own labels, its organization's, and the effective set. |
| `PATCH /api/organizations/{org_id}/workspaces/{ws_id}/runner-labels` | [See above](#who-can-see-and-change-what) | Replace the workspace's own labels. |

Reading works on every edition; every change needs the license. A request
from someone who isn't a server admin to a server route gets 403 with "This
needs a server admin." A label change from someone without the permission
gets 403 naming it, such as "This needs the policies:update permission in
this organization." or "This needs the workspace_policies:update permission
in this workspace." A request
is checked in this order: membership (404), the caller's role or server admin
(403), the license (403 `license_required`), the body (400
`fleet_body_invalid`), each field (400 `fleet_version_invalid`,
`fleet_label_invalid` or `fleet_state_invalid`), then the
[lockout guard](#the-lockout-guard) (409 `fleet_no_runner_left`). An unknown
runner gets 404, and a removal of a runner that is connected or still holds
live work gets 409 `fleet_runner_online` or `fleet_runner_busy`. Request
bodies refuse keys they don't take.

### Fleet policy

```http
PATCH /api/fleet-policy
Content-Type: application/json

{"minimum_version": "1.4.0"}
```

`minimum_version` set to `null`, or left out, clears the minimum. Add
`"confirm": true` to make a change the lockout guard refused. `GET` and
`PATCH` answer with the same shape:

```json
{
  "entitled": true,
  "enforced": true,
  "edition": "enterprise",
  "status": "active",
  "editable": true,
  "minimum_version": "1.4.0",
  "updated_at": "2026-11-17T09:30:00Z"
}
```

| Field | Meaning |
|---|---|
| `entitled` | The license allows changes: Enterprise, active or in grace. |
| `enforced` | The license enforces fleet policy: Enterprise in any state, or a stored key that no longer verifies. |
| `edition` | `community`, `team` or `enterprise`. |
| `status` | `community`, `active`, `grace` or `lapsed`. |
| `editable` | The license allows changes and you may make them. |
| `minimum_version` | The minimum, or `null` when none is set. |
| `updated_at` | When the minimum was last saved, or `null` if it never was. |

### Listing runners

```http
GET /api/runners
```

The answer lists every runner, with the fleet's licensing state:

```json
{
  "editable": true,
  "enforced": true,
  "current": "3f1c2b8e-5d7a-4c19-9e0b-6a2d4f8c1e37",
  "runners": [
    {
      "id": "3f1c2b8e-5d7a-4c19-9e0b-6a2d4f8c1e37",
      "name": "zone-server-7c9d5b6f4-x2k8q",
      "version": "1.4.0",
      "labels": ["gpu", "region=eu"],
      "state": "active",
      "connected": true,
      "outdated": false,
      "serving": true,
      "code": null,
      "current": true,
      "runs": 2,
      "started_at": "2026-11-17T08:00:00Z",
      "last_seen_at": "2026-11-17T09:30:00Z",
      "state_changed_at": null
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `editable` | The license allows changes. |
| `enforced` | The license enforces fleet policy. |
| `current` | The runner this server process registered as, which served the response. |
| `runners[].state` | `active`, `cordoned` or `revoked`. |
| `runners[].connected` | Seen in the last 60 seconds. |
| `runners[].outdated` | A minimum is set and the runner is below it. |
| `runners[].serving` | Connected, and able to take unattended work under the saved policy, labels aside. |
| `runners[].code` | Why the saved policy keeps the runner from unattended work, labels aside: `fleet_runner_cordoned`, `fleet_runner_revoked` or `fleet_runner_outdated`, even while it is offline; otherwise `null`. An offline runner that is otherwise fine is not serving and has no code. |
| `runners[].current` | This is the runner that served the response. |
| `runners[].runs` | Its task runs that are running or waiting. |
| `runners[].started_at` | When the process now holding the name started. |
| `runners[].last_seen_at` | Its last registration or heartbeat. |
| `runners[].state_changed_at` | When a server admin last changed its state, or `null`. |

`serving` and `code` follow the saved policy even when the license doesn't
enforce it, so you can see what enforcement would do.

### Changing a runner

```http
PATCH /api/runners/{runner_id}
Content-Type: application/json

{"state": "cordoned"}
```

`state` is `active`, `cordoned` or `revoked`, and `confirm` works as for the
[fleet policy](#fleet-policy). The answer is the one runner, with the fields
of an entry in `runners` [above](#listing-runners):

```json
{
  "id": "3f1c2b8e-5d7a-4c19-9e0b-6a2d4f8c1e37",
  "name": "zone-server-7c9d5b6f4-x2k8q",
  "version": "1.4.0",
  "labels": ["gpu", "region=eu"],
  "state": "cordoned",
  "connected": true,
  "outdated": false,
  "serving": false,
  "code": "fleet_runner_cordoned",
  "current": true,
  "runs": 2,
  "started_at": "2026-11-17T08:00:00Z",
  "last_seen_at": "2026-11-17T09:30:00Z",
  "state_changed_at": "2026-11-17T09:31:00Z"
}
```

### Runner labels

```http
PATCH /api/organizations/{org_id}/runner-labels
Content-Type: application/json

{"labels": ["region=eu"]}
```

`labels` replaces the list, and `[]` clears it. Repeats collapse and the
list comes back sorted. For an organization:

```json
{
  "entitled": true,
  "enforced": true,
  "edition": "enterprise",
  "status": "active",
  "editable": true,
  "labels": ["region=eu"],
  "available": [
    {"label": "gpu", "runners": 1},
    {"label": "region=eu", "runners": 2}
  ],
  "matching": 2,
  "updated_at": "2026-11-17T09:30:00Z"
}
```

| Field | Meaning |
|---|---|
| `labels` | The labels the organization requires. |
| `available` | Each label carried by connected runners that aren't revoked, with how many carry it. `[]` unless you may change the labels, since on a shared server they are every organization's runners' labels. |
| `matching` | How many connected, active and current runners carry every required label. When it is 0, no active runner on the minimum version carries the labels, and unattended work waits until one does. |
| `updated_at` | When the labels were last saved, or `null` if they never were. |

The licensing fields mean what they do for the
[fleet policy](#fleet-policy), and `editable` also needs you to be
allowed to change the labels. A workspace's response has the same licensing
fields, `available`, `matching` and `updated_at` (the workspace's own), and
in place of `labels`:

| Field | Meaning |
|---|---|
| `organization` | The labels the organization requires. |
| `workspace` | The workspace's own labels. |
| `effective` | What unattended work in the workspace requires: both lists together. `matching` counts runners that carry these. |
