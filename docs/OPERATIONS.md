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

`make backup` archives every named volume into
`backups/zone_backup_<date>.tar.gz`, with the cluster under `postgres/`. It
checks that `zone_postgres_data` holds a `PG_VERSION` first and stops with the
migration steps above when it does not; `ALLOW_EMPTY_POSTGRES=1` archives the
other volumes regardless.

`make restore BACKUP=<archive>` extracts into the same volumes and warns when
the archive carried no cluster, which is what every archive taken before the
mount moved looks like. Restore with the stack down and start it afterwards.

## Pulling models into the bundled Ollama

With `PROFILES=bundled-ollama`, `ollama-init` pulls `OLLAMA_MODEL_FAST`,
`OLLAMA_MODEL_REASON` and `OLLAMA_MODEL_EMBED` into whatever `OLLAMA_BASE_URL`
names. It joins the `edge` network as well as `internal`, so a host Ollama at
`http://host.docker.internal:11434` is reachable. When that host does not
answer, the container says so, names the `OLLAMA_BASE_URL=http://ollama:11434`
setting that pulls into the bundled Ollama instead, and exits 0; only the
bundled Ollama being unreachable is treated as a failure.
