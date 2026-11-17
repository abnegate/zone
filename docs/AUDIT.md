# Audit export, streaming and retention

Zone records security-relevant actions in an audit log: sign-ins, membership
and role changes, settings, licenses, SSO, agent policy and more. With a Zone
Team or Enterprise license you can also:

- download the audit log for a date range as NDJSON or CSV;
- stream every event, at least once, to an HMAC-signed webhook, an
  S3-compatible bucket or a syslog server (Enterprise);
- delete events older than a retention period, once every enabled destination
  has delivered them (Enterprise).

Each organization has its own destinations, retention period and downloads,
managed by its owners and admins under **Settings → Organization → Audit
export**. Events that belong to no organization, such as sign-ins, license
changes and SSO changes, form the server scope, which server admins manage
under **Settings → This server → Audit export**.

The audit log itself is in every edition. Community keeps **Settings →
Organization → Audit log** and its **Export CSV** button
(`GET /api/organizations/{org_id}/audit-logs/export`, a CSV of up to 10,000
rows), unchanged and with no license.

## Licensing

| License | Download | Streaming | Retention period | Changing destinations or retention |
|---|---|---|---|---|
| None (Community) | Refused | Nothing is sent | Not applied: events are kept forever | Refused, except [disabling and deleting](#what-needs-a-license) |
| Team, active or in grace | Allowed | Paused (`edition`) | Applied, if one was set | Refused, except disabling and deleting |
| Enterprise, active or in grace | Allowed | Running | Applied | Allowed |
| Team or Enterprise, lapsed (past grace) | Refused | Paused (`lapsed`; Team reports `edition`), backlog kept | Still applied | Refused, except disabling and deleting |
| Stored key that no longer verifies | Refused | Paused (`unverified`), backlog kept | Still applied | Refused, except disabling and deleting |

Install the license under **Settings → This server → License**. Grace is the
30 days after the license expires.

**A lapse pauses, it doesn't lose.** While streaming is paused, every enabled
destination keeps its place in the log. Events written meanwhile wait, and
delivery resumes from where it stopped once the license is renewed. Waiting
events also hold retention back, so nothing is deleted before it's delivered.

**Retention keeps applying until the license is removed.** A period you set
keeps deleting expired events under a lapsed license, after a downgrade to
Team and under a stored key that no longer verifies, because a lapse or a
downgrade must not silently stop deleting data you committed to delete.
Removing the license returns the server to Community, which keeps every event
forever; the saved period applies again as soon as a license is installed.

### What needs a license

| Change | Needs |
|---|---|
| Download | Team or Enterprise, active or in grace, with a key that verifies |
| Add, edit or test a destination, or enable a disabled one | Enterprise, active or in grace, with a key that verifies |
| Disable a destination with a `PATCH` whose only field is `"enabled": false` | Nothing |
| Delete a destination | Nothing |
| Set, change or clear the retention period | Enterprise, active or in grace, with a key that verifies |

Without the license a change needs, it's refused with `license_required`.
Disabling and deleting stay open so that you can always stop events leaving
and remove credentials you can no longer edit. Clearing the retention period
doesn't: it loosens a compliance control, so a lapsed admin can neither clear
nor change it until the license is renewed. Without a license, the console's
**Enabled** switch can only turn a destination off, and **Delete** stays
available.

Every response carries the license as `standing`:

| Field | Meaning |
|---|---|
| `edition` | `community`, `team` or `enterprise`. |
| `status` | `community`, `active`, `grace` or `lapsed`. |
| `export` | Downloads are allowed. |
| `streaming` | Destinations can be added, changed and tested, and they deliver. |
| `retention` | The retention period can be changed. |
| `applies` | A retention period that's set is applied: a license is installed, or the stored key doesn't verify. |
| `pause` | Why delivery is paused: `unlicensed`, `edition`, `lapsed` or `unverified`; `null` while it runs. |

## Destinations

A destination receives every event of its scope written after it was added.
There's no backfill: download earlier history with an
[export](#downloading-the-audit-log). The first batch can include a few events
written just before the destination was added, and never misses one written
after.

Every destination has a name, unique within its organization or the server,
and is enabled or disabled. A disabled destination sends nothing and keeps its
place; enabling it again resumes from there, skipping anything retention
deleted in between.

Secrets (the webhook signing secret, S3 keys, syslog certificates and key) are
encrypted with the server's master key before they're stored. The API returns
only the names of the secrets that hold a value, the console never shows them
after you save, and the audit log never records them. Leave a secret field
blank when editing to keep the stored value.

### Where a destination may send

The webhook URL and the S3 endpoint follow the same rule as endpoints saved
in AI settings:

- `http` or `https`, to any host the Zone server can reach, loopback, a LAN
  address and a compose service name included;
- no user name or password, query or fragment;
- never a link-local address or a cloud metadata host or address, such as
  `169.254.169.254` or `metadata.google.internal`;
- when [`ZONE_ENDPOINT_HOSTS`](CONFIGURATION.md#zone_endpoint_hosts) is set,
  only a host it lists.

The rules apply to the host a request reaches. With `virtual_host` addressing
that's `{bucket}.{endpoint host}`, so list the bucket's host
(`zone-audit.minio.lan`) or a suffix covering it (`.minio.lan`), not the bare
endpoint host. S3 without an endpoint sends to
`{bucket}.s3.{region}.amazonaws.com`, or `s3.{region}.amazonaws.com` with
`path` addressing, which the list must then cover. `virtual_host` addressing
needs an endpoint named by host: an IP-address endpoint such as
`http://10.0.0.5:9000` is refused with `audit_destination_url_invalid`; use
`path` addressing for it.

A syslog host is a host name or IP address and follows the same metadata and
`ZONE_ENDPOINT_HOSTS` rules.

Zone checks these rules when you save, and again before every delivery. A
destination whose host an operator has since removed from
`ZONE_ENDPOINT_HOSTS` fails with `configuration` until the list allows it
again or you change the host. When sending, Zone also drops any address a
host name resolves to that's link-local or metadata, and fails with `target`
when none is left. It never uses a proxy (`HTTP_PROXY` and `HTTPS_PROXY` are
ignored) and never follows a redirect.

Plain `http` and UDP syslog are allowed, for receivers on a network you trust.
The console warns that events then travel unencrypted.

On a server shared by organizations that don't trust each other, set
`ZONE_ENDPOINT_HOSTS`: without it, an organization admin can point a
destination at any host the Zone server reaches, LAN services included.

### Webhook

| Field | Value |
|---|---|
| `url` | Where Zone sends a `POST` for each batch. |
| `signing_secret` (secret) | Required. Any text; the console's **Generate** button makes a random one. |

Each request carries up to 100 events:

```http
POST /zone-audit HTTP/1.1
Content-Type: application/json
Zone-Timestamp: 1763371800
Zone-Signature: v1=…

{"events":[{"id":"…","version":1,…}]}
```

- `Zone-Timestamp` is the time of sending, in Unix seconds.
- `Zone-Signature` is `v1=` followed by the lowercase hex HMAC-SHA256, keyed
  with the signing secret, of the timestamp, a `.`, and the raw body bytes.
- The body is `{"events":[…]}`, each element an [event](#events).
- Any `2xx` answer means delivered. Any other status, a redirect included,
  fails with `status` and is retried.
- A `429` or `503` may carry `Retry-After`, in seconds or as an HTTP date.
  Zone waits at least that long, up to 15 minutes.
- A request times out after 30 seconds, or 10 seconds to connect.
- A retry sends the same body with a new timestamp and signature.

Verify the signature against the raw body before parsing it, and refuse old
timestamps. In Python:

```python
import hashlib
import hmac
import time

TOLERANCE_SECONDS = 300


def verified(secret: bytes, timestamp: str, signature: str, body: bytes) -> bool:
    if abs(time.time() - int(timestamp)) > TOLERANCE_SECONDS:
        return False
    signed = timestamp.encode() + b"." + body
    expected = "v1=" + hmac.new(secret, signed, hashlib.sha256).hexdigest()
    return hmac.compare_digest(expected, signature)
```

In Node.js:

```js
import { createHmac, timingSafeEqual } from 'node:crypto';

const TOLERANCE_SECONDS = 300;

export function verified(secret, timestamp, signature, body) {
  if (Math.abs(Date.now() / 1000 - Number(timestamp)) > TOLERANCE_SECONDS) {
    return false;
  }
  const expected = `v1=${createHmac('sha256', secret).update(`${timestamp}.`).update(body).digest('hex')}`;
  return expected.length === signature.length && timingSafeEqual(Buffer.from(expected), Buffer.from(signature));
}
```

`body` is the request body as received, `bytes` or a `Buffer`, not the parsed
JSON. To check an implementation: the secret `whsec_test`, the timestamp
`1700000000` and the body `{"events":[]}` give
`v1=4f6f9b7a41398ad124e52a1320534f978e522923f3f2b2724e31d7a9dbf78a7f`.

### S3, MinIO and R2

| Field | Value |
|---|---|
| `bucket` | 3 to 63 lowercase letters, digits, dots and hyphens, starting and ending with a letter or digit. |
| `region` | Lowercase letters, digits and hyphens, such as `us-east-1`. Zone signs requests for this region. |
| `endpoint` | Optional. Leave it out for AWS (`https://s3.{region}.amazonaws.com`); set it for another S3-compatible store. |
| `prefix` | Optional key prefix, such as `zone/audit/`. Leading slashes are dropped and a trailing one is added. |
| `addressing` | `virtual_host` (the default) sends to `{bucket}.{endpoint host}`, and needs an endpoint named by host; `path` sends to `{endpoint}/{bucket}`. |
| `access_key_id`, `secret_access_key` (secrets) | Required. The key needs only `s3:PutObject` on the prefix. |

Zone writes one NDJSON object (`Content-Type: application/x-ndjson`) of up to
1,000 events, as soon as 1,000 are waiting or once the oldest waiting event is
60 seconds old. Each object's key is:

```
{prefix}{organization_id or server}/{YYYY}/{MM}/{DD}/{HHMMSS}Z-{first event id}.ndjson
```

The date and time are the first event's `created_at`, in UTC. Server-scope
objects use `server` in place of the organization ID. A retry writes the same
key with the same content.

Requests are `PUT`s signed with AWS Signature Version 4 (service `s3`),
carrying `x-amz-date`, and `x-amz-content-sha256` with the SHA-256 of the
body. They also carry the body's integrity checksums, `Content-MD5` and
`x-amz-checksum-sha256` (with `x-amz-sdk-checksum-algorithm: SHA256`), which
buckets with Object Lock retention require; every header is signed. Answers,
retries, `Retry-After` and timeouts work as for a webhook.

| Store | `endpoint` | `region` | `addressing` |
|---|---|---|---|
| AWS S3 | Leave it out | The bucket's region | `virtual_host`, or `path` for a bucket name with dots |
| MinIO | `http://minio:9000`, or the server's URL | The server's region, `us-east-1` unless you changed it | `path` |
| Cloudflare R2 | `https://{account_id}.r2.cloudflarestorage.com` | `auto` | `path` or `virtual_host` |

A store on your LAN can use plain `http`. A SigV4 request never carries the
secret access key, but the events and the access key ID travel unencrypted.

### Syslog

| Field | Value |
|---|---|
| `host` | Host name or IP address, with no scheme, port or path. |
| `port` | 1 to 65535. Usually 6514 for TLS and 514 for UDP. |
| `protocol` | `tls` or `udp`. |
| `facility` | 0 to 23. Defaults to 13 (log audit). |
| `hostname` | The `HOSTNAME` field of each message, 1 to 255 printable ASCII characters. Defaults to `zone`. |
| `ca_certificate` (secret) | Optional PEM. When set, Zone trusts only this CA; otherwise the operating system's trust store. TLS only. |
| `client_certificate`, `client_key` (secrets) | Optional PEM, for mutual TLS. Set both or neither. TLS only. |

Zone parses certificates and keys when you save, and refuses a bad one with
`audit_destination_certificate_invalid`. To clear an optional secret, send it
as `""`.

Each event is one RFC 5424 message:

```
<PRI>1 TIMESTAMP HOSTNAME zone - MSGID [zone@32473 id="…" organization="…" workspace="…" actor="…" actor_email="…" resource_type="…" resource="…" ip="…"] BOM{event JSON}
```

- `PRI` is facility × 8 + severity. Severity is 4 (warning) for an action
  ending `_denied`, 5 (notice) for one ending `.deleted`, `.removed` or
  `.blocked`, and 6 (informational) otherwise. With the default facility, an
  informational event has `<110>`.
- `TIMESTAMP` is the event's `created_at`. `APP-NAME` is `zone` and `PROCID`
  is `-`.
- `MSGID` is the event's `action`, keeping its printable ASCII characters, up
  to 32.
- The structured data element's SD-ID is `zone@32473`, and it lists only the
  parameters that have a value. `"`, `\` and `]` in a value are escaped with
  `\`. 32473 is the private enterprise number reserved for documentation, used
  until Zone registers its own.
- `MSG` is a UTF-8 byte order mark followed by the [event](#events) as JSON.

**TLS** uses TLS 1.2 or 1.3 and checks the server's certificate against
`host`. Each delivery opens one connection, writes up to 500 messages with
octet counting (`MSG-LEN SP SYSLOG-MSG`, RFC 6587), and closes it. Resolving,
connecting, the handshake and the write each time out after 10 seconds.

**UDP** sends one datagram per event, to the first address the host resolves
to. It's unencrypted, takes no certificates, and is best effort: an event
counts as delivered once its datagram is sent, so one lost on the way isn't
sent again. A message longer than 65,507 bytes is cut short, which leaves its
JSON incomplete, and many receivers drop or truncate datagrams far smaller
than that. Use TLS when every event must arrive whole.

## Events

Every format carries the same event, version 1. Keys always appear in this
order, and a field with no value is `null`, never left out.

| Field | Type | Meaning |
|---|---|---|
| `id` | UUID | The event's ID. The same event always has the same ID, so deduplicate on it. |
| `version` | number | The schema version, `1`. |
| `created_at` | string | When the event was recorded: RFC 3339 in UTC with microseconds, such as `2026-11-17T09:30:00.123456Z`. |
| `organization_id` | UUID or null | `null` for a server-scope event. |
| `workspace_id` | UUID or null | The workspace the action happened in. |
| `actor_id` | UUID or null | Who acted. `null` for actions Zone takes itself, and once the user is deleted. |
| `actor_email` | string or null | The actor's email when the event was recorded. |
| `action` | string | Such as `member.role_changed` or `audit_destination.created`. |
| `resource_type` | string | Such as `member` or `audit_destination`. |
| `resource_id` | UUID or null | The resource acted on. |
| `old_values` | object or null | The resource's values before the action. |
| `new_values` | object or null | Its values after the action. |
| `ip_address` | string or null | The client's address, such as `203.0.113.7`, with no prefix length. |
| `user_agent` | string or null | The client's user agent. |

```json
{"id":"0b6a5a8e-3d1f-4f0e-9a51-6f3c2f1e9c11","version":1,"created_at":"2026-11-17T09:30:00.123456Z","organization_id":"5f0c6c1e-8a43-4b8e-9d0e-2b7f4a1c3d55","workspace_id":null,"actor_id":"9d2e7f40-1c6b-4a9e-8f3d-0e5a6b7c8d91","actor_email":"admin@example.com","action":"audit_retention.updated","resource_type":"audit_retention","resource_id":"5f0c6c1e-8a43-4b8e-9d0e-2b7f4a1c3d55","old_values":{"days":null},"new_values":{"days":365},"ip_address":null,"user_agent":null}
```

Zone may add fields to version 1. Renaming or removing a field, or changing
what one means, raises `version`. Ignore fields you don't know.

## Delivery

### At least once, in order

Every event reaches every enabled destination at least once. A batch can
arrive twice: after a failure Zone couldn't tell from a success, after a
server restart mid-send, or after a [logical restore](#logical-restores).
Deduplicate on `id`.

Each event has a position: the ID of the database transaction that wrote it,
then a sequence number. A destination receives events in position order,
which is close to, but not exactly, `created_at` order. Sort by `created_at`
when you need time order.

An event is sent only once its position is final: once no transaction that
could still write an event before it is open. So a transaction that stays
open delays every destination until it ends. Any transaction that has written
something counts, in any database on the same PostgreSQL server, including a
session left idle in a transaction and an orphaned prepared transaction.

The destinations response reports this as `stalled_since`, per organization
or for the server scope: when the oldest event of that scope that an open
transaction holds back was written, once that's more than 5 minutes ago, and
`null` otherwise. To find the transaction:

```sql
SELECT pid, datname, usename, state, xact_start
FROM pg_stat_activity
WHERE backend_xid IS NOT NULL
ORDER BY xact_start;

SELECT gid, prepared, database FROM pg_prepared_xacts;
```

PostgreSQL's `idle_in_transaction_session_timeout` ends forgotten sessions on
its own.

### Batches and retries

Every 5 seconds, each Zone server process looks for destinations with events
to send, up to 8 at a time. A destination is leased in the database for 2
minutes while one process sends to it, so several Zone servers on one database
share the work without sending to one destination twice at once. Each lease
has its own token, so a send that outlives its lease can't record a result
once the destination is claimed again, even by the same process. After each
batch a destination goes behind every other one waiting, so one long backlog
doesn't hold up the rest. Retry and wait times are kept on the database's
clock, so a Zone server whose clock is off doesn't retry early.

| Kind | Batch |
|---|---|
| Webhook | Up to 100 events per request. |
| S3 | Up to 1,000 events per object, sent when full or once the oldest is 60 seconds old. |
| Syslog | Up to 500 messages per connection. |

Before a batch's first attempt, Zone stores where it ends, and every retry
sends exactly that batch: the same webhook body, the same S3 key and content,
the same syslog messages. Changing a destination's settings or secrets, or
enabling it again, drops the stored batch and retries at once.

A failed batch is retried after 5 seconds, then 10, 20 and so on, doubling up
to 15 minutes, each delay shortened at random by up to a quarter. A
`Retry-After` from a webhook or S3 store lengthens the wait, up to 15 minutes.
Zone retries forever and never skips an event. A send still running 5 seconds
before its 2-minute lease runs out is abandoned as `timeout`; one cut short by
a restart is sent again once its lease expires.

### Health

Each destination's `delivery`:

| Field | Meaning |
|---|---|
| `health` | `healthy`; `failing`, when the last attempt failed; `paused`, when the license [pauses streaming](#licensing); or `disabled`. |
| `backlog` | Events written after the last delivered one, final or not, counted up to 10,000. |
| `backlog_capped` | `true` when there are more than 10,000 of them; show the count as 10,000+. |
| `oldest_pending_at` | `created_at` of the next of them to deliver, or `null`. |
| `attempts` | Failed attempts since the last success. |
| `next_attempt_at` | When Zone tries next. `null` while paused or disabled. |
| `last_success_at` | When a batch was last delivered. |
| `last_failure_at` | When an attempt last failed. |
| `last_error_code` | The [failure code](#failure-codes) of the last failure. |
| `last_error` | What failed, in a sentence. |
| `delivered_count` | Events delivered since the destination was added. |

The last error stays after a success, as history; `health` and `attempts`
say whether it still applies.

### Failure codes

| Code | Meaning |
|---|---|
| `connect` | Zone couldn't resolve the host, connect, or finish sending. |
| `timeout` | No answer in time: 30 seconds for a webhook or S3 request, 10 seconds for each syslog step, or the send ran into the end of its 2-minute lease. |
| `tls` | The TLS handshake failed, for example on an untrusted certificate, a name mismatch or a refused client certificate. |
| `status` | A webhook or S3 store answered with a status outside `2xx`, such as `HTTP 403`. |
| `target` | The host resolves only to link-local or cloud metadata addresses. |
| `configuration` | The destination can't be used as saved: `ZONE_ENDPOINT_HOSTS` no longer allows its host, its stored settings can't be read, or its TLS settings can't build a client. |
| `unsealable` | Its secrets don't open under this server's `ENCRYPTION_KEY`. Enter them again. |

A failure message is a fixed sentence for its code, such as "Zone couldn't
connect to the destination or finish sending to it." for `connect`; a
`status` failure gives the status, such as `HTTP 403`, and a `configuration`
failure says which setting can't be used. It never names the address or port
Zone tried, the underlying network error, a response body or header, or a
secret, so failures can't be used to probe the server's network. The server
log records the cause, with the host and port, for operators.

### Testing a destination

`POST …/audit-destinations/{destination_id}/tests` sends one made-up event to
the destination at once, enabled or not, and answers:

```json
{"delivered": false, "error_code": "status", "error": "HTTP 401"}
```

The event has a new `id`, the action `audit_destination.tested`, the resource
type `audit_destination` and the destination's ID as `resource_id`. It isn't
written to the audit log, doesn't move the destination's place, and doesn't
change its health. An S3 test writes a one-event object. Testing needs the
same license as adding a destination.

## Retention

A retention period is a whole number of days, at least one. Without one,
events are kept forever. Each organization has its own, and the server scope
has one for events with no organization.

An event is deleted when both hold:

- its `created_at` is before the cutoff, now minus the period;
- every enabled destination of its scope has delivered it.

So retention waits for a destination that's behind, failing or paused by the
license, and deletes the events once they're delivered. Disabled destinations
don't hold events back: an event can be deleted before a disabled destination
gets it, and that destination skips it when enabled again.

The sweep runs hourly, as the `audit-retention` housekeeping sweep, starting
an hour after the server starts; a run missed while the server was down isn't
made up. It deletes in batches of 5,000 until nothing more is due, and records
an `audit_retention.applied` event for each scope it deleted from. It applies
a period whenever [`applies`](#licensing) is true. With several servers on one
database, one sweeps at a time: a server that finds another sweeping skips its
turn and records nothing. Like delivery, retention leaves an event alone while
a transaction that could still commit ahead of it is open, so a destination
added after a sweep began starts after every event that sweep can delete.

The retention response:

| Field | Meaning |
|---|---|
| `days` | The period, or `null` to keep forever. |
| `cutoff` | Events written before this are past the period. `null` without a period. |
| `expired` | Events past the period that are still stored, counted up to 10,000. |
| `expired_capped` | `true` when more than 10,000 are; show the count as 10,000+. |
| `held` | Of those, how many an enabled destination hasn't delivered yet, counted up to 10,000. |
| `held_capped` | `true` when more than 10,000 are held. |
| `holders` | The enabled destinations holding them, each with `destination_id`, `name` and `wait`: `backlog` (catching up), `failing` (its last attempt failed) or `paused` (the license pauses streaming). |
| `swept_at` | When the sweep last ran for this scope. |
| `deleted_count` | How many events that latest sweep deleted. The `audit_retention.applied` events record every sweep's count. |
| `updated_at` | When the period was last set. |

**The server scope** holds every event whose `organization_id` is `null`.
Server admins set its period and its destinations under **Settings → This
server → Audit export**, or through the server routes.

**Deleting an organization** deletes its destinations and its retention
period. Its events stay, with `organization_id` set to `null`, so they move to
the server scope and follow the server's period. A server destination sends
only events positioned after its own place, so it doesn't receive the
organization's earlier history.

## Downloading the audit log

```
GET /api/organizations/{org_id}/audit-exports?format=ndjson&start_date=2026-01-01&end_date=2026-03-31
GET /api/audit-exports?format=csv&start_date=2026-01-01T00:00:00Z&end_date=2026-01-31T23:59:59Z
```

| Parameter | Value |
|---|---|
| `format` | `ndjson` (the default) or `csv`. |
| `start_date` | Required. A date (`2026-01-01`, from the start of that day in UTC) or an RFC 3339 time. |
| `end_date` | Required. A date (through the end of that day in UTC) or an RFC 3339 time. Not before `start_date`. |

The range includes both ends. Events come oldest first, by `created_at` then
`id`.

- **NDJSON** (`application/x-ndjson`): one [event](#events) per line, each
  line ending in `\n`.
- **CSV** (`text/csv; charset=utf-8`, RFC 4180, CRLF line ends): a header row,
  then the columns `id,created_at,organization_id,workspace_id,actor_id,actor_email,action,resource_type,resource_id,ip_address,user_agent,old_values,new_values`.
  `old_values` and `new_values` are JSON text. A text field starting with `=`,
  `+`, `-`, `@`, a tab or a carriage return is quoted and prefixed with `'`,
  so a spreadsheet doesn't run it as a formula.

The response has `Content-Disposition: attachment;
filename="audit-{organization_id or server}-{start}-{end}.{ndjson|csv}"` and
`Cache-Control: no-store`.

Zone streams the file, reading 1,000 events at a time, so a range of any size
downloads in constant server memory. It isn't one snapshot: events that
retention deletes during a long download can be missing, and events written
during it can be included. If the server fails part way, the download stops
before the end of the range.

Every download is recorded as `audit_export.downloaded` before the first byte
is sent. If Zone can't record it, the download is refused with `500` and an
empty body.

**The console holds the whole file in browser memory** until it's saved,
because the API takes your access token in a header, not in a link. For a
large range, use curl with an access token, such as the `access_token` that
`POST /api/auth/login` returns. The token only needs to be valid when the
download starts.

```sh
curl --fail --header "Authorization: Bearer $ZONE_TOKEN" \
  --output audit.ndjson \
  "https://zone.example.com/api/organizations/$ORGANIZATION_ID/audit-exports?format=ndjson&start_date=2026-01-01&end_date=2026-12-31"
```

## Audit log

These changes are recorded in the scope they change: an organization's in its
audit log, the server's with no organization. No entry ever contains a secret.

| Action | When | Recorded values |
|---|---|---|
| `audit_destination.created` | A destination is added | `name`, `kind`, `enabled` and `settings`, plus `secrets_changed`: the names of the secrets set |
| `audit_destination.updated` | A destination is changed | The changed fields, before and after, plus `secrets_changed` when secrets were added, replaced or cleared |
| `audit_destination.deleted` | A destination is deleted | Its `name`, `kind`, `enabled` and `settings` |
| `audit_retention.updated` | The period is set, changed or cleared | `days`, before and after |
| `audit_retention.applied` | A sweep deleted events; recorded by Zone, with no actor | `days`, `cutoff` and `deleted_count` |
| `audit_export.downloaded` | Someone downloads the audit log | `format`, `start_date` and `end_date` |

Saving without a change records nothing, and neither does testing a
destination.

## API

| Method and path | Purpose |
|---|---|
| `GET /api/organizations/{org_id}/audit-destinations` | `standing`, `stalled_since` and the destinations with their health. |
| `POST /api/organizations/{org_id}/audit-destinations` | Add a destination. Answers `201`. |
| `GET /api/organizations/{org_id}/audit-destinations/{destination_id}` | One destination. |
| `PATCH /api/organizations/{org_id}/audit-destinations/{destination_id}` | Change a destination. |
| `DELETE /api/organizations/{org_id}/audit-destinations/{destination_id}` | Delete a destination. Answers `204`. |
| `POST /api/organizations/{org_id}/audit-destinations/{destination_id}/tests` | [Send a test event](#testing-a-destination). |
| `GET /api/organizations/{org_id}/audit-retention` | The retention period and what it's waiting on. |
| `PUT /api/organizations/{org_id}/audit-retention` | Set or clear the period. |
| `GET /api/organizations/{org_id}/audit-exports` | [Download](#downloading-the-audit-log) the audit log. |

Organization owners and admins can use these. A member gets 403, and someone
outside the organization 404. Server admins use the same routes without
`/organizations/{org_id}` for the server scope: `/api/audit-destinations`,
`/api/audit-destinations/{destination_id}`,
`/api/audit-destinations/{destination_id}/tests`, `/api/audit-retention` and
`/api/audit-exports`. Anyone else gets 403.

A request is checked in this order: membership (404), role (403), the license
(403 `license_required`), the body (400 `audit_body_invalid`), then each field
(400 with the field's code). A `PATCH` reads its body before the license,
because a `PATCH` that only disables needs none.

Adding a destination:

```http
POST /api/organizations/{org_id}/audit-destinations
Content-Type: application/json

{
  "name": "SIEM",
  "enabled": true,
  "settings": {"kind": "syslog", "host": "siem.example.com", "port": 6514, "protocol": "tls"},
  "secrets": {"ca_certificate": "-----BEGIN CERTIFICATE-----\n…\n-----END CERTIFICATE-----\n"}
}
```

`settings` always carries `kind`: `webhook`, `s3` or `syslog`. `enabled`
defaults to `true`. Settings of the other kinds:

```json
{"kind": "webhook", "url": "https://siem.example.com/zone-audit"}
{"kind": "s3", "bucket": "audit-logs", "region": "us-east-1", "endpoint": null, "prefix": "zone/", "addressing": "virtual_host"}
```

with the secrets `{"signing_secret": "…"}` and
`{"access_key_id": "…", "secret_access_key": "…"}`.

A `PATCH` carries only what changes:

- `name` and `enabled` replace their values;
- `settings` replaces the whole settings object, and its `kind` must stay the
  same;
- `secrets` changes only the names it lists, and `""` clears an optional one.

The list answers with this shape; the other destination routes answer with
one element of `destinations`:

```json
{
  "standing": {"edition": "enterprise", "status": "active", "export": true, "streaming": true, "retention": true, "applies": true, "pause": null},
  "stalled_since": null,
  "destinations": [
    {
      "id": "c1d2e3f4-0a1b-4c2d-8e3f-405162738495",
      "name": "SIEM",
      "kind": "webhook",
      "enabled": true,
      "settings": {"kind": "webhook", "url": "https://siem.example.com/zone-audit"},
      "secrets": ["signing_secret"],
      "delivery": {
        "health": "healthy",
        "backlog": 0,
        "backlog_capped": false,
        "oldest_pending_at": null,
        "attempts": 0,
        "next_attempt_at": "2026-11-17T09:30:05Z",
        "last_success_at": "2026-11-17T09:30:00Z",
        "last_failure_at": null,
        "last_error_code": null,
        "last_error": null,
        "delivered_count": 1204
      },
      "created_at": "2026-11-01T12:00:00Z",
      "updated_at": "2026-11-01T12:00:00Z"
    }
  ]
}
```

Setting retention:

```http
PUT /api/organizations/{org_id}/audit-retention
Content-Type: application/json

{"days": 365}
```

`{"days": null}` or `{}` clears it. `GET` and `PUT` answer:

```json
{
  "standing": {"edition": "enterprise", "status": "active", "export": true, "streaming": true, "retention": true, "applies": true, "pause": null},
  "days": 365,
  "cutoff": "2025-11-17T09:30:00Z",
  "expired": 120,
  "expired_capped": false,
  "held": 20,
  "held_capped": false,
  "holders": [{"destination_id": "c1d2e3f4-0a1b-4c2d-8e3f-405162738495", "name": "SIEM", "wait": "failing"}],
  "swept_at": "2026-11-17T09:00:00Z",
  "deleted_count": 0,
  "updated_at": "2026-11-01T12:00:00Z"
}
```

## Codes

Refusals are `{"error": "…", "code": "…"}`.

| Code | Status | Meaning |
|---|---|---|
| `license_required` | 403 | The license doesn't allow this. See [What needs a license](#what-needs-a-license). |
| `audit_body_invalid` | 400 | The body isn't valid JSON, has a field the request doesn't take, or has a value of the wrong type. |
| `audit_destination_not_found` | 404 | No such destination in this organization or the server scope. |
| `audit_destination_duplicate` | 409 | A destination with this name already exists in the scope. |
| `audit_destination_name_invalid` | 400 | The name is empty or has control characters. |
| `audit_destination_kind_invalid` | 400 | `settings` has no known `kind`, has fields of another kind, or a `PATCH` changes the kind. |
| `audit_destination_url_invalid` | 400 | The webhook URL or S3 endpoint breaks the [endpoint rule](#where-a-destination-may-send), or `virtual_host` addressing has an IP-address endpoint. |
| `audit_destination_bucket_invalid` | 400 | The bucket name isn't a valid S3 bucket name. |
| `audit_destination_region_invalid` | 400 | The region isn't lowercase letters, digits and hyphens. |
| `audit_destination_prefix_invalid` | 400 | The prefix has an empty, `.` or `..` segment or control characters, or leaves no room for the object name. |
| `audit_destination_host_invalid` | 400 | The syslog host isn't a host name or IP address, is a metadata address, or isn't allowed by `ZONE_ENDPOINT_HOSTS`. |
| `audit_destination_port_invalid` | 400 | The port isn't 1 to 65535. |
| `audit_destination_facility_invalid` | 400 | The facility isn't 0 to 23. |
| `audit_destination_hostname_invalid` | 400 | The syslog hostname isn't 1 to 255 printable ASCII characters. |
| `audit_destination_secret_invalid` | 400 | A required secret is missing, a secret isn't text, or the kind doesn't take that secret. |
| `audit_destination_certificate_invalid` | 400 | A certificate or key isn't PEM or doesn't parse, a client certificate and key don't match or aren't both set, or certificates are set for UDP. |
| `audit_retention_invalid` | 400 | `days` isn't a whole number from 1 to 2147483647. |
| `audit_export_format_invalid` | 400 | `format` isn't `ndjson` or `csv`. |
| `audit_export_range_invalid` | 400 | A date is missing or unreadable, or the start is after the end. |

## Upgrading

Migration `0027_audit_logs_positions` adds the position columns to
`audit_logs`. It rewrites the whole table and builds two indexes while holding
an `ACCESS EXCLUSIVE` lock, so nothing can read or write the audit log until
it finishes, which takes time in proportion to the table's size: seconds to
minutes at the sizes a self-hosted server reaches. If it can't take the lock
within 5 seconds, the migration fails and the server doesn't start; starting
it again retries.

Migrations 0027 to 0030 run on PostgreSQL 14 and later.

## Logical restores

A position's transaction ID comes from PostgreSQL's transaction counter. A
physical copy keeps that counter: `make backup` and `make restore`,
`pg_basebackup`, a promoted streaming replica and `pg_upgrade` all do, and
need nothing more. A logical restore starts it again lower, so new events
would sort before ones already delivered. That covers `pg_dump` with
`pg_restore` or `psql`, a dump and restore across major versions, and logical
replication to a new server.

Zone detects this and rebases every position below the new counter. It
checks at startup, before its workers start, at the start of every delivery
pass and before every retention sweep: it rebases when any stored event, a
destination's place or a stored batch is at or above the server's next
transaction ID, and otherwise does nothing.

The rebase waits for any retention batch in flight, and for any other rebase,
before it reads anything, then runs in one transaction. It sets every event's
transaction ID to 0, keeping its sequence number, and restarts each destination
just before the first event it may still owe: the first event after its old
place, or the first event written on the restored server before the rebase
ran. Zone tells those new events apart from old ones by a sequence number
above every restored event still at or above the new counter. When no
restored event is, or the sequence lags (below), it counts every event with a
transaction ID below the counter as new instead, which re-sends restored
events that were already delivered. The rebase also drops stored batches and
leases, so a send in flight when it ran can't move a destination's place.

**A rebase can deliver events twice.** Undelivered events are always sent;
some delivered ones are sent again. An S3 batch can land under a new key, so
the same events can appear in two objects. Deduplicate on `id`.

**A lagging sequence.** Logical replication doesn't copy sequences, so the
restored server can hand out sequence numbers already in use. When the
`audit_logs` sequence is behind the highest number stored, or has already
handed out a number a restored event holds, so two events share it, the rebase
renumbers every event with a transaction ID below the new counter, first moving
the sequence past the highest number stored if it's behind. That covers the
events written on the new server before the rebase, and any restored events
whose old transaction ID happens to be that low. All of those events are sent
again, in no particular order. One case goes undetected: a new event whose
number falls among the restored events' without matching any of them, which
needs every restored number in that range to be one the new server skipped
(a rolled-back write). Such an event isn't sent.

**The first start after a logical restore takes longer.** Setting every
event's transaction ID to 0 rewrites every row of `audit_logs` once, and the
startup rebase runs before the server accepts requests, so startup waits in
proportion to the table's size. If the startup rebase fails, the server logs
the error and starts, and the next delivery pass tries again; nothing is
delivered, no retention sweep runs and adding a destination is refused with
`500` until a rebase succeeds.
