# Distributing LTS images and air-gapped bundles

This page is for whoever runs `zone-license` and the Zone registry. It covers
how LTS images and air-gapped bundles reach Zone Enterprise customers, and only
them: the registry at `registry.bitecode.ltd`, the signer's `/token` and
`/bundles` routes, the S3 bucket behind both, and how to deploy, audit, rotate
and test them. Read [LICENSING.md](LICENSING.md#deploying-the-signer) first;
this page builds on the signer described there.

- [Architecture](#architecture)
- [Why a registry, not a GHCR proxy](#why-a-registry-not-a-ghcr-proxy)
- [Who gets in](#who-gets-in)
- [The token route](#the-token-route)
- [The bundle route](#the-bundle-route)
- [Rate limits and client addresses](#rate-limits-and-client-addresses)
- [Environment](#environment)
- [Logs and pull auditing](#logs-and-pull-auditing)
- [The registry](#the-registry)
- [The token key](#the-token-key)
- [Caddy](#caddy)
- [AWS](#aws)
- [Network access](#network-access)
- [Deploying](#deploying)
- [Smoke tests](#smoke-tests)

## Architecture

```text
                      licenses.bitecode.ltd                 registry.bitecode.ltd
 customer ─────────►  Caddy ── /token, /bundles/* ──►  zone-license (signer)
   │  │                 │                                   │ presigns bundles/*
   │  │                 └── GET/HEAD /v2/* ──────────►  registry (read-only)
   │  │                                                     │ reads registry/*
   │  └──── blob and bundle downloads (presigned URLs) ───► S3 bucket
   │                                                         ▲
   └── pulls manifests through Caddy                         │ writes both prefixes
                                                  GitHub Actions (OIDC, CI only)
```

Everything public runs on the signer's EC2 host (`i-0325b72f3b2ad4f9f`, a
t3.micro in ap-southeast-2) as containers on the docker network `signer`:

- **Caddy** terminates TLS for both names and forwards only the paths listed in
  [Caddy](#caddy).
- **`zone-license`** checks the license on every `/token` and `/bundles`
  request. `/token` answers with a short-lived registry token; `/bundles`
  answers with presigned S3 URLs for the bundle files.
- **`registry`** is the CNCF distribution registry, `registry:3.1.1`, read-only,
  over the `registry/` prefix of the bucket. It trusts only tokens signed by
  the signer.
- **The S3 bucket** holds two prefixes: `registry/` (the registry's storage)
  and `bundles/` (the air-gapped bundles).

The host never handles image or bundle bytes. The registry answers a blob
`GET` with a redirect to a presigned S3 URL, and `/bundles` answers with
presigned URLs, so customers download straight from S3. The t3.micro serves
only manifests, tokens and URLs.

Nothing publishes through the host either. The LTS workflow runs its own
throwaway `registry:3.1.1` on `127.0.0.1:5000` in the CI runner, with
[`publish.yml`](../runner/zone_license/deploy/registry/publish.yml), against
the same bucket and prefix, using AWS credentials from GitHub OIDC. The token
route therefore only ever grants `pull`, and no push credential exists
anywhere.

## Why a registry, not a GHCR proxy

The other way to gate the images was to keep them private on GHCR and run a
pull-through proxy in front of them on the host. It was rejected because:

- **It needs a long-lived credential.** The proxy would hold a bot personal
  access token with `read:packages`, tied to a user account, on an
  internet-facing host.
- **Every cache miss streams through the t3.micro.** A first pull of
  `zone/server` is several gigabytes; with S3 redirects the host never carries
  those bytes.
- **A proxy has one upstream.** The distribution registry reads the same S3
  storage CI writes, so publishing, serving and the bundle all share one
  bucket.

The registry costs roughly 30–50 MB of memory, and runs with a 128 MB limit.

## Who gets in

Both routes take HTTP Basic credentials. The user name is the license ID, a
UUID in any case, and the password is the license key, trimmed. The signer
admits a request only when, in this order:

1. the key verifies against the release's keyring;
2. neither its license ID is listed in `ZONE_LICENSE_REVOKED` nor its
   subscription in `ZONE_LICENSE_REVOKED_SUBSCRIPTIONS`;
3. the user name is the key's own license ID;
4. the key's plan includes distribution, which only Enterprise does;
5. the license is active or in its grace period.

Otherwise it refuses with one of these, on both routes:

| Refusal | Status | `code` | `message` |
|---|---|---|---|
| `anonymous` | `401` | `UNAUTHORIZED` | Sign in with your license ID as the user name and your license key as the password. |
| `malformed` | `401` | `UNAUTHORIZED` | The credentials are not valid HTTP Basic credentials. |
| `unverified` | `401` | `UNAUTHORIZED` | The license key does not verify. |
| `mismatched` | `401` | `UNAUTHORIZED` | The user name is not the license key's license ID. |
| `revoked` | `401` | `UNAUTHORIZED` | This license has been revoked. |
| `ineligible` | `403` | `DENIED` | This needs a Zone Enterprise license. |
| `lapsed` | `403` | `DENIED` | This license has lapsed. Renew it to continue. |

Every `401` carries `WWW-Authenticate: Basic realm="Zone license", charset="UTF-8"`.
Every refusal's body is the registry error shape Docker clients print:

```json
{"errors":[{"code":"DENIED","message":"This needs a Zone Enterprise license."}]}
```

`malformed` covers a scheme other than `Basic` (the scheme itself is
case-insensitive), base64 that isn't standard, decoded credentials without a
`:`, and credentials that aren't UTF-8. The first `:` splits the user name from
the key. The plan is checked before the status, so a lapsed Team key is
`ineligible`, not `lapsed`.

## The token route

`GET /token?service=<service>&scope=<scope>[&scope=…]` is the Docker registry
token endpoint the registry's challenge points at. One `scope` value may hold
several scopes separated by spaces. The signer works through each request in
this order: the address limit, the license gate, the key limit, the service
check, granting, minting, then one log line.

| `outcome` | Status | When |
|---|---|---|
| `issued` | `200` | A token was minted, perhaps with empty `access` |
| `disabled` | `404` | `ZONE_REGISTRY_SERVICE` is unset |
| `unreadable` | `400` | `service` is missing, repeated or isn't `ZONE_REGISTRY_SERVICE`, or a `scope` can't be parsed. Other parameters, such as Docker's `account`, are ignored |
| a [refusal](#who-gets-in) | `401` or `403` | The gate refused |
| `limited` | `429` | A [rate limit](#rate-limits-and-client-addresses) tripped; `Retry-After` says how many seconds to wait |
| `unaddressable` | `500` | The signer can't read the peer address, so it can't apply the address limit. It mints nothing and logs at `error`; the body is `{"errors":[{"code":"UNKNOWN","message":"The client address can't be read."}]}` |

`POST /token` and `HEAD /token` answer `405` with `Allow: GET`. They mint
nothing and log no `issued` line. containerd and nerdctl POST to the realm
first, with an OAuth2 form, whenever they have credentials, and fall back to
`GET` on `405`, `404`, `401` or `400` (`405` only when there's a user name,
which a license login always has; see `doBearerAuth` in containerd's
`core/remotes/docker/authorizer.go`). That's why Caddy must pass `POST /token`
through to the signer instead of refusing it itself.

An `issued` response is:

```json
{"token":"<jwt>","access_token":"<jwt>","expires_in":300,"issued_at":"2026-11-23T04:05:06Z"}
```

It carries `Cache-Control: no-store`, as RFC 6749 section 5.1 requires of a
token response, and so does every other `GET` answer, refusals included.

The token is a compact JWS with the header
`{"alg":"EdDSA","typ":"JWT","kid":"registry-<key id>-<generation>"}` and these
claims:

| Claim | Value |
|---|---|
| `iss` | `zone-license` |
| `sub` | The license ID |
| `aud` | `ZONE_REGISTRY_SERVICE`, `registry.bitecode.ltd` |
| `iat`, `nbf` | The issue time |
| `exp` | `iat` + 300 seconds |
| `jti` | A random UUID v4 |
| `access` | `[{"type":"repository","name":"zone/server","actions":["pull"]}, …]` |

**Granting.** Only `repository` scopes whose name is exactly one of
`ZONE_REGISTRY_REPOSITORIES` are granted, and only their `pull` action. Push,
delete, `*`, any other repository and `registry:catalog:*` are dropped and
counted. A request with nothing grantable, such as `docker login`, which asks
for no scope at all, still gets `200` with an empty `access`: the registry then
accepts the login and refuses any pull.

### Why the registry doesn't check `jti`

The registry checks `iss`, `aud`, `exp` and `nbf` (with 60 seconds of leeway)
and the signature, but not `iat` or `jti`, and it keeps no replay store. That
is acceptable here:

- **A token grants only `pull`** on images the license holder may pull
  anyway, so a replayed token gives nobody more than the leaked license would.
- **It's bound to one audience**, `registry.bitecode.ltd`, so it opens nothing
  else.
- **It expires 300 seconds after issue**, plus the registry's 60 seconds of
  leeway. Revoking a license or its subscription therefore cuts it off within
  six minutes.

Checking `jti` would need shared state in front of every pull for no gain.

## The bundle route

`GET /bundles/{version}/{architecture}` returns presigned download URLs for
one air-gapped bundle, such as `/bundles/1.4.3-lts/amd64`. The signer works
through each request in this order: the address limit, the license gate, the
key limit, parsing the version and architecture, a probe for the bundle's
commit marker, then presigning.

| `outcome` | Status | When |
|---|---|---|
| `issued` | `200` | The bundle exists |
| `disabled` | `404` | `ZONE_BUNDLE_BUCKET` is unset |
| `unknown` | `404` | The version isn't `X.Y.Z-lts` without leading zeros, the architecture isn't `amd64` or `arm64` (neither calls S3), or S3 answers `NoSuchKey`, `AccessDenied` or a `404` without a code |
| a [refusal](#who-gets-in) | `401` or `403` | The gate refused |
| `limited` | `429` | A rate limit tripped, with `Retry-After` |
| `unavailable` | `503` | S3 can't be reached in 10 seconds, answers any other error code, or any other status |
| `unaddressable` | `500` | The signer can't read the peer address, so it can't apply the address limit. It presigns nothing, never reaches S3 and logs at `error`, with the same body as on `/token` |

`POST` and `HEAD` answer `405` with `Allow: GET`. They presign nothing, never
reach S3 and log no `issued` line.

An `issued` response lists the three files, each URL valid for 900 seconds:

```json
{
  "version": "1.4.3-lts",
  "architecture": "amd64",
  "expires_in": 900,
  "files": [
    {"name": "zone-bundle-1.4.3-lts-amd64.tar", "url": "https://s3.ap-southeast-2.amazonaws.com/…"},
    {"name": "zone-bundle-1.4.3-lts-amd64.tar.sha256", "url": "…"},
    {"name": "zone-bundle-1.4.3-lts-amd64.tar.sha256.sigstore.json", "url": "…"}
  ]
}
```

The object keys are `<ZONE_BUNDLE_PREFIX>/<version>/<name>`. The bundle
workflow uploads the `.sigstore.json` file last, so a bundle counts as present
only once that commit marker exists, and that's the object the signer probes.
It probes with a presigned ranged `GET` (`Range: bytes=0-0`), not a `HEAD`,
because S3 `HEAD` responses carry no error code. `AccessDenied` counts as
absent because S3 answers it, instead of `404`, for a missing key when the
caller can't list the bucket, which the presigner user can't. It still logs at
`warn`, while `NoSuchKey` and a bare `404` log at `info`, because a presigner
policy that has lost `s3:GetObject` answers the same way: `AccessDenied` for a
bundle you know is published means the policy is broken.

`SignatureDoesNotMatch`, `InvalidAccessKeyId` and `ExpiredToken` mean the
presigner's key is wrong or gone. They answer `503`, and like every other S3
error code and any status without one, they log at `error`, because bundle
downloads keep failing until you fix the key or the bucket. Storage that can't
be reached or doesn't answer within 10 seconds logs at `warn` instead.

Every `GET` answer carries `Cache-Control: no-store`, as on `/token`, so no
proxy or browser keeps the presigned links or a refusal.

## Rate limits and client addresses

Each of the two routes has its own pair of fixed one-minute windows:

- **Per client address**, `ZONE_LICENSE_ADDRESS_LIMIT` requests a minute,
  checked before the key is verified, so anonymous and garbage requests count
  too. An IPv4-mapped IPv6 address counts as its IPv4 address, and an IPv6
  address counts as its /64.
- **Per license ID**, `ZONE_LICENSE_KEY_LIMIT` requests a minute, checked once
  the key is admitted.

A tripped limit answers `429` with `Retry-After` in whole seconds, at least 1,
and this body:

```json
{"errors":[{"code":"TOOMANYREQUESTS","message":"Too many requests. Wait for the time in Retry-After, then try again."}]}
```

**Trusted proxies are required in production.** The signer sees Caddy as the
peer of every request. When the peer is inside one of
`ZONE_LICENSE_TRUSTED_PROXIES`, the signer takes the client address from the
rightmost `X-Forwarded-For` entry, which is the one Caddy adds; otherwise it
uses the peer and ignores `X-Forwarded-For`, so a client can't spoof its way
around the limit. Caddy's `reverse_proxy` replaces any `X-Forwarded-For` a
client sends with the address it saw, so the rightmost entry is always the
real client.

Set it to the `signer` network's subnet:

```sh
sudo docker network inspect signer --format '{{range .IPAM.Config}}{{.Subnet}} {{end}}'
```

When `/token` or `/bundles` is on and `ZONE_LICENSE_TRUSTED_PROXIES` is unset,
the signer refuses to start and names the variable. Behind Caddy with no
trusted proxies, every customer would share Caddy's address, so one busy
client would throttle everyone. Only when nothing proxies the signer, so each
peer is the real client, set it to `none`, which trusts no proxy.

At startup, when either route is on, the signer logs the trusted list:

```text
Taking the client address from X-Forwarded-For when the peer is a trusted proxy   proxies=172.18.0.0/16
```

or, set to `none`:

```text
Taking the client address from the peer, because no proxy is trusted   proxies=none
```

## Environment

These variables are all optional and add to the ones in
[LICENSING.md](LICENSING.md#environment). Values are trimmed, a blank value
counts as unset, and every invalid one is named in the startup error. With
none of them set, the signer runs exactly as before and both routes answer
`GET` with `404`. Both still answer every other method with `405`. Once
`ZONE_REGISTRY_SERVICE` or `ZONE_BUNDLE_BUCKET` is set,
`ZONE_LICENSE_TRUSTED_PROXIES` is required.

| Variable | Default | Meaning |
|---|---|---|
| `ZONE_LICENSE_REVOKED` | None | Comma-separated license IDs (UUIDs, any case) that `/token` and `/bundles` refuse as `revoked`. It covers only the key with that ID: a renewal, seat change or plan change issues a new ID that it doesn't cover. Zone servers verify keys offline and still accept them; see [Revocation](LICENSING.md#revocation). |
| `ZONE_LICENSE_REVOKED_SUBSCRIPTIONS` | None | Comma-separated Polar subscription IDs (UUIDs, any case). `/token` and `/bundles` refuse every key on them as `revoked`, including keys renewed after you listed them. A key signed by hand without a subscription is revoked only by its ID. |
| `ZONE_LICENSE_TRUSTED_PROXIES` | Required with either route | Comma-separated CIDR ranges or single addresses, such as `172.18.0.0/16`, or `none` (any case, on its own) when no proxy is in front of the signer. Unset, the signer refuses to start while `/token` or `/bundles` is on. See [Rate limits and client addresses](#rate-limits-and-client-addresses). |
| `ZONE_LICENSE_ADDRESS_LIMIT` | `120` | Requests a minute per client address, per route. A whole number from 1. |
| `ZONE_LICENSE_KEY_LIMIT` | `240` | Requests a minute per license ID, per route. A whole number from 1. |
| `ZONE_REGISTRY_SERVICE` | None | The registry's name, such as `registry.bitecode.ltd`: the `service` clients must send and the token's `aud`. Unset, `/token` is off and answers `404`. It may not contain spaces. |
| `ZONE_REGISTRY_REPOSITORIES` | `zone/server,zone/manager,zone/litellm` | Comma-separated repositories a token may grant `pull` on. Each must be a valid repository path, listed once. |
| `ZONE_REGISTRY_KEY_GENERATION` | `1` | The [token key](#the-token-key) generation to sign with. A whole number from 1. |
| `ZONE_BUNDLE_BUCKET` | None | The S3 bucket holding the bundles. Unset, `/bundles` is off and answers `404`. Once it's set, `ZONE_BUNDLE_REGION`, `ZONE_BUNDLE_ACCESS_KEY_ID` and `ZONE_BUNDLE_SECRET_ACCESS_KEY` are required, and each missing one is named at startup. |
| `ZONE_BUNDLE_REGION` | Required with the bucket | The bucket's region, such as `ap-southeast-2`. |
| `ZONE_BUNDLE_ENDPOINT` | `https://s3.<region>.amazonaws.com` | The S3 endpoint, as an `http` or `https` URL. URLs are path style. |
| `ZONE_BUNDLE_PREFIX` | `bundles` | The key prefix the bundles live under. Leading and trailing slashes are dropped. |
| `ZONE_BUNDLE_ACCESS_KEY_ID` | Required with the bucket | The `zone-bundle-presigner` user's access key ID. |
| `ZONE_BUNDLE_SECRET_ACCESS_KEY` | Required with the bucket | Its secret access key. It's redacted wherever the signer prints its configuration. |

## Logs and pull auditing

Each `/token` and `/bundles` request logs one JSON line on the signer's
standard error, next to the [existing lines](LICENSING.md#logs). Neither ever
logs the license key, the token, a presigned URL, the presigner's secret, or a
customer name or email address.

| Field | Route | Meaning |
|---|---|---|
| `outcome` | both | One of the outcomes above, or a refusal name |
| `license` | both | The license ID, once the key verifies |
| `address` | both | The client address the limits use |
| `plan` | both | The key's plan, on `ineligible` (and wherever the gate read it) |
| `status` | both | The license status, on `lapsed` (and wherever the gate read it) |
| `error` | both | On `unverified`, why the key doesn't verify: `license_malformed`, `license_unknown_key`, `license_signature_invalid` or `license_payload_invalid` |
| `granted` | `/token` | The granted scopes, space-joined as `name:pull`, such as `zone/server:pull zone/manager:pull` |
| `dropped` | `/token` | How many requested actions were dropped, each distinct type, name and action counted once, so `pull,push` on `zone/server` drops 1 |
| `version`, `architecture` | `/bundles` | The bundle asked for |
| `code` | `/bundles` | The S3 error code, when S3 gave one |
| `cause` | `/bundles` | On `unavailable` without a code: that S3 was unreachable, or the status it answered |
| `retry_after` | `/bundles` | On `limited`, the seconds sent in `Retry-After` |

When `/token` is on, the startup log also says `Issuing registry tokens` with
the `service` and the `key_id` it signs with, which is the JWT header's `kid`.
When `/bundles` is on, it says `Serving air-gapped bundles` with the `bucket`
and `prefix`, never the keys.

**Pull auditing.** The registry runs with JSON logs. Once a token checks out,
every request it serves logs `authorized request` and then `response completed`,
both with `auth.user.name` set to the token's `sub`, which is the license ID,
plus the method, URI, status and size. So you can tie each pull to a license
from either side:

```sh
sudo docker logs zone-license 2>&1 | grep '"license":"<license id>"'
sudo docker logs registry 2>&1 | grep '"auth.user.name":"<license id>"'
```

The registry's Apache-style access log is off (`log.accesslog.disabled`): it
isn't JSON and can't carry the user name. `http.request.remoteaddr` is the
first `X-Forwarded-For` entry, which Caddy sets to the customer's address, so
it matches the signer's `address`. Docker keeps container logs only while the container exists,
so ship them somewhere durable if you need an audit trail beyond the host.

## The registry

[`config.yml`](../runner/zone_license/deploy/registry/config.yml) is the
served registry's whole configuration. Every key below was checked against the
distribution v3.1.1 source (`configuration/configuration.go`,
`registry/handlers/app.go`, `registry/auth/token/accesscontroller.go` and
`registry/storage/driver/s3-aws/s3.go`) and by running the pinned image.

- **Storage.** `storage.s3` with no `accesskey` or `secretkey`, so the S3 driver
  takes the instance role's credentials from IMDS, `rootdirectory: /registry`
  (keys start `registry/`) and `encrypt: true` (SSE-S3). `bucket` and `region`
  come from `REGISTRY_STORAGE_S3_BUCKET` and `REGISTRY_STORAGE_S3_REGION`, so
  the file names no bucket. `objectacl: bucket-owner-full-control` is the only
  ACL a bucket with ACLs disabled accepts on a write; it matters only to the
  publishing registry, which shares this storage block.
- **Read-only.** `storage.maintenance.readonly.enabled: true` removes the
  upload and manifest write handlers, so `POST`, `PUT`, `PATCH` and `DELETE`
  answer `405` with `Allow: GET, HEAD` whatever a token grants.
  `storage.delete.enabled: false` turns off the delete API too, and
  `storage.maintenance.uploadpurging.enabled: false` stops the purger, which
  would otherwise try to delete stale uploads the instance role can't delete.
- **Redirects.** `storage.redirect.disable: false` keeps blob `GET`s redirecting
  to presigned S3 URLs. The key must be present with a boolean if `redirect`
  appears at all.
- **Cache.** `storage.cache.blobdescriptor: inmemory`.
- **Token auth.** `auth.token` with `realm: https://licenses.bitecode.ltd/token`,
  `service: registry.bitecode.ltd`, `issuer: zone-license`,
  `jwks: /etc/zone-registry/jwks.json` and `signingalgorithms: [EdDSA]`. The
  registry reads the JWKS file once, at startup, and keys it by `kid`, so a
  changed JWKS needs a restart. A token whose `kid` isn't in it is refused.
- **HTTP.** `http.addr: :5000` on the `signer` network only (no published
  port), and `http.host: https://registry.bitecode.ltd` so any URL the registry
  builds names the public host. No debug server.
- **Logs.** `log.formatter: json`.

The registry logs a warning at startup that no HTTP secret is set. The secret
only signs upload state, and this registry accepts no uploads, so leave it.

[`publish.yml`](../runner/zone_license/deploy/registry/publish.yml) is the CI
registry: the same `storage` block apart from `maintenance` (writable), no
`auth`, listening on `127.0.0.1:5000`. `scripts/test-registry-config.py`
checks that the two storage blocks stay identical, and runs both files through
the pinned image.

### Running it

Keep both files in `/etc/zone-registry/`. The JWKS holds only public keys, so
it can be world-readable, which the registry's unprivileged user needs:

```sh
sudo install -d -m 0755 /etc/zone-registry
sudo install -m 0644 config.yml /etc/zone-registry/config.yml

sudo docker run -d --name registry --restart unless-stopped \
  --network signer \
  --memory 128m --memory-swap 128m \
  --user 65534:65534 --read-only --cap-drop ALL --security-opt no-new-privileges \
  --log-opt max-size=50m --log-opt max-file=10 \
  -e REGISTRY_STORAGE_S3_BUCKET=<bucket> \
  -e REGISTRY_STORAGE_S3_REGION=ap-southeast-2 \
  -v /etc/zone-registry:/etc/zone-registry:ro \
  registry:3.1.1@sha256:325b4b29b041e82803abeb703e201655e4e23ab83264ec1a7c9ddb0a5b14a6e0 \
  /etc/zone-registry/config.yml
```

The digest is the 3.1.1 index, checked with
`docker buildx imagetools inspect registry:3.1.1`. The image's entrypoint
turns a `.yml` argument into `registry serve <file>`. The S3 driver reaches the
instance role through IMDSv2 from inside a container, which needs the
metadata hop limit raised to 2 (see [AWS](#aws)).

## The token key

The registry verifies tokens with an Ed25519 key derived from the license
signing key, so there's no new secret to keep:

```text
seed = HKDF-SHA256(salt = "zone-license registry token", ikm = license seed, info = "<key id>:<generation>")
kid  = registry-<key id>-<generation>        e.g. registry-2026-2-1
```

HKDF is one-way, so a leaked token key can't sign licenses, and the cold
backup of the license key recovers every token key.

`zone-license registry jwks` prints the public half as a JWKS, one key per
`--generation` (default `1`; repeat it for an overlap). It reads the private
key the way `sign` does, from `--private-key-file`, `ZONE_LICENSE_PRIVATE_KEY_FILE`
or `ZONE_LICENSE_PRIVATE_KEY`, takes the key id from `--key-id` or
`ZONE_LICENSE_KEY_ID`, and refuses a key this release doesn't trust:

```sh
sudo docker run --rm --user 10001:10001 --network none \
  -v /etc/zone-license/zone-license-2026-2.key:/run/secrets/zone-license-key:ro \
  ghcr.io/bitecodehq/zone-license:1.3.0 \
  registry jwks --generation 1 --key-id 2026-2 --private-key-file /run/secrets/zone-license-key \
  | sudo tee /etc/zone-registry/jwks.json >/dev/null
sudo chmod 0644 /etc/zone-registry/jwks.json
```

It prints only public keys, in the shape the registry reads:

```json
{"keys":[{"kty":"OKP","crv":"Ed25519","alg":"EdDSA","use":"sig","kid":"registry-2026-2-1","x":"…"}]}
```

### Rotating it

A token lives 300 seconds and the registry allows 60 seconds of leeway, so
after the signer switches keys, the old key must stay trusted for more than
six minutes. Always widen the registry's JWKS before the signer switches, and
narrow it only afterwards.

**A new generation** (a suspected token-key leak, or routine rotation):

1. Publish both generations and restart the registry:

   ```sh
   … registry jwks --generation 1 --generation 2 … | sudo tee /etc/zone-registry/jwks.json >/dev/null
   sudo docker restart registry
   ```

2. Set `ZONE_REGISTRY_KEY_GENERATION=2` in `signer.env` and recreate the signer
   container (`docker restart` keeps the old environment). Its startup line
   names the new `kid`.
3. Wait more than 360 seconds.
4. Publish generation 2 alone and restart the registry again.

**A license-key rotation** (see
[Rotating the signing key](LICENSING.md#rotating-the-signing-key)) changes the
kid prefix from `registry-<old id>-` to `registry-<new id>-`. Follow the same
overlap with the new key id's kid:

1. Before the signer switches, print the old key's JWKS and the new key's,
   merge them, and restart the registry:

   ```sh
   jq -s '{keys: (map(.keys) | add)}' old.json new.json | sudo tee /etc/zone-registry/jwks.json >/dev/null
   sudo docker restart registry
   ```

2. Switch the signer to the new key, as LICENSING describes.
3. Wait more than 360 seconds.
4. Publish the new key's JWKS alone and restart the registry.

A registry restart takes a second or two; Docker clients retry the requests it
drops.

## Caddy

The live `/etc/zone-license/Caddyfile` is reconstructed in
[`Caddyfile.current`](../runner/zone_license/deploy/Caddyfile.current). The
target is [`Caddyfile`](../runner/zone_license/deploy/Caddyfile), and
[`Caddyfile.diff`](../runner/zone_license/deploy/Caddyfile.diff) is the change
between them. Both validate with Caddy 2.11.9.

`licenses.bitecode.ltd` passes these to `zone-license:8080`, and answers
everything else `404`:

| Path | Methods | Called by |
|---|---|---|
| `/webhooks/polar` | any | Polar |
| `/licenses/renew` | any | Customers' Zone servers |
| `/health` | any | Your health check |
| `/token` | `GET`, `POST`, `HEAD` | Docker, containerd and Kubernetes nodes. The signer answers `POST` and `HEAD` with `405`, which sends containerd to `GET` |
| `/bundles/*` | `GET`, `HEAD` | Customers downloading a bundle. The signer answers `HEAD` with `405`; Caddy answers `POST` with `404` |

`registry.bitecode.ltd` passes only `GET` and `HEAD` on `/v2/*` to
`registry:5000`. Any other method on `/v2/*` gets `405` with
`Allow: GET, HEAD` from Caddy, and any other path `404`. A push is refused
three times over: the token never grants `push`, Caddy refuses the upload, and
the registry is read-only.

## AWS

Replace `<bucket>` and `<account id>` throughout. Keep the bucket name free of
dots, so S3's virtual-hosted URLs match its TLS certificate.

### The bucket

One bucket in ap-southeast-2, with public access blocked, SSE-S3, ACLs disabled
(the default), versioning on, and lifecycle rules that abort incomplete
multipart uploads after 7 days and expire the registry's replaced versions
after 7 days:

```sh
aws s3api create-bucket --bucket <bucket> --region ap-southeast-2 \
  --create-bucket-configuration LocationConstraint=ap-southeast-2
aws s3api put-public-access-block --bucket <bucket> --public-access-block-configuration \
  BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true
aws s3api put-bucket-encryption --bucket <bucket> --server-side-encryption-configuration \
  '{"Rules":[{"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":"AES256"}}]}'
aws s3api put-bucket-versioning --bucket <bucket> --versioning-configuration Status=Enabled
aws s3api put-bucket-lifecycle-configuration --bucket <bucket> --lifecycle-configuration \
  '{"Rules":[{"ID":"abort-incomplete-uploads","Status":"Enabled","Filter":{},"AbortIncompleteMultipartUpload":{"DaysAfterInitiation":7}},{"ID":"expire-replaced-registry-versions","Status":"Enabled","Filter":{"Prefix":"registry/"},"NoncurrentVersionExpiration":{"NoncurrentDays":7},"Expiration":{"ExpiredObjectDeleteMarker":true}}]}'
aws s3api put-bucket-policy --bucket <bucket> --policy file://bundle-policy.json
```

`bundle-policy.json` refuses every write to `bundles/` that could replace an
object:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "DenyBundleOverwrites",
      "Effect": "Deny",
      "Principal": "*",
      "Action": "s3:PutObject",
      "Resource": "arn:aws:s3:::<bucket>/bundles/*",
      "Condition": {
        "Null": {"s3:if-none-match": "true"},
        "Bool": {"s3:ObjectCreationOperation": "true"}
      }
    }
  ]
}
```

`s3:if-none-match` is the condition key S3 sets from a write's `If-None-Match`
header, and `Null` matches a `PutObject` or `CompleteMultipartUpload` that
doesn't send one. `CreateMultipartUpload`, `UploadPart` and `UploadPartCopy`
can't carry the header, so `s3:ObjectCreationOperation` exempts them; the
part uploads create nothing until `CompleteMultipartUpload`, which the policy
does check. S3 then answers a write to an existing key with
`412 Precondition Failed`, whoever holds the credentials. The bundle workflow
sends `If-None-Match: *` on all three files: `put-object --if-none-match '*'`
for the checksum and its signature, and `s3 cp --no-overwrite` for the tar,
which AWS CLI 2.33.0 and later put on the `PutObject` or the
`CompleteMultipartUpload` of a multipart upload. A copy into `bundles/`
without the header is refused too. See
[Enforce conditional writes](https://docs.aws.amazon.com/AmazonS3/latest/userguide/conditional-writes-enforce.html).

`s3 cp --no-overwrite` treats `412` as a skipped file and exits `0`, so
`scripts/bundle-upload.sh` refuses to start when the tar's key already exists, uploads it with
its SHA-256 in the `sha256` metadata, and stops unless the object it then
reads back carries that hash.

Versioning covers what the policy can't: a delete, or a write by the bucket
owner after removing the policy, leaves the previous version recoverable. The
registry commits every upload by copying it into place and deleting the
original, so its prefix piles up replaced versions; the second lifecycle rule
expires them. Restore a bundle by copying its previous version back:

```sh
aws s3api list-object-versions --bucket <bucket> --prefix bundles/1.4.3-lts/
aws s3api copy-object --bucket <bucket> --key <key> \
  --copy-source '<bucket>/<key>?versionId=<version id>'
```

The policy refuses that copy, so an operator removes it for the restore and
puts it back after.

The prefixes `registry/` and `bundles/` appear with the first publish. Nothing
purges the registry's own `_uploads` state from a failed publish; it is a few
small files.

**Egress.** Customers download blobs and bundles straight from S3, which bills
internet egress per gigabyte. A first pull of the three images is several
gigabytes, and a bundle is about 11 GB per architecture per download. Check the
current ap-southeast-2 rate before turning this on.

### The instance role

Attach a role to `i-0325b72f3b2ad4f9f` (or add this policy to its existing
role). It reads and lists the registry's storage and nothing else:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "ListDistribution",
      "Effect": "Allow",
      "Action": ["s3:ListBucket", "s3:GetBucketLocation"],
      "Resource": "arn:aws:s3:::<bucket>"
    },
    {
      "Sid": "ReadRegistry",
      "Effect": "Allow",
      "Action": "s3:GetObject",
      "Resource": "arn:aws:s3:::<bucket>/registry/*"
    }
  ]
}
```

`s3:ListBucket` isn't narrowed to the `registry/` prefix. S3 answers a `GET`
for a missing key with `404 NoSuchKey` only when the caller may list the
bucket, and a prefix condition doesn't apply to a `GET`; without it, a
mistyped tag gets `403` from S3 and the registry answers `500` instead of
`MANIFEST_UNKNOWN`. Listing reveals only key names, and bundle versions aren't
secret.

The trust policy is the standard EC2 one:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {"Service": "ec2.amazonaws.com"},
      "Action": "sts:AssumeRole"
    }
  ]
}
```

The registry runs in a container, one network hop past the host, so raise the
IMDSv2 hop limit to 2:

```sh
aws ec2 modify-instance-metadata-options --instance-id i-0325b72f3b2ad4f9f \
  --http-tokens required --http-put-response-hop-limit 2
```

### The presigner user

An IAM user, `zone-bundle-presigner`, whose access key goes in `signer.env` as
`ZONE_BUNDLE_ACCESS_KEY_ID` and `ZONE_BUNDLE_SECRET_ACCESS_KEY`. It can only
read bundles, so a leaked key only lets someone download bundles, which one
leaked license allows anyway:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "ReadBundles",
      "Effect": "Allow",
      "Action": "s3:GetObject",
      "Resource": "arn:aws:s3:::<bucket>/bundles/*"
    }
  ]
}
```

It has no `s3:ListBucket`, so S3 answers a missing bundle with `AccessDenied`,
which the signer reports as `unknown` and logs at `warn`.

### The publisher role

GitHub Actions assumes `zone-distribution-publisher` through OIDC. First add
the provider `token.actions.githubusercontent.com` with audience
`sts.amazonaws.com`, if the account doesn't have it. The trust policy admits
only jobs running in the `distribution` environment of `bitecodehq/zone`:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Federated": "arn:aws:iam::<account id>:oidc-provider/token.actions.githubusercontent.com"
      },
      "Action": "sts:AssumeRoleWithWebIdentity",
      "Condition": {
        "StringEquals": {
          "token.actions.githubusercontent.com:aud": "sts.amazonaws.com",
          "token.actions.githubusercontent.com:sub": "repo:bitecodehq/zone:environment:distribution"
        }
      }
    }
  ]
}
```

It reads and writes both prefixes. Only the registry's storage may be deleted
from, because the registry commits an upload by copying it into place and
deleting the original; bundles are only ever added:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "ListDistribution",
      "Effect": "Allow",
      "Action": ["s3:ListBucket", "s3:GetBucketLocation", "s3:ListBucketMultipartUploads"],
      "Resource": "arn:aws:s3:::<bucket>"
    },
    {
      "Sid": "WriteDistribution",
      "Effect": "Allow",
      "Action": ["s3:GetObject", "s3:PutObject", "s3:ListMultipartUploadParts", "s3:AbortMultipartUpload"],
      "Resource": ["arn:aws:s3:::<bucket>/registry/*", "arn:aws:s3:::<bucket>/bundles/*"]
    },
    {
      "Sid": "CommitRegistryUploads",
      "Effect": "Allow",
      "Action": ["s3:DeleteObject", "s3:PutObjectAcl"],
      "Resource": "arn:aws:s3:::<bucket>/registry/*"
    }
  ]
}
```

`s3:PutObjectAcl` covers the `bucket-owner-full-control` header the registry
sends on every write.

The bundle job starts its registry with a two-hour session, because assembling
about 11 GB through it can outlast the default hour. STS refuses a session
longer than the role's maximum, which is one hour unless you raise it:

```sh
aws iam update-role --role-name zone-distribution-publisher --max-session-duration 7200
```

The registry runs only while the bundle is assembled, and the upload takes a
fresh one-hour session.

## Network access

- **The host** needs outbound HTTPS to `s3.ap-southeast-2.amazonaws.com` and
  `<bucket>.s3.ap-southeast-2.amazonaws.com`, which the default security group
  egress allows. Inbound stays 80 and 443; the registry publishes no port.
- **Customers** pulling images or downloading bundles must reach, on 443:
  - `registry.bitecode.ltd`, for manifests;
  - `licenses.bitecode.ltd`, for tokens and bundle URLs;
  - `*.s3.ap-southeast-2.amazonaws.com`, where the registry's blob redirects go
    (`<bucket>.s3.ap-southeast-2.amazonaws.com`);
  - `s3.ap-southeast-2.amazonaws.com`, where the bundle URLs go (path style).

  A proxy or firewall that allows only the two `bitecode.ltd` names fails on
  the first layer download, after the manifest succeeds. Docker drops the
  `Authorization` header when it follows the redirect, so S3 never sees the
  registry token.

## Deploying

None of this is live yet. In order, after the code merges:

1. **DNS.** An `A` record `registry.bitecode.ltd` → `3.24.143.182` in
   Cloudflare. It and `licenses.bitecode.ltd` must both be DNS only (not
   proxied), so Caddy can get their certificates and sees each customer's own
   address. Behind Cloudflare's proxy, every request reaches Caddy from a
   Cloudflare edge address, so `X-Forwarded-For` names the edge and not the
   customer. Customers behind the same edge would then share one
   [address limit](#rate-limits-and-client-addresses), and the `address` in
   the signer's logs and `http.request.remoteaddr` in the registry's would
   name Cloudflare instead of the client. Check `licenses.bitecode.ltd` too:
   it predates distribution and may have been proxied.
2. **S3.** Create the bucket as in [The bucket](#the-bucket), once the egress
   cost is approved.
3. **IAM.** The [instance role](#the-instance-role) with the hop limit raised
   to 2, the [presigner user](#the-presigner-user) and its access key, and the
   OIDC provider with the [publisher role](#the-publisher-role).
4. **GitHub.**
   - An environment `distribution` with a required reviewer and a deployment
     policy that allows only `v*-lts` tags.
   - Repository variables `ZONE_DISTRIBUTION_BUCKET`, `ZONE_DISTRIBUTION_REGION`
     and `ZONE_DISTRIBUTION_ROLE` (the role's ARN). `ZONE_LTS_RUNNER_AMD64` and
     `ZONE_LTS_RUNNER_ARM64` are optional.
   - A tag ruleset on `v*-lts` (admins only) and a branch ruleset on
     `release/*-lts` (pull request and green CI required).
5. **Signer.** Open port 22 to your address in `sg-021730ada0d45d64d` for this
   session only, then:
   1. Tag `license-signer-v1.3.0` and wait for its image.
   2. Record the running image:
      `sudo docker inspect zone-license --format '{{.Config.Image}}'`.
   3. Back up the env file:
      `sudo cp -p /etc/zone-license/signer.env /etc/zone-license/signer.env.<date>-before-distribution`.
   4. Write `/etc/zone-registry/jwks.json` with
      `zone-license registry jwks --generation 1` and the 2026-2 key, as in
      [The token key](#the-token-key).
   5. Add to `signer.env`: `ZONE_REGISTRY_SERVICE=registry.bitecode.ltd`,
      `ZONE_LICENSE_TRUSTED_PROXIES=<the signer network's subnet>`,
      `ZONE_BUNDLE_BUCKET=<bucket>`, `ZONE_BUNDLE_REGION=ap-southeast-2`,
      `ZONE_BUNDLE_ACCESS_KEY_ID` and `ZONE_BUNDLE_SECRET_ACCESS_KEY`.
   6. `sudo docker rename zone-license zone-license-previous && sudo docker stop zone-license-previous`.
   7. Start `ghcr.io/bitecodehq/zone-license:1.3.0` as `zone-license` with the
      same flags as before: `--network signer --restart unless-stopped
      --user 10001:10001 --env-file /etc/zone-license/signer.env` and the key
      mounted read-only at `/run/secrets/zone-license-key`.
   8. Check the startup log lists the trusted proxies once, says
      `Issuing registry tokens` with `key_id` `registry-2026-2-1`, and says
      `Serving air-gapped bundles` with your bucket.
   9. To roll back: `sudo docker rm -f zone-license`, restore the env backup,
      `sudo docker rename zone-license-previous zone-license && sudo docker start zone-license`.
6. **Registry.** Copy `config.yml` to `/etc/zone-registry/` and start it as in
   [Running it](#running-it). Check its log ends with `listening on [::]:5000`.
7. **Caddy.**
   1. Back up the live file:
      `sudo cp -p /etc/zone-license/Caddyfile /etc/zone-license/Caddyfile.<date>`.
   2. Reconcile it with the reference `Caddyfile`, using `Caddyfile.diff` as the
      guide. If the live file differs from `Caddyfile.current`, keep its
      differences.
   3. Find where the `caddy` container reads it with
      `sudo docker inspect caddy --format '{{json .Mounts}}'` (the official image
      reads `/etc/caddy/Caddyfile`), then validate and reload:
      `sudo docker exec caddy caddy validate --config /etc/caddy/Caddyfile` and
      `sudo docker exec caddy caddy reload --config /etc/caddy/Caddyfile`.
   4. Keep the backup for rollback: restore it and reload.

   Close port 22 again when done.
8. **Smoke tests**, below.

No LTS branch is cut as part of this. That's a separate decision.

## Smoke tests

Never put a license key on a command line. Keep it in a curl config file only
you can read:

```sh
umask 077
cat > ~/.zone-license.curl   # then type: user = "<license id>:<license key>"
```

With a hand-signed Enterprise test key (see
[Hand-signing a license](LICENSING.md#hand-signing-a-license)):

1. **Login.** `docker login registry.bitecode.ltd -u <license id> --password-stdin`
   succeeds, and the signer logs `outcome` `issued` with an empty `granted` and
   `dropped` 0.
2. **A pull token.**

   ```sh
   curl -sS -K ~/.zone-license.curl \
     'https://licenses.bitecode.ltd/token?service=registry.bitecode.ltd&scope=repository:zone/server:pull,push'
   ```

   answers `200`; the signer logs `granted` `zone/server:pull` and `dropped` 1,
   the `push`.
3. **Refusals.** A Team key gets `403` `ineligible`; a garbage password gets
   `401` with `WWW-Authenticate: Basic realm="Zone license", charset="UTF-8"`;
   no `service` gets `400` `unreadable`.
4. **Methods.** Each of these prints `405 GET`:

   ```sh
   curl -sS -o /dev/null -w '%{http_code} %header{allow}\n' -X POST https://licenses.bitecode.ltd/token
   curl -sS -o /dev/null -w '%{http_code} %header{allow}\n' -I https://licenses.bitecode.ltd/token
   curl -sS -o /dev/null -w '%{http_code} %header{allow}\n' -I https://licenses.bitecode.ltd/bundles/1.4.3-lts/amd64
   ```

   and `curl -sS -o /dev/null -w '%{http_code}\n' -X POST https://licenses.bitecode.ltd/bundles/1.4.3-lts/amd64`
   prints `404`, from Caddy.
5. **No pushes.** Tagging any image as `registry.bitecode.ltd/zone/server:smoke`
   and pushing it fails, and
   `curl -sS -o /dev/null -w '%{http_code}\n' -X POST https://registry.bitecode.ltd/v2/zone/server/blobs/uploads/`
   prints `405`.
6. **Separate address limits.** From one address, send one more anonymous
   request than `ZONE_LICENSE_ADDRESS_LIMIT` within a minute:

   ```sh
   for request in $(seq 130); do
     curl -sS -o /dev/null -w '%{http_code}\n' 'https://licenses.bitecode.ltd/token?service=registry.bitecode.ltd'
   done | sort | uniq -c
   ```

   It prints at most 120 `401`s per minute and `429` for the rest; the window
   is fixed, so a run that crosses a minute boundary starts counting again.
   While the first address is still limited, from a second address
   (AWS CloudShell, or a phone hotspot), one request still gets `401`, not
   `429`. The signer logs each with its own `address`, never Caddy's
   `172.x.x.x`. If the second address gets `429` too, the trusted proxies are
   wrong.
7. **After the first LTS tag:**
   - `docker pull registry.bitecode.ltd/zone/server:X.Y-lts` works, and
     `cosign verify` passes for it (see [LTS.md](LTS.md)).
   - The registry log shows the pull with `auth.user.name` set to the test
     license ID.
   - `curl -sS -K ~/.zone-license.curl https://licenses.bitecode.ltd/bundles/X.Y.Z-lts/amd64`
     answers `200` with three URLs that download, and the bundle verifies
     offline and installs with `install.sh compose` on a machine with no
     network (see [AIRGAP.md](AIRGAP.md)).

Delete `~/.zone-license.curl` when you're done.
