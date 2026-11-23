# High availability

Zone runs as several zone-server replicas on Kubernetes in one supported topology: the three `values-ha.yaml` profiles in `helm/`, layered over your own values. This page describes that topology, what it needs, what survives which failure, and where its edges are.

Every other way of running several replicas, including the default `helm/zone-apps/values.yaml`, is outside it. The topology is kept small on purpose, so that a supported install is one Zone can reason about.

## Topology

```mermaid
flowchart TD
    Browser["Browsers and agent CLIs"] --> Ingress["HAProxy Kubernetes Ingress<br/>cookie zone-server"]
    Ingress -->|"/"| Console["Console<br/>2-6 pods"]
    Ingress -->|"/api, /ws"| Server["zone-server<br/>3-10 pods, one per node first"]
    Server -->|"20 pooled + 1 leadership connection each"| Postgres[("CloudNativePG<br/>3 instances, synchronous,<br/>one per node")]
    Server -->|"bus, shared rate limits"| Valkey[("Managed Redis or Valkey<br/>with failover")]
    Server -->|"POSIX with locks"| Shared[("ReadWriteMany volume<br/>&lt;fullname&gt;-shared-state")]
    Server --> LiteLLM["LiteLLM<br/>2 pods"]
    LiteLLM --> Postgres
    Postgres -->|"WAL archive and base backups"| Objects[("Object store<br/>S3, GCS or Azure Blob")]
```

| Part | Profile | What it runs |
|---|---|---|
| zone-server and console | `helm/zone-apps/values-ha.yaml` | Server autoscaling 3-10, console 2-6, PDBs, probes, shutdown timings, the shared volume, cookie affinity |
| Postgres | `helm/zone-infra/values-ha.yaml` | CloudNativePG with 3 instances, synchronous replication, required anti-affinity, `max_connections: "400"`, optional Barman Cloud backups. No Valkey. |
| LiteLLM | `helm/zone-ai/values-ha.yaml` | 2 replicas, a PDB, preferred anti-affinity. Ollama stays at one replica, outside the topology. |

Replicas coordinate through three things:

- **Postgres** holds every piece of durable state: chats, the chat lease that says which replica runs a turn, task runs and their heartbeats, and the ledgers that stop a report or alert going out twice. One replica at a time holds the housekeeping leadership lock and runs the cluster-wide sweeps.
- **Valkey** carries the cluster bus: signed messages that relay live chat frames, chat commands, task question answers, titles and action cards between replicas, plus the shared context-search and single sign-on rate limits. Nothing in it is durable.
- **The shared volume** holds what the agent CLIs and git need on disk: sign-in homes, clones, worktrees, chat workspaces and artifacts.

## Minimum cluster

- **Kubernetes 1.27 or later.** Every topology spread constraint in the profile sets `nodeTaintsPolicy: Honor` (on by default from 1.26) and `matchLabelKeys: [pod-template-hash]` (on by default from 1.27). An older API server silently drops a field whose feature is off, so the profile still installs there but spreads badly. See [Scheduling](#scheduling).
- **3 or more schedulable nodes.** CloudNativePG's required anti-affinity puts each of the 3 Postgres instances on a node of its own, so with fewer nodes an instance stays Pending. The server alone runs on 2 nodes, but 2 nodes is not a supported topology.
- **A ReadWriteMany StorageClass with POSIX locking.** See [Shared state](#shared-state).
- **A managed Redis or Valkey with failover.** See [Valkey](#valkey).
- **Postgres 14 or later, reached directly or through a session-mode pooler.** The leadership lock is a session advisory lock on a dedicated connection that sets `idle_session_timeout`, which Postgres 14 added. A transaction-mode pooler (PgBouncer `pool_mode = transaction`, and the like) hands the session to someone else between statements and breaks it, as it does the other [session locks](#locks-across-replicas). The CloudNativePG image the chart pins is Postgres 16. An external managed Postgres through `global.externalDatabase` is supported on the same terms.
- **An object store for backups**: S3, GCS or Azure Blob Storage, through the CloudNativePG Barman Cloud plugin.
- **The CloudNativePG operator 1.30 or later**, and the HAProxy Kubernetes Ingress controller.

### Scheduling

The server's pods spread over nodes with `maxSkew: 1` and `whenUnsatisfiable: DoNotSchedule` on `kubernetes.io/hostname`, and on a best-effort basis (`ScheduleAnyway`) over `topology.kubernetes.io/zone`. Their anti-affinity on the host name is preferred, never required. The console spreads over nodes on a best-effort basis only.

- **Each rollout revision spreads on its own.** `matchLabelKeys: [pod-template-hash]` makes the spread count only the pods of the new pod's own ReplicaSet. Without it the spread also counts the old ReplicaSet's pods while they are being removed, and a rollout can end with 2 new pods on one node and none on another.
- **Cordoned and tainted nodes don't count.** `nodeTaintsPolicy: Honor` keeps a node the pod cannot tolerate, such as a tainted control plane or a node being drained, out of the skew. Without it that node counts as an empty domain, and rollouts and the HPA deadlock.
- **Releases don't count each other.** With `highAvailability.enabled` the chart adds `app.kubernetes.io/instance: <release>` to every spread and anti-affinity selector, so two releases in one namespace spread on their own.
- **Keep room for one more server pod on every node.** The server rolls with `maxSurge: 1` and `maxUnavailable: 0`, so a new pod must be Ready before an old one stops, and a surge pod that cannot be scheduled stalls the rollout. A server pod requests 500m CPU and 512Mi. Within one revision the spread always has a node for the next pod: while the revision has fewer pods than nodes, some node has none of them, and past that a skew of 1 allows the next. So the HPA can also grow past the node count.
- **The PDB allows one disruption** (`maxUnavailable: 1`), so a drain evicts server pods one at a time. The server, console and LiteLLM budgets set `unhealthyPodEvictionPolicy: AlwaysAllow`, so a drain can evict a pod that is not Ready instead of waiting out its crash loop.
- **Nothing rebalances.** After a node is lost or drained, its pods move to the other nodes and stay there when it comes back. Run a descheduler, or roll the Deployment (`kubectl rollout restart deployment/zone-apps-server` for a release named `zone-apps`), to spread them again.

**On 2 nodes** the server still rolls and drains. After a rollout no node holds more than half the replicas, rounded up. A drain cordons its node, which then leaves the skew, so every replica moves to the other node. CloudNativePG's third instance has nowhere to go, though, so 2 nodes is not a supported topology.

## Installing

```bash
kubectl create namespace zone

helm upgrade --install zone-infra helm/zone-infra --namespace zone \
  -f helm/zone-infra/values-ha.yaml \
  --set postgres.backup.objectStore=<ObjectStore name>

helm upgrade --install zone-ai helm/zone-ai --namespace zone \
  -f helm/zone-ai/values-ha.yaml

helm upgrade --install zone-apps helm/zone-apps --namespace zone \
  -f <your values> \
  -f helm/zone-apps/values-ha.yaml \
  --set server.agentState.persistence.storageClass=<ReadWriteMany class> \
  --set server.trustedProxies=<ingress controller pod CIDR> \
  --set highAvailability.externalValkey=true
```

`server.trustedProxies` lists the proxies whose `X-Forwarded-For` zone-server believes, such as `10.244.0.0/16`, the pod CIDR of kind and flannel; use your ingress controller's pod CIDR. Every request reaches the servers through the ingress controller, so without it every client shares the controller's address and its [single sign-on limit](#shared-limits). An HA render without it fails, and the message gives that example. To trust loopback only on purpose, set it to `127.0.0.1/32,::1/128` (escape the comma for `--set`: `--set 'server.trustedProxies=127.0.0.1/32\,::1/128'`). See [ZONE_TRUSTED_PROXIES](CONFIGURATION.md#zone_trusted_proxies) for what a trusted range allows.

`highAvailability.externalValkey=true` states that the existing secret's `valkey-url` key already points at the managed endpoint; use `secrets.create` with `secrets.redisUrl` instead to have the chart write it (see [Valkey](#valkey)). The secrets the charts read, and the 32-character `JWT_SECRET` and `ENCRYPTION_KEY`, are the same as for any install: see [helm/QUICK_START.md](../helm/QUICK_START.md).

Set `postgres.storage.size` and `postgres.storage.storageClass` for the Postgres instances, and `server.agentState.persistence.size` (100Gi in the profile) for the shared volume. Every CLI turn writes a transcript to the shared volume and Zone prunes none of them, so size it for growth.

## Sizing

### Postgres connections

Each server replica pools 20 connections and opens 1 more, outside the pool, for the leadership lock: 21 per replica. Migrations at startup use a pooled connection. LiteLLM pools 10 connections per worker, and the chart runs 4 workers per pod.

| Client | Pods at most | Connections |
|---|---|---|
| zone-server | 11: the HPA maximum of 10, plus one surge pod | 11 × 21 = 231 |
| LiteLLM | 3: 2 replicas, plus one surge pod | 3 × 4 workers × 10 = 120 |
| Headroom | | 49: the operator, administrators, `psql` sessions |
| **`max_connections`** | | **400** |

When you change `server.autoscaling.maxReplicas`, `litellm.replicas` or `litellm.workers`, keep `postgres.parameters.max_connections` at least

```
21 × (server maxReplicas + 1) + 10 × litellm workers × (litellm replicas + 1) + headroom
```

CloudNativePG applies a new `max_connections` by restarting its instances. With an external Postgres, set the same number at the provider. `scripts/test-helm-ha.sh` checks that the HA renders stay within it.

LiteLLM recommends one worker per pod on Kubernetes and scaling by pods instead. The chart keeps 4 workers per pod; if you move to 1 worker and more replicas, the formula above still holds.

### CPU, memory and concurrency

The HA profile keeps the server's default resources: requests of 500m CPU and 512Mi, limits of 1 CPU and 1Gi. The HPA scales on 70% CPU. The claude and codex CLIs run as child processes inside the server pod and count against its limits, so raise the memory limit for organizations on the Claude Code or Codex provider, and size it from what the pods actually use. A LiteLLM pod requests 4Gi of memory and is limited to 6Gi, because its master and each of its 4 workers is a Python process of about 1 GiB.

Several limits are per replica and grow with the replica count rather than being shared:

- Each replica runs at most 5 task runs at once (`MAX_CONCURRENT_TASKS` in `workers/task.rs`, not configurable). A cluster at its HPA minimum of 3 runs up to 15.
- Each replica runs one image generation at a time, and accepts up to 5 sockets per chat.

### Shared limits

These rate limits are counted in Valkey, across every replica, in fixed windows that every replica numbers the same way. Each counts under its own key, so one never uses up another's budget:

- the context-search rate limit, per user;
- the single sign-on limit, per client address: [`ZONE_SSO_REQUESTS_PER_MINUTE`](CONFIGURATION.md#zone_sso_requests_per_minute) (120 by default) single sign-on starts and SAML responses a minute. The address is the one zone-server resolves through `server.trustedProxies`;
- the [SCIM limits](SCIM.md#rate-limit): 600 requests a minute per organization, and 1,200 rejected SCIM tokens a minute per client address.

While Valkey is down, or does not answer within 500 milliseconds, each replica counts on its own instead, so one user, organization or address can make up to the limit times the replica count. The warning logged for an address's first refusal in a window is per replica, so a flood across 3 replicas can log up to 3; likewise each replica records an organization's first SCIM refusal in a window. Which addresses are refused for rejected SCIM tokens is also kept per replica: a replica that has not refused an address yet looks up one more token from it, which the shared count then refuses.

## What survives a failure

| Failure | What happens | What is lost |
|---|---|---|
| **A server pod is lost** (SIGKILL, OOM, a crash) | Its sockets drop and their clients reconnect to another replica. Chat turns it ran are cut. Clients on other replicas watching those turns get "The server running this reply stopped; any queued messages were dropped. Send again." once the chat's 30-second lease lapses and their 5-second watchdog sees it, and the chat then takes a new message on any replica. Task runs it ran fail once their heartbeat is 60 seconds old, and can be retried. If it held the leadership lock, the lock frees when its Postgres session ends, or after 30 seconds of silence (`idle_session_timeout`) if the connection hangs, and a new leader waits 20 seconds before its first cluster sweep. | The cut turns' unsaved output. Messages queued behind them, which lived in its memory. Agent CLI sign-ins in flight on it (see [Ingress and sign-ins](#ingress-and-sign-ins)): start them again. |
| **A server pod stops gracefully** (rollout, drain, scale-in) | Readiness turns 503 at once, and chat sockets on it start no new reply: a message that would start one is refused with an error and the socket closes with 1012 so the client reconnects elsewhere. It also gives up the leadership lock at once, so another replica takes the cluster sweeps after its 20-second wait, and claims no more reminder firings. It keeps serving for `ZONE_SHUTDOWN_DELAY_SECONDS` (10) while the Service drops it, then stops listening and gives HTTP requests in flight up to `ZONE_SHUTDOWN_GRACE_SECONDS` (30). Turns still running are then stopped the way Stop does: the reply so far is saved and the chat's lease released at once, so another replica can take the next message without waiting 30 seconds. A reminder's turn stopped this way gives its firing back, still owed and with no attempt counted, and another replica runs it again from the start on its next 10-second tick. Every socket closes with 1012. It finishes background writes, then exits. Turns are not waited for during the grace, because a coding agent reaches Zone's tools at `/mcp` through the same listener. From SIGTERM to exit takes at most delay + grace + 17 seconds: up to 10 to stop turns, 2 to close sockets and 5 for background writes. The profile's `terminationGracePeriodSeconds` of 75 covers its 10 + 30 + 17 = 57, and `scripts/test-helm-ha.sh` checks that it does. | Messages queued behind its running turns, held from the moment it began draining. Task runs it ran, as for a lost pod. Sign-ins in flight on it. A reminder firing it stopped can show twice in its chat: the prompt with the reply cut short, then the prompt again with the full reply from the replica that ran it again. |
| **A node is lost** | Every Zone pod on it is lost, as above. Kubernetes marks the node unreachable and stops sending it traffic; replacement pods are scheduled elsewhere once its pods are evicted. A Postgres instance on it is lost: if it was the primary, CloudNativePG promotes the synchronous standby. Writes continue on the 2 remaining instances, but the third cannot come back until a third node is schedulable, so a second failure would pause writes. | As for lost server pods. No committed Postgres data. |
| **The Postgres primary fails over** | 10 to 30 seconds of write errors while CloudNativePG promotes a standby and moves the `zone-postgres-rw` Service. Replicas stay Ready throughout, by design: readiness checks only whether a replica is draining, never Postgres or Valkey, or a failover would mark every replica unready at once and empty the Service. The leadership lock does not survive onto the new primary; the old leader's pings fail and it demotes, and the new leader waits 20 seconds before its first cluster sweep. | Nothing committed: with `synchronous: {method: any, number: 1, dataDurability: required}` every commit is on a standby before it returns. Requests that failed during the failover need retrying. |
| **Valkey is down** | Replicas stop hearing each other. Clients on a replica that is not running a turn stop receiving its live frames; they see the reply once it is saved and they reload. Commands sent on such a replica are not forwarded: a Send there is refused as busy while the turn runs, and Stop, approvals and queue edits there do not reach it. Answers to a task run's question posted on a replica other than the one it waits on return 404, at once while the bus is disconnected, or after the 2-second answer timeout. Titles and action cards reach only the replica that made them. The context-search, single sign-on and SCIM limits fall back to each replica's own count ([Shared limits](#shared-limits)). `zone_cluster_bus_connected` reads 0, and every replica logs an error that the cluster bus is not connected. Each replica reconnects and resubscribes on its own once Valkey is back. | Nothing persisted. Live frames and commands sent while it was down. |
| **A LiteLLM pod is lost** | Requests in flight on it fail, and the other replica serves the rest. | Spend logs it had not flushed yet, up to one flush interval. |
| **The shared volume is lost** | Everything that touches it fails: agent turns, clones, worktrees, chat workspaces and artifacts. Postgres is intact, so chats, tasks and Claude sign-ins (sealed in the database) remain. | Codex logins, CLI session files, clones and any uncommitted work in them, chat workspaces and artifacts. Restore it from a snapshot ([Backup and restore](#backup-and-restore)). Codex sign-ins with no home sign in again, and a chat whose session file is missing starts a fresh session with its whole transcript. |
| **An availability zone is lost** | The server spreads across zones on a best-effort basis (`topology.kubernetes.io/zone`, `ScheduleAnyway`), but the profile's Postgres anti-affinity is per node, so all 3 instances can sit in one zone. To survive a zone, run nodes in at least 3 zones, set `postgres.affinity.topologyKey: topology.kubernetes.io/zone`, and use a regional ReadWriteMany backend and a multi-zone Valkey. Otherwise a zone loss is a loss of everything in it. | Depends on placement, as above. |

Notifications are sent at most once across leader changes. A scheduled report is claimed in `report_deliveries` before it is sent, and a regression alert in `regression_alerts`, so a sweep that a demoted leader cut short and the next leader repeated sends nothing twice. A crash between the claim and the send loses that one digest rather than sending it twice. Every other cluster sweep recomputes what is due from stored state, so a repeated pass is harmless.

## Backup and restore

### Postgres

The profile backs Postgres up through the [CloudNativePG Barman Cloud plugin](https://cloudnative-pg.io/plugin-barman-cloud/), which archives WAL continuously and takes scheduled base backups, so you can recover to any point in time the archive covers.

1. Install the plugin as its documentation describes.
2. Create a `barmancloud.cnpg.io/v1` `ObjectStore` in the `zone` namespace that points at your bucket. The chart does not create it, because its credentials and endpoint are yours:

   ```yaml
   apiVersion: barmancloud.cnpg.io/v1
   kind: ObjectStore
   metadata:
     name: zone-backups
     namespace: zone
   spec:
     configuration:
       destinationPath: s3://<bucket>/zone/
       s3Credentials:
         accessKeyId:
           name: <secret>
           key: ACCESS_KEY_ID
         secretAccessKey:
           name: <secret>
           key: ACCESS_SECRET_KEY
       wal:
         compression: gzip
   ```

3. Set `postgres.backup.objectStore` to its name. The `zone-postgres` Cluster then archives WAL through the plugin, and a `ScheduledBackup` named `zone-postgres` takes a base backup on `postgres.backup.schedule` (`"0 0 0 * * *"`, daily at midnight; the schedule has a leading seconds field) from a standby (`postgres.backup.target: prefer-standby`). Retention is set on the ObjectStore.

### The shared volume

Back the shared volume up with your storage backend: a CSI `VolumeSnapshot` of `<fullname>-shared-state` where its driver supports snapshots, or the provider's own backup or snapshot service otherwise. Snapshots taken while Zone runs are crash-consistent. Take them at least as often as you would accept losing codex logins, clones and artifacts.

Anyone holding a snapshot can use the codex logins in it, so protect snapshots like the secrets below. See [Coding agent sign-ins in an archive](OPERATIONS.md#coding-agent-sign-ins-in-an-archive) for the layout and what each part holds.

Valkey holds nothing durable and needs no backup.

### Secrets

Keep these outside the cluster. Without them a restored database is unreadable or unusable:

- `ENCRYPTION_KEY`: Claude sign-ins, MCP server secrets, SSO client secrets and other credentials are sealed with keys derived from it, and every replica derives the cluster bus's signing key and namespace from it. A restore under another key shows every Claude Code sign-in as expired and leaves sealed values unopenable.
- `JWT_SECRET`: under another value, every session signs out.
- The database credentials in the secret CloudNativePG bootstraps from, and the superuser secret if you created one.
- The license key. Zone keeps the installed license in the database, so a restore brings it back, but keep the key string from the license email so you can paste it into a fresh install.

### Restore order

1. Recreate the secrets above in the namespace.
2. Stop the servers so nothing writes while Postgres is replaced: upgrade zone-apps with `--set server.autoscaling.enabled=false --set server.replicaCount=0`.
3. Restore Postgres. The chart's Cluster bootstraps with `initdb` only, so restore by hand:
   - Create a new ObjectStore for the restored cluster to archive into. The old archive belongs to the cluster it came from, and archiving into it again is refused.
   - Delete the `zone-postgres` Cluster. CloudNativePG deletes its volumes with it.
   - Render the chart's Cluster with `helm template zone-infra helm/zone-infra --namespace zone -f helm/zone-infra/values-ha.yaml --set postgres.backup.objectStore=<new ObjectStore> --show-only templates/postgres-cluster.yaml`. Replace `bootstrap.initdb` with a recovery from the old archive, and add Helm's ownership metadata so the next `helm upgrade` adopts it: the label `app.kubernetes.io/managed-by: Helm` and the annotations `meta.helm.sh/release-name: zone-infra` and `meta.helm.sh/release-namespace: zone`.

     ```yaml
     bootstrap:
       recovery:
         source: origin
         recoveryTarget:
           targetTime: "2026-11-20 09:30:00+00"
     externalClusters:
       - name: origin
         plugin:
           name: barman-cloud.cloudnative-pg.io
           parameters:
             barmanObjectName: <old ObjectStore>
             serverName: zone-postgres
     ```

     Leave out `recoveryTarget` to replay to the end of the archive.
   - Apply it and wait for the Cluster to report healthy, then `helm upgrade zone-infra` with the new `postgres.backup.objectStore`.
4. Restore the shared volume from the snapshot nearest before the Postgres recovery point, into a claim named `<fullname>-shared-state`. The database decides which sign-ins exist: a home whose sign-in the database does not hold is never used, and a sign-in whose home is missing signs in again.
5. Start the servers again with your normal zone-apps upgrade. They run any migrations at startup, and the startup probe covers them.

## Rolling upgrades

- **Migrations run at server startup**, not as a job (`migration.enabled` is refused). Every replica that starts tries to migrate, under a Postgres advisory lock held on one dedicated connection, so one applies them and the others wait, find nothing to do and carry on. `zone-server --migrate-only` uses the same lock if you would rather migrate before rolling.
- **The startup probe covers them.** `/health` with a period of 10 and a failure threshold of 60 gives a starting pod 10 minutes before liveness or readiness are checked at all, so a long migration is not killed half way.
- **One pod at a time.** The server rolls with `maxSurge: 1` and `maxUnavailable: 0`: a new pod has to become Ready before an old one is told to stop, and the old one drains as [described above](#what-survives-a-failure). The PDBs allow one disruption at a time. See [Scheduling](#scheduling) for the room a rollout needs.
- **Every new pod is a new runner.** Each pod registers in the runner fleet under its own name, so a rollout registers new names and leaves the old ones offline. See [Runner fleet](#runner-fleet).
- **Old and new code run side by side** for the length of a rollout, against the new schema. Schema changes are therefore expand and contract across releases: a release adds what the new code needs without breaking the old code, and only a later release removes what the old code used. Never skip a release that does an expand step on the way to the one that contracts it.

## Locks across replicas

Work that only one replica may do at a time takes a Postgres advisory lock, so it holds across replicas as it does within one. Each fixed key is a 64-bit constant that spells `zone` and four letters in hex, and the unit test `every_advisory_lock_key_is_distinct` fails if two of them match, since the holders of one key would block each other.

| Key | Value | Scope | Held by |
|---|---|---|---|
| `LEADER_KEY` | `0x7A6F_6E65_4C45_4144` | Session, on a dedicated connection | The housekeeping leader, for as long as it leads |
| `SWEEP_LOCK` | `0x7A6F_6E65_5245_544E` | Session | The audit retention sweep |
| `REBASE_LOCK` | `0x7A6F_6E65_4155_4454` | Session | An audit position rebase; each retention batch also holds it shared, within its transaction |
| `SPEND_LOCK` | `0x7A6F_6E65_5350_4E44` | Session | Spend ingestion from LiteLLM |
| `NOTICE_LOCK` | `0x7A6F_6E65_4255_4447` | Session | Settling budgets and sending their notices |
| `CLAIM_LOCK` | `0x7A6F_6E65_5455_524E` | Transaction | Claiming a reminder turn |
| `SEATS_LOCK` | `0x7A6F_6E65_5345_4154` | Transaction | Every capped admission, and every sign-up that finds the server empty |

`SEATS_LOCK` keeps the Community member cap exact however many replicas run. Every path that adds a person on a capped server (`services::members::admit` and `reactivate`) takes it with `pg_advisory_xact_lock` in the transaction that inserts the membership, counts the active members, and holds it until that transaction ends. Two people joining on different replicas therefore can't both take the last seat. A sign-up that finds no accounts takes it too, capped or not, and counts again, so of the sign-ups racing on an empty server only one becomes its admin. An admission waits up to 5 seconds for the lock, then answers 503 `member_admission_busy` and changes nothing.

Ordering a workspace's projects, delivering a sync's updates for one external issue, and saving a learned knowledge entry take transaction-scoped locks on keys hashed from what they protect. Migrations take sqlx's own session lock, as [above](#rolling-upgrades).

A transaction-scoped lock works through any connection. A session lock needs the session to stay on one server connection, which is why a transaction-mode pooler is not supported (see [Minimum cluster](#minimum-cluster)).

## Shared state

The HA profile puts four directories on one ReadWriteMany volume, the claim `<fullname>-shared-state` (`zone-apps-shared-state` for a release named `zone-apps`), mounted by every replica through subPaths:

| subPath | Mounted at | What it holds | Why it is on the volume |
|---|---|---|---|
| `agent-state` | `/app/agent-state` (`ZONE_AGENT_STATE_DIR`) | Each sign-in's CLI home: codex's login and both CLIs' session files | The CLIs read and write their homes as files, with locks. A turn can run on any replica. |
| `clones` | `/app/clones` (`ZONE_PROJECT_CLONE_DIR`) | Project clones and task worktrees | git needs a POSIX filesystem with working locks; Zone takes `flock` on clones and worktrees, and those locks work across pods only on a shared filesystem that honours them. |
| `workspace` | `/app/workspace` (`ZONE_CHAT_AGENT_CWD`) | Chat working directories | A chat's next turn can run on another replica and must find its files. |
| `artifacts` | `/app/artifacts` | Generated artifacts | A choice, not a need. Object storage would work, but moving artifacts there is a change beyond HA, so they stay beside the rest. |

The first three need POSIX semantics with locks, so object storage cannot replace the volume.

**Supported backends**: NFSv4.1 or later, Amazon EFS, Google Filestore, Azure Files over NFS, CephFS. **Not supported**: SMB (including Azure Files over SMB), and NFSv3 without a lock manager (`lockd`), where `flock` either fails or silently locks only the local node.

The chart refuses a ReadWriteMany claim with no `server.agentState.persistence.storageClass`.

### Moving from a single-node install

A single-replica install keeps agent state on a ReadWriteOnce claim, `<fullname>-agent-state`, with agent state at its root and clones under `clones/`. The HA profile uses a new claim, because a claim's access modes cannot change, and does not copy anything into it. The old claim is kept (`helm.sh/resource-policy: keep`). Artifacts and the chat workspace were emptyDirs on a single node, so there is nothing of theirs to carry over.

To carry codex logins, session files and clones over, for a release named `zone-apps`:

1. Upgrade to the HA profile with the servers at 0, so the new claim exists and nothing writes to either:

   ```bash
   helm upgrade zone-apps helm/zone-apps --namespace zone -f <your values> \
     -f helm/zone-apps/values-ha.yaml \
     --set server.agentState.persistence.storageClass=<ReadWriteMany class> \
     --set server.trustedProxies=<ingress controller pod CIDR> \
     --set highAvailability.externalValkey=true \
     --set server.autoscaling.enabled=false --set server.replicaCount=0
   ```

2. Copy with a pod that mounts both claims as the server's user:

   ```yaml
   apiVersion: v1
   kind: Pod
   metadata:
     name: zone-state-copy
     namespace: zone
   spec:
     restartPolicy: Never
     securityContext:
       runAsUser: 1000
       runAsGroup: 3000
       fsGroup: 2000
     containers:
       - name: copy
         image: alpine:3.20
         command:
           - sh
           - -euc
           - |
             mkdir -p /new/agent-state /new/clones
             cp -a /old/clones/. /new/clones/
             find /old -mindepth 1 -maxdepth 1 ! -name clones ! -name lost+found \
               -exec cp -a {} /new/agent-state/ \;
         volumeMounts:
           - name: old
             mountPath: /old
           - name: new
             mountPath: /new
     volumes:
       - name: old
         persistentVolumeClaim:
           claimName: zone-apps-agent-state
       - name: new
         persistentVolumeClaim:
           claimName: zone-apps-shared-state
   ```

   `kubectl apply` it, wait for it to complete, check its log, then `kubectl delete pod zone-state-copy`.
3. Upgrade again without the last two `--set` flags. Once the servers run, delete the old claim: `kubectl delete pvc zone-apps-agent-state`.

## Valkey

The profile needs an external, managed Redis or Valkey with automatic failover, such as Amazon ElastiCache, Google Memorystore or Azure Cache for Redis. It must expose one primary endpoint: Sentinel is not supported, and neither is cluster mode. zone-infra's HA profile sets `valkey.enabled: false`, so no in-cluster Valkey is deployed.

The server reads `REDIS_URL` only from the secret, under the key `secrets.keys.redisUrl` (default `valkey-url`). There are two ways to set it, and an HA render fails unless one holds:

- **The chart writes the secret.** Set `secrets.create: true` and `secrets.redisUrl` to the endpoint, `redis://` or `rediss://` (TLS). It takes precedence over `commonEnv.REDIS_URL` and the URL built from `VALKEY_HOST`. The render fails when the URL names the in-cluster `valkey` service (`valkey`, `valkey.<namespace>`, `valkey.<namespace>.svc` or `valkey.<namespace>.svc.cluster.local`) or uses another scheme.
- **You manage the secret.** Point the `valkey-url` key of `secrets.existingSecret` at the endpoint yourself, and set `highAvailability.externalValkey: true` to say so. `helm template` cannot read an existing secret, so the chart cannot check this; the setting is your statement that it is true.

Either way, check the result after installing: every server pod's `/metrics` should report `zone_cluster_bus_connected 1`. While `REDIS_URL` is set but the bus is disconnected, or could not use the URL at all, it reads 0 and the server logs an error, at most once a minute. Alert on it. A wrong URL shows up there, not as a failure to start: the server starts with Valkey down.

Every replica of one install must share `ENCRYPTION_KEY`: the bus signs every message with a key derived from it and names its channels and keys under a namespace derived from it. Installs with different keys can share one Valkey without hearing each other.

With `networkPolicy.enabled`, the server's egress normally admits only the in-cluster Valkey pods. In HA it admits `networkPolicy.externalValkeyCIDR` on `networkPolicy.externalValkeyPort` instead, and the render fails when either is missing.

## LiteLLM

Zone issues LiteLLM keys with no budgets and no rpm or tpm limits. The LiteLLM replicas therefore share nothing but the database and need no LiteLLM Redis, although LiteLLM's own guidance is to add Redis once there is more than one proxy: the features that need it are ones Zone does not use. Do not add budgets or rate limits to Zone's keys by hand.

The profile runs 2 replicas with preferred anti-affinity and a PDB allowing one disruption. A replica that is killed loses the spend logs it had not flushed yet, up to one flush interval. On worker counts, see [Sizing](#postgres-connections).

## Ingress and sign-ins

The profile sets `haproxy.org/cookie-persistence: zone-server` on the server Service, so the HAProxy Kubernetes Ingress controller pins each browser to one server replica with a cookie named `zone-server`. Chats do not need it; they work across replicas through the bus. Two things do:

- **Agent CLI sign-ins in flight live in one replica's memory**: a Claude sign-in waiting for its pasted code, a codex device sign-in in progress, and its receipt. A sign-in that starts on one replica cannot finish on another, and one in flight on a replica that dies is lost: start it again.
- **The lock that orders an organization's sign-in changes is per process.** Two admins starting a codex sign-in for the same organization at the same moment, from browsers pinned to different replicas, can race. Affinity makes this rare; if it happens, sign in again.

The supported ingress is the HAProxy Kubernetes Ingress controller, which reads the annotation from the Service.

## Runner fleet

Every server pod registers as a runner in the [runner fleet](FLEET.md), named after its pod: the HA render sets [`ZONE_RUNNER_NAME`](CONFIGURATION.md#zone_runner_name) from the pod's `metadata.name`. Two live processes sharing a name share its state, and under a Zone Enterprise license only the newer one takes work. So the chart refuses a render that sets `ZONE_RUNNER_NAME` in `server.env` or `commonEnv` with `highAvailability.enabled`, with `server.autoscaling.enabled`, or with `server.replicaCount` above 1.

- **A rollout registers new runners.** A Deployment's new pods get new names, so a cordon or revoke on an old pod's name does not carry over to them ([Names carry state](FLEET.md#names-carry-state)). A container that restarts inside the same pod keeps its name, and its state.
- **Old names stay listed for 7 days.** The Runners page shows each replaced pod's name as offline. Once a day, retention forgets runners unseen for 7 days that hold no live work, unless they are revoked ([Removing runners and retention](FLEET.md#removing-runners-and-retention)).
- **Policy applies per replica.** Revoking one replica's runner refuses the chats that land on that replica, while the other replicas take theirs. Cordoning it stops it claiming task runs, reminder turns and auto-project drives, which the other replicas take, but it keeps serving chats. The `zone-server` cookie keeps a browser on the replica it is pinned to, so reloading does not move it: the people pinned to a revoked replica stay refused until that pod is gone, and its replacement registers under a new name, as active.

## Supported and unsupported

Supported: the three `values-ha.yaml` profiles over your own values, on a cluster that meets the [minimum](#minimum-cluster), with the server's sizing kept within the [connection formula](#postgres-connections).

Not supported in the HA topology:

- **Ollama.** It runs one replica and is outside the topology; use LiteLLM against hosted or external model endpoints.
- **ComfyUI**: media generation and training. Training-job recovery is local to one process.
- **The model-pull websocket.** A pull's progress reaches only sockets on the replica running it.
- **`ZONE_AGENT_CALLBACK`.** Its loopback sign-in reaches one pod through `kubectl port-forward`; leave it unset and paste Claude's code.
- **`server.hostRoot`.** It bind-mounts a directory of whichever node the pod lands on, so replicas on different nodes see different directories.
- **The default chart values.** `helm/zone-apps/values.yaml` runs 2 to 10 server replicas, each with its own emptyDir agent state, clones, artifacts and workspace. That is not this topology, and agent providers, clones and chat files misbehave across its replicas.
- **Fewer than 3 nodes.** The server runs on 2, but Postgres does not; see [Scheduling](#scheduling).
- **Sentinel, cluster-mode Redis, an in-cluster Valkey, a transaction-mode pooler, a ReadWriteOnce claim, SMB or NFSv3 without `lockd`.**

Known limits within it:

- **Task wait events.** An agent's `wait_for` on a task run that finishes on another replica is not woken by that replica's event; it wakes at its deadline and reads the run's state then.
- **The device connected flag.** The devices list marks a device connected when it has a socket on the replica answering the request, or was seen in the last 2 minutes. A device connected only to another replica can show as disconnected for up to 2 minutes after it was last seen.
- **Messages held after Stop.** When someone presses Stop, the messages queued behind the stopped turn are held on the replica that ran it, and the next Send releases them. Once that replica has released the chat, a Send arriving at another replica runs there, and the held messages stay where they were.
- **Per-replica caches.** An installed or replaced license reaches the other replicas within 60 seconds. A change to an organization's MCP servers reaches them the next time they list its servers or run a turn for it, and within about a minute otherwise.
- **Per-replica limits**, listed under [Sizing](#cpu-memory-and-concurrency).
- **Task stops** reach a run on another replica through its heartbeat, within 15 seconds.

## Licensing and support

The HA code is MIT, like the rest of Zone, and nothing in it is gated: any edition can run the profiles. The member cap of Community still applies however many replicas run (see [LICENSING.md](LICENSING.md#editions)), and holds exactly across them through `SEATS_LOCK` ([Locks across replicas](#locks-across-replicas)).

What Zone Enterprise adds is the support commitment: Zone supports exactly the topology on this page, at the versions and limits stated here. That boundary is:

- **Inside it**: the three `values-ha.yaml` profiles layered over your values; changes to hosts, TLS, image references, secrets, resources, storage sizes and classes, the backup object store and schedule, network policy CIDRs, and replica counts that stay within the connection formula; an external managed Postgres or Valkey that meets the requirements above.
- **Outside it**: anything listed under [Supported and unsupported](#supported-and-unsupported); changes to the profile's probes, shutdown timings, rollout strategy, PDBs, spread constraints or anti-affinity; another ingress controller; another storage backend; running several replicas from the default values or from Compose.

A problem on an install outside the boundary is supported once it reproduces on one inside it.

## Testing

### CI

The failover coverage runs on every pull request, in `.github/workflows/rust-test.yml`, with a real Postgres and Valkey. The binary-level tests start several real `zone-server` processes on one database, one Valkey and one shared agent-state directory, as the pods of one deployment do (`runner/zone_server/tests/integration/common/replica.rs`), then kill or stop them:

| Tests | What they pin |
|---|---|
| `high_availability_chat` | A turn on one replica reaches a socket on another byte for byte; joining mid-turn resumes with no gap or duplicate; cancel and send on another replica reach the turn; killing a replica with SIGKILL mid-turn tells the other replica's sockets and frees the chat; a turn that finished unseen closes without an error; joining an idle chat gets `init` at once. |
| `high_availability_shutdown` | After SIGTERM readiness fails while `/health` passes, requests in flight finish and the process exits within its delay and grace; a stopped replica closes its sockets with 1012 and frees the chat at once; a stopped reminder turn leaves its firing owed and unclaimed, the replica claims no firing that falls due after SIGTERM, and another replica runs both; readiness stays ready while the database is unreachable; default settings exit promptly. |
| `high_availability_updates` | Titles and action cards made on one replica reach sockets on another. |
| `housekeeping_election` | Cluster sweeps run on one replica and replica sweeps on both; leadership moves without overlap when the leader's session is terminated, partitioned or paused; a leader that starts draining stops leading at once and another replica takes the lock; a digest goes out once per slot and a regression is alerted once across leader changes. |
| `task_relay` | An answer posted on one replica resolves a run waiting on another; titles and action cards cross once and are never relayed back. |
| `shared_limiter` | Two replicas sharing Valkey together allow exactly the limit per window, and fall back to their own counts without it. |
| `shared_sso_throttle` | Two replicas sharing Valkey together allow one client address exactly `ZONE_SSO_REQUESTS_PER_MINUTE` single sign-on requests per window, refuse the next on either with 429 and `Retry-After`, still allow another address, and limit on their own without Valkey. |
| `high_availability_fleet` | Each replica registers as a runner of its own, under the name it was given, as each pod does. |
| `cluster_bus` | Delivery between peers, ordering, request and reply, isolation by master key, resubscribing after a killed subscription, and starting with Valkey down with the gauge at 0. |

To run them locally, start a Postgres with pgvector and a Valkey, apply migrations from `runner/zone_server` with `sqlx migrate run`, then from `runner`:

```bash
export DATABASE_URL=postgres://postgres:postgres@127.0.0.1:<port>/zone_test
export TEST_DATABASE_URL=$DATABASE_URL REDIS_URL=redis://127.0.0.1:<port>
cargo test -p zone_server --test integration -- \
  high_availability housekeeping_election task_relay shared_limiter \
  shared_sso_throttle cluster_bus
```

`scripts/test-helm-ha.sh`, run by `.github/workflows/helm.yml`, checks the profiles' renders: the properties on this page, that the guards refuse what they should, and that the default renders are unchanged.

### Smoke test on kind

`scripts/ha-smoke.sh` installs the three profiles on a throwaway kind cluster named `zone-ha-smoke-<random>` and deletes it when it exits, whether it passed or not. It never touches another cluster.

```bash
scripts/ha-smoke.sh --dry-run                       # render and validate only; needs no cluster
scripts/ha-smoke.sh --build --keep-images           # build the images, run on 3 workers, keep the images
scripts/ha-smoke.sh --workers 2 --server-image <ref> --console-image <ref>
```

| Flag | Effect |
| --- | --- |
| `--dry-run` | Renders zone-infra, zone-ai and zone-apps with their `values-ha.yaml` and the kind overrides, validates them with kubeconform when it is installed, and prints the plan. Creates nothing. |
| `--build` | Builds the server image (the Dockerfile's builder and runtime-base stages with `zone-server` and its entrypoint, without the agent toolchains) and the console image from this checkout. |
| `--server-image`, `--console-image` | Use these images instead of building them. Images published before `/health/ready` existed cannot pass, so a run needs `--build` or both. |
| `--keep-images` | Keeps the server and console images `--build` made and prints the command that reruns with them. |
| `--workers N` | Runs N kind workers, 3 by default and at least 2. |

It needs Docker, kind, kubectl, Helm and openssl, and about 30 GB free in the Docker VM for `--build`.

What it sets up:

- One control plane and N workers from `k8s/kind-ha-config.yaml`, with no host ports; it reaches the cluster through the API server.
- A temporary host directory mounted at `/shared` on every node, behind a hostPath ReadWriteMany volume, as the agent-state volume.
- CloudNativePG 1.30.0, and a single throwaway Valkey standing in for a managed one.
- The three charts with `values-ha.yaml` and overrides for generated secrets, small resources, the shared storage class and `server.trustedProxies` set to the kind pod subnet, `10.244.0.0/16`.
- With fewer than 3 workers, Postgres runs N instances with `postgres.synchronous.dataDurability: preferred`. Each instance needs a node of its own, and under `required` a primary left without a standby stops committing. Two nodes are tested for the server but are not a supported topology.

What it checks, in order. A failure is recorded and the run continues where it can; it exits non-zero if anything failed.

1. The Postgres cluster reaches N Ready instances (at most 3).
2. 3 server pods are Ready with at most ceil(3 / N) on any node: distinct nodes on 3 workers, at most 2 per node on 2. LiteLLM's rollout completes.
3. Every server pod reads `zone_cluster_bus_connected 1` on `/metrics`, and reaches Postgres through `/api/auth/sso/providers`.
4. The PodDisruptionBudgets exist: `zone-apps-server`, `zone-apps-manager`, `litellm`, `zone-postgres-primary`, and `zone-postgres`, which CloudNativePG creates only from 3 instances. The server's budget allows exactly 1 disruption.
5. Four disruptions, each while `/health/ready` is polled through the Service every 0.5 seconds:
   - **Pod deletion**: deletes a server pod, waits for 3 Ready pods and checks the spread again. A poll may land on the deleted pod while it drains, but none may find no Ready server.
   - **Rollout restart**: `kubectl rollout restart` must finish within 600 seconds, then the spread is checked again, with the same tolerance.
   - **Database failover**: promotes a standby with the cnpg kubectl plugin when it is installed, or else deletes the primary. Every server pod must reach the new primary within 60 seconds, no server pod may lose readiness or restart, and no poll may fail.
   - **Node drain**: drains a worker that runs a server pod but not the Postgres primary with `kubectl drain --ignore-daemonsets --delete-emptydir-data --timeout=300s`. The drain must finish, 3 server pods must be Ready with none left on that node, and no poll may fail. It then uncordons the node and waits for the Postgres cluster to return to N Ready instances.
6. Every server pod reads `zone_cluster_bus_connected 1` again.

It prints each disruption's poll counts, and on failure the pods, recent events and server logs. On exit it uncordons a node it drained, deletes the cluster and the temporary directory, and removes the images it built unless `--keep-images` was given.
