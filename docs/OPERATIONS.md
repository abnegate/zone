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
first `make up` on the new mount:

```bash
make stop            # docker compose stop: containers and anonymous volumes stay
make migrate-pgdata  # copies the cluster into zone_postgres_data
make up
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
`backups/zone_backup_<date>.tar.gz`: `zone_postgres_data` (the cluster, under
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
`zone_backup_stage_<date>`. It then stops postgres, allowing it 120 seconds to
shut down, refuses to go on when it did not exit cleanly, copies the cluster
into that volume with `cp -a`, and starts postgres again. The stack has no
database only while the copy runs, usually seconds, but requests that need it
fail meanwhile. Postgres starts again when the copy fails or the backup is
interrupted too. If it cannot start, the backup prints the
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
and writes each archive with mode 0600. Docker runs the archiving container as
root, so on a Linux host whose Docker daemon runs as root the archive belongs
to root: read or copy it with `sudo`. The archive is written as
`backups/.zone_backup_<date>.tar.gz` and renamed once complete.

`make restore BACKUP=<archive>` refuses to start while any running container
mounts one of the nine volumes: stop the stack first (`make stop`), and start
it afterwards. It lists the archive first, which also checks it is readable,
and then empties each volume the archive carries before extracting into it,
so files written after the backup, such as a table's `_vm` and `_fsm` forks,
do not survive the restore. Volumes the archive does not carry are left alone.
An archive with no cluster under `postgres/`, which is what every archive
taken before the mount moved looks like, leaves `zone_postgres_data` as it
is and says so. Archives from before the postgres pass restore the same way.

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

## Pulling models into the bundled Ollama

With `PROFILES=bundled-ollama`, `ollama-init` pulls `OLLAMA_MODEL_FAST`,
`OLLAMA_MODEL_REASON` and `OLLAMA_MODEL_EMBED` into whatever `OLLAMA_BASE_URL`
names. It joins the `edge` network as well as `internal`, so a host Ollama at
`http://host.docker.internal:11434` is reachable. When that host does not
answer, the container says so, names the `OLLAMA_BASE_URL=http://ollama:11434`
setting that pulls into the bundled Ollama instead, and exits 0; only the
bundled Ollama being unreachable is treated as a failure.

## Monitoring

On Docker Desktop, the "Container CPU" and "Container memory" panels of the
Chat Quality and Zone Overview dashboards stay empty while "Use containerd for
pulling and storing images" (Settings > General) is on, which it is by default
from Docker Desktop 4.34. The containerd image store keeps image layers where
cAdvisor v0.52.1, the version the compose file pins, does not look, so its log
repeats `failed to identify the read-write layer ID for container …` and it
exports only `machine_memory_bytes`. The upstream issue is
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
