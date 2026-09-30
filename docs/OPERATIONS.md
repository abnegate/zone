# Operations

## Where the database lives

The `postgres` service runs `pgvector/pgvector:pg16`, whose `PGDATA` is
`/var/lib/postgresql/data`, a path the image declares as a `VOLUME`.
`docker-compose.yml` mounts the named volume `zone_postgres_data` exactly there,
so the cluster, `make backup` and `make restore` all point at the same bytes.

Until 2026-09 the compose file mounted `zone_postgres_data` one level up, at
`/var/lib/postgresql`. Docker then satisfied the image's `VOLUME` with an
anonymous volume, the cluster was written there, and `make backup` archived an
empty `postgres/` tree. An install created under that mount still has its
data in that anonymous volume.

## Moving an existing install's cluster

Do this once, with the stack stopped but its containers kept, before the
stack first starts on the new mount:

```bash
make stop                   # docker compose stop: containers and anonymous volumes stay
make migrate-pgdata         # copies the cluster into zone_postgres_data
./scripts/compose.sh up -d  # starts the stack again with its saved profiles
```

`make migrate-pgdata` runs `scripts/migrate-pgdata.sh`, which

- refuses to run while `postgres` is running,
- finds the anonymous volume through the stopped container's mounts, or, if
  the container was already removed (`make down`), through the one dangling
  anonymous volume that holds a `PG_VERSION` file (pass `ZONE_PGDATA_SOURCE`
  when there is more than one),
- refuses to overwrite `zone_postgres_data` when it already holds a cluster,
- copies with `cp -a` from an Alpine container, preserving ownership.

If `make up` already ran on the new mount, `postgres` refused to start:
the old mount left an empty `data/` directory inside `zone_postgres_data`,
and `initdb` stops at `directory "/var/lib/postgresql/data" exists but is
not empty`. Nothing was overwritten; `make stop` and run the migration, which
removes that empty directory before copying. A volume that was empty instead
holds the fresh cluster the image initialised: stop the stack, remove it
(`docker volume rm zone_postgres_data`), and run the migration; the old
anonymous volume is still there, dangling.

Once the stack is healthy and the data is back, remove the old volume with
the name the script printed: `docker volume rm <64-hex-name>`.

## Backup and restore

`make backup` archives nine named volumes into
`backups/zone_backup_<date>-<process>.tar.gz`, where `<process>` is the ID of
the shell running the backup: `zone_postgres_data` (the cluster, under
`postgres/`), `zone_valkey_data`, `zone_ollama_data`, `zone_manager_repos`,
`zone_manager_artifacts`, `zone_manager_agent_state`, `zone_prometheus_data`,
`zone_grafana_data` and `zone_traefik_letsencrypt`. It leaves out
`zone_comfyui_models` (downloaded weights, and the LoRAs Zone trains),
`zone_comfyui_output`, `zone_manager_embed_cache`, and every volume of the
`dev` profile, `zone_dev_agent_state` included. It checks that
`zone_postgres_data` holds a `PG_VERSION` first and stops with the migration
steps above when it does not; `ALLOW_EMPTY_POSTGRES=1` archives the other
volumes regardless.

A running postgres is stopped while its cluster is copied, so the archive
holds a cleanly shut down, point-in-time copy rather than files read while
Postgres wrote them. The backup first makes sure the `alpine` image it runs is
present, pulling it if needed, and creates a temporary Docker volume,
`zone_backup_stage_<date>-<process>`. It then stops postgres, allowing it 120
seconds to shut down, refuses to go on when it did not exit cleanly, copies
the cluster into that volume with `cp -a`, and starts postgres again. The
stack has no database only while the copy runs, usually seconds, but requests
that need it fail meanwhile. Postgres starts again when the copy fails or the
backup is interrupted too. If it cannot start, the backup prints the
`docker start <container>` command to run.

The archive is then written in one streaming pass, the staged copy under
`postgres/` and the other volumes read live, and the temporary volume is
removed when the backup ends, whether it succeeded or not. The staged copy
needs free disk the size of the cluster for as long as the backup runs; the
archive is compressed as it is written, so nothing else is held twice. A
postgres that was not running stays stopped and is archived in place, and
nothing is stopped or staged when the volume holds no cluster.

The live volumes may change while they are read. Files that grow or change are
archived as read, but a file deleted or truncated mid-read (a Prometheus WAL
segment removed at compaction, for instance) makes `tar` fail; the backup then
removes its partial archive and exits non-zero. Run it again.

It creates `backups/` with mode 0700 when the directory does not exist yet,
and writes each archive with mode 0600, owned by the user who ran
`make backup`. The archiving container runs as root, so the backup creates the
empty partial archive itself before the container starts and `tar` writes
into that file; on a Linux host the archive would otherwise belong to root and
be unreadable without `sudo`. The archive is written as
`backups/.zone_backup_<date>-<process>.tar.gz` and renamed once complete; if
an archive of that name already exists, the backup leaves it alone, removes
its own partial archive and exits non-zero. The process ID keeps two backups
started in the same second apart: each has its own temporary volume,
container, partial archive and archive, and removes only its own when it
ends.

A volume-level `.zone-restore-previous` directory, which only an interrupted
restore leaves behind, can hold the only intact copy of that volume while the
rest of it holds a partial extraction, and an archive leaves it out. So
`make backup` checks every volume first and, when any holds one, names it and
exits non-zero before it stops postgres or stages anything. Recover the volume
first, then back up: undo the restore by replacing the rest of the volume with
the contents of `.zone-restore-previous` and removing that directory, or
finish the restore by removing `.zone-restore-previous` and running
`make restore` again with the same archive.

`make restore BACKUP=<archive>` refuses to start while any running container
mounts one of the nine volumes: stop the stack first (`make stop`), and start
it afterwards. It lists the archive first, which also checks it is readable,
so a truncated or corrupt archive stops the restore before any volume is
touched. It then moves what each volume the archive carries holds into a
`.zone-restore-previous` directory inside that volume, a rename on the same
filesystem that copies nothing, and extracts the archive. Files written after
the backup, such as a table's `_vm` and `_fsm` forks, do not survive the
restore. Volumes the archive does not carry are left alone.

When the extraction succeeds, the restore deletes each
`.zone-restore-previous` directory. When it fails, for example because a
volume's disk fills up or a member of the archive is corrupt, the restore
deletes what it extracted and moves each volume's earlier contents back, so
every volume holds what it held before. Until the restore ends, a volume
therefore needs free space for both its earlier contents and the archived
ones. An interrupted restore can leave a `.zone-restore-previous` directory
behind if the container itself is killed: the next restore, and every
backup, refuses to start until you delete that directory (keeping what the
volume holds now) or replace the rest of the volume with its contents. Each
directory at the top of the archive (`postgres/`, `valkey/` and so on) is the
volume listed in the first paragraph. An archive that itself holds a volume-level
`.zone-restore-previous` directory is refused.

An archive with no cluster under `postgres/`, which is what every archive
taken before the mount moved looks like, leaves `zone_postgres_data` exactly
as it is, extracting nothing into it, and says so. Archives from before the
postgres pass restore the same way.

### Coding agent sign-ins in an archive

The archive includes `zone_manager_agent_state`, under `manager_agent_state/`.
It holds each organization's Claude Code and Codex state: codex's `auth.json`,
a working ChatGPT login stored in plain form, and both CLIs' session
transcripts. Anyone holding the archive can use those logins, so keep archives
private. Deleting an organization signs it out of codex and removes its
directory from the volume, but archives taken before then still hold its
login. The Claude tokens are not in that volume. They are in the database,
sealed with a key derived from `ENCRYPTION_KEY`, and a restored instance needs
the same `ENCRYPTION_KEY` to use them: under another key, every Claude Code
sign-in shows as expired until an organization admin signs in again.

## Upgrading to migration 048

The first start of a server that includes coding agent sign-ins applies
migration 048, which adds the `agent_logins` table and lets AI settings store
the `claude_code` and `codex` providers. An image built before it cannot start
against the migrated database: it stops with
`Failed to run migrations: VersionMissing(48)`. Run `make backup` before the
upgrade. Going back to an older image means restoring that backup, and losing
whatever changed after it was taken.

## Upgrading to migration 049

The first start of a server that records failed reviewer rounds applies
migrations 049 and 050, which let `task_reviews` store the `failed` verdict.
049 swaps the verdict check without reading the table, and 050 then validates
it while reviews stay writable. An image built before them cannot start
against the migrated database: it stops with
`Failed to run migrations: VersionMissing(49)`. Run `make backup` before the
upgrade. Going back to an older image means restoring that backup, and losing
whatever changed after it was taken.

An auto-project reviewer model that errors now records a failed round: the
next tick asks the next reviewer, and a second round on the same head without
a verdict pauses the task with the error, pointing at
`ZONE_AUTO_REVIEW_MODELS`. A reviewer endpoint that does not answer (a refused
connection, a timeout, a 5xx while Ollama restarts, or a 429) records nothing
and is asked again on the next tick, with a warning logged each time. After 5
unanswered attempts the task asks the next reviewer in its rotation instead,
still without recording a round, and once every reviewer has gone unanswered
at least 5 times over 10 minutes the task pauses, naming each reviewer and the
last error. Only an answer, or the task leaving review, ends a reviewer's
count; however far apart its attempts land while sibling tasks are reviewed,
they still add up. The counts live in the driver's memory, so a restart starts
them over and the task asks its round's own reviewer first again.

## Upgrading to migration 051

The first start of a server that skips repeated sync deliveries applies
migrations 051, 052 and 053. 051 adds the `sync_deliveries` table and lets
`sync_events` record the `unlink` event of a deleted GitHub issue, swapping the
event type check without reading the table; 052 then validates it while sync
events stay writable; 053 adds the `sync_unlinked_items` table that keeps a
deleted issue from becoming a task again. An image built before them cannot
start against the migrated database: it stops with
`Failed to run migrations: VersionMissing(51)`. Run `make backup` before the
upgrade. Going back to an older image means restoring that backup, and losing
whatever changed after it was taken.

## Pulling models into the bundled Ollama

With `PROFILES=bundled-ollama`, `ollama-init` pulls `OLLAMA_MODEL_FAST`,
`OLLAMA_MODEL_REASON` and `OLLAMA_MODEL_EMBED` into whatever `OLLAMA_BASE_URL`
names. It joins the `edge` network as well as `internal`, so a host Ollama at
`http://host.docker.internal:11434` is reachable. When that host does not
answer, the container says so, names the `OLLAMA_BASE_URL=http://ollama:11434`
setting that pulls into the bundled Ollama instead, and exits 0; only the
bundled Ollama being unreachable is treated as a failure.

## External sync

A project can take issues from a GitHub repository or a Linear project. Sync
is inbound only: Zone does not create or update external issues from its
tasks.

A sync acts only on issues in its configured GitHub repository or Linear
project: an event for an issue anywhere else, including an issue with the same
number in another repository that an organization-wide webhook sends, is
acknowledged and ignored. A sync with no repository or project ID set therefore
acts on nothing.

A newly opened issue becomes a non-agentic task in the project, so it never
runs an agent on its own. GitHub creates a task only for an issue whose author
is the repository's owner, a member of the organization that owns it, or a
collaborator (`author_association` `OWNER`, `MEMBER` or `COLLABORATOR`). Linear
creates one only when the issue is created in the configured project; an issue
moved into the project later does not become a task. An outbound-only sync
creates no tasks. A GitHub `opened` event for an issue opened more than 24
hours ago creates no task, so a captured delivery cannot be replayed into one
later. Titles are cut to 500 bytes and descriptions to 50,000 bytes, never
mid-character.

Later events for a linked issue update its task's title and description. The
task's status follows the issue only when the issue changes state: a GitHub
`closed` completes the task and `reopened` sets it back to created. A Linear
update moves it when the issue's state type maps to a different status than
the last update applied did: `started` is in progress, `completed` or
`canceled` is complete, and any other type is created. The first Linear update
to a task linked some other way than by a created issue only records the
state. Labels, assignments and edits leave the status alone. Deleting a
GitHub issue or removing a Linear issue unlinks it and leaves its task as it
is, and that issue never becomes a task again. The same holds for an issue in
the configured repository or project that is deleted before the event opening
it arrives: the late `opened` or `create` creates no task.

While a run owns a task's status, an event that would move it updates only the
title and description, and the state stored for the issue stays the one the
task last followed. The next later event for the issue, once the run has
ended, moves the status to match: for GitHub, a label, assignment or edit
reporting a state the task has not followed does so. A GitHub label,
assignment or edit in the same second as the last event applied cannot be
ordered against it, so it leaves the status and the stored state alone too, and
the next later event settles them.

An event that says the issue last changed before the last event applied to its
task, or in the same second with the same state, title and body, is
acknowledged and dropped, so a delivery arriving out of order cannot undo a
newer one. GitHub reports these times to the second, so two changes within one
second, such as a bot opening and closing an issue, both apply in the order
they arrive. A deletion is dropped only when it is older than the last event
applied. Deliveries for one issue apply one at a time, including the first
ones for a newly opened issue.

Each delivery's ID (`X-GitHub-Delivery` or `Linear-Delivery`) is recorded once
it is applied, and a delivery with an ID already recorded is answered 200
"Delivery already processed" without being applied again. A delivery whose
processing fails applies nothing, including its ID, so the provider's retry is
processed in full. Neither the ID nor GitHub's `X-GitHub-Event` header is
signed, so the ID only catches the provider's own retries; a signed `issues`
delivery whose body carries a `comment`, and so came from another event, is
refused with a 400.

Each sync has its own endpoint, `/api/webhooks/sync/{id}/github` or
`/api/webhooks/sync/{id}/linear`, shown as the Payload URL in the project's
sync settings. Bodies over 1 MB are refused with a 413. A delivery is refused
with the same 401 whether its signature is wrong or the sync does not exist, is
disabled, is for the other provider, or has no secret; the server log says
which. Events other than issue events are acknowledged and ignored.

For GitHub, open the repository's Settings → Webhooks and add a webhook with
that Payload URL, content type `application/json`, the secret shown once when
the sync is created, and only the "Issues" event. If the secret is lost, use
"Rotate secret" and paste the new one into GitHub. A GitHub sync created
before Zone issued secrets has none and refuses every delivery; use
"Generate secret" on it. GitHub's first `ping` is acknowledged.

For Linear, create a webhook for Issues with the same URL, then paste Linear's
signing secret into the sync with "Set signing secret". The sync's project ID
must be the Linear project's ID, a UUID. Deliveries more than 60 seconds old
are refused.

Only a workspace admin can create or delete a sync, or rotate, generate or set
its secret: whoever holds the secret can sign deliveries, replacing it stops
the live webhook verifying until the provider has the new one, creating a
GitHub sync issues a fresh one, and deleting a sync drops every item it linked.
Other members see the syncs without those controls. The UI asks before
rotating. Creating and deleting are written to the audit log as `sync.created`
and `sync.deleted`, and each secret change as `sync.webhook_secret_rotated`
(Zone generated it) or `sync.webhook_secret_set` (the admin supplied it),
never with the secret. A
change that races another is answered 409 and shows no secret, since the one it
generated was not the one kept; load the sync again and retry.

## Monitoring

On Docker Desktop, the "Container CPU" and "Container memory" panels of the
Chat Quality dashboard stay empty while "Use containerd for pulling and storing
images" (Settings > General) is on, which it is by default from Docker Desktop
4.34. The containerd image store keeps image layers where cAdvisor v0.52.1, the
version the compose file pins, does not look, so its log repeats
`failed to identify the read-write layer ID for container …` and it exports
only `machine_memory_bytes`. The upstream issue is
[google/cadvisor#3643](https://github.com/google/cadvisor/issues/3643); the fix,
[google/cadvisor#3709](https://github.com/google/cadvisor/pull/3709), first
shipped in cAdvisor v0.54.0. Until the pin moves past it, turn that setting off
to get the panels back. Docker Desktop keeps the two image stores apart and
hides the inactive one, so the classic store starts without Zone's images and
containers: the next `make up` pulls the images again and recreates the
containers. The containerd copies stay on disk and come back if the setting
is turned on again.

The Gluetun scrape target and the blackbox SearXNG probe come from DNS
discovery of the `gluetun` name, so they exist only while the `vpn` profile
runs. A stack started with `monitoring` alone shows neither and fires no
"SearXNG Probe Down" alert. Prometheus drops the targets only when the name
answers NXDOMAIN, which is what Docker's DNS returns for a stopped container.
A SERVFAIL or a timeout keeps the last targets, and their failing scrapes and
probes can still fire "SearXNG Probe Down".

A dead `gluetun` therefore raises no Gluetun or SearXNG alert: both rules
treat no data as OK. With `vpn` on, nothing else reaches you either. Grafana,
the manager, and LiteLLM share Gluetun's network namespace, and when Gluetun
stops or restarts they are left with loopback only. Prometheus still records
`up{job="manager"}` and `up{job="litellm"}` as 0, and "Manager Service Down"
and "LiteLLM API Gateway Down" still go to Alerting, because Grafana's failed
query to Prometheus counts as an error and those rules alert on errors. But
Grafana has no route out, so no email or Discord notification leaves, and the
dashboards go dark as well, since Traefik reaches Grafana through Gluetun. The
attached containers stay cut off after Gluetun comes back on its own, so run
`make restart` to rejoin them to its network.
