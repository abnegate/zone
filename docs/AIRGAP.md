# Air-gapped installs

A Zone Enterprise license can download an air-gapped bundle for each LTS
release: every image Zone runs, the source for Compose, the Helm charts and an
installer, in one signed tar per architecture. This page covers getting it
onto a host with no internet access, checking it, and installing, licensing
and upgrading Zone from it.

Bundles exist only for LTS releases (`X.Y.Z-lts`, see [LTS.md](LTS.md)) and
only for an Enterprise license that is active or in its grace period.

## What's in a bundle

`zone-bundle-<version>-<architecture>.tar` holds one directory,
`zone-bundle-<version>-<architecture>/`:

| Path | What |
|---|---|
| `VERSION` | The release, such as `1.4.3-lts` |
| `install.sh` | The installer |
| `bin/crane` | [crane](https://github.com/google/go-containerregistry), which the installer pushes images with, built for the bundle's architecture |
| `LICENSES/crane` | crane's license (Apache-2.0) |
| `source.tar` | The Zone source at the release tag, for Compose |
| `compose/docker-compose.airgap.yml` | The Compose override that points every service at a digest-pinned image |
| `helm/<chart>-<version>.tgz` | The `zone-apps`, `zone-infra` and `zone-ai` charts |
| `images/registry.tar` | `registry:3.1.1`, used only while `install.sh compose` runs |
| `images/<name>/` | One OCI image layout per image, for the bundle's architecture only |
| `images.txt` | One line per image: name, repository, tag, digest, layout and where it's used |
| `SHA256SUMS` | The checksum of every other file |

Two more files sit next to the tar when you download it:
`<tar>.sha256`, the tar's checksum, and `<tar>.sha256.sigstore.json`, the
Sigstore signature over that checksum.

## Prerequisites

On a connected machine, to download and check the bundle:

- `curl`, and `jq` to read the download response;
- [cosign](https://github.com/sigstore/cosign) v3.1.4 or later;
- `sha256sum`, or `shasum` on macOS;
- room for the bundle, about 11 GB.

On the air-gapped host, for Compose:

- Linux on the bundle's architecture (`amd64` or `arm64`);
- Docker Engine with Docker Compose **2.24 or later**. Check with
  `docker compose version`. The override uses Compose's `!reset` tag, which
  older releases don't understand, and `install.sh compose` refuses them;
- `make`, plus `bash`, `openssl` and `htpasswd` for `scripts/setup.sh`;
- free disk for the tar, its extracted copy, a temporary registry copy of the
  images and the images Docker unpacks: plan for five times the bundle's size.
  Delete the tar once it's extracted and verified.

For Kubernetes:

- `kubectl` and Helm 3 on a machine that can reach the cluster and your
  registry, on the bundle's architecture, since `install.sh push` runs the
  bundled crane;
- a registry the cluster's nodes pull from;
- the [CloudNativePG](https://cloudnative-pg.io) operator already installed,
  and a default StorageClass, as [the Helm quick start](../helm/QUICK_START.md)
  describes. The operator's own image isn't in the bundle.

## Downloading

Downloads use your license: the user name is the **license ID**, shown under
**Settings → This server → License**, and the password is the **license key**.
Keep the key off the command line, where it would land in shell history and
in the process list. Put both in a file only you can read, and point curl at
it.

With a netrc file (create it with an editor, then `chmod 600 ~/.zone-netrc`):

```
machine licenses.bitecode.ltd
login 2b9f3c1e-6d7a-4e5b-9c8d-0f1a2b3c4d5e
password <license key>
```

```sh
curl --fail --silent --show-error --netrc-file ~/.zone-netrc \
  https://licenses.bitecode.ltd/bundles/1.4.3-lts/amd64 > bundle.json
```

Or with a curl config file (`chmod 600 ~/.zone-curl`):

```
user = "2b9f3c1e-6d7a-4e5b-9c8d-0f1a2b3c4d5e:<license key>"
```

```sh
curl --fail --silent --show-error -K ~/.zone-curl \
  https://licenses.bitecode.ltd/bundles/1.4.3-lts/amd64 > bundle.json
```

The path is `/bundles/<version>/<architecture>`, where the version is
`X.Y.Z-lts` and the architecture is `amd64` or `arm64`. The answer is JSON
with three presigned download links:

```json
{
  "version": "1.4.3-lts",
  "architecture": "amd64",
  "expires_in": 900,
  "files": [
    { "name": "zone-bundle-1.4.3-lts-amd64.tar", "url": "https://…" },
    { "name": "zone-bundle-1.4.3-lts-amd64.tar.sha256", "url": "https://…" },
    { "name": "zone-bundle-1.4.3-lts-amd64.tar.sha256.sigstore.json", "url": "https://…" }
  ]
}
```

Download each file. The links need no credentials, so don't pass the netrc or
config file again:

```sh
jq -r '.files[] | [.name, .url] | @tsv' bundle.json |
  while IFS="$(printf '\t')" read -r name url; do
    curl --fail --location --output "$name" "$url"
  done
rm bundle.json
```

Each link works for 900 seconds (15 minutes) from the response, and anyone
holding one can download that file, so treat `bundle.json` like a password
until then. A download that starts in time may run past the 15 minutes. If
one is cut off, ask for fresh links and resume it with
`curl --continue-at - --output <name> <url>`.

| Status | Meaning |
|---|---|
| `200` | The JSON above. |
| `401` | The license ID or key is wrong, the key doesn't verify, or the ID doesn't match the key. |
| `403` | The license isn't Enterprise, or it has lapsed past its grace period. |
| `404` | No bundle for that version and architecture. |
| `429` | Too many requests. Wait for the number of seconds in `Retry-After`. |
| `503` | Bundle storage is unavailable. Try again later. |

Error bodies look like
`{"errors":[{"code":"UNAUTHORIZED","message":"…"}]}`.

## Checking the bundle

Three checks, in order. The first proves the checksum was signed by Zone's
bundle workflow for that release tag. The second ties the tar to the checksum.
The third checks every file after extraction, so it also catches damage on
the way across to the air-gapped host.

### 1. The Sigstore trust root

`cosign verify-blob` checks the signature against Sigstore's trust root,
`trusted_root.json`. The bundle never carries it, because a bundle that
brought its own trust root would vouch for itself. Fetch it on a connected
machine from Sigstore's TUF repository:

```sh
cosign initialize
cp ~/.sigstore/root/tuf-repo-cdn.sigstore.dev/targets/trusted_root.json .
```

`cosign initialize` checks the TUF metadata against the root embedded in
cosign and caches the trusted root under `~/.sigstore/root/` (or under
`$TUF_ROOT` when that's set). Sigstore rotates its keys without notice, so
fetch a fresh copy for each bundle rather than keeping one indefinitely.

### 2. The signature and the checksum

Run these on the connected machine or the air-gapped host, with
`trusted_root.json` and the three downloaded files in one directory. Replace
`1.4.3` with the bundle's version; the tag in the identity is the version
with a leading `v`:

```sh
cosign verify-blob \
  --bundle zone-bundle-1.4.3-lts-amd64.tar.sha256.sigstore.json \
  --trusted-root trusted_root.json \
  --certificate-identity https://github.com/bitecodehq/zone/.github/workflows/bundle.yml@refs/tags/v1.4.3-lts \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  zone-bundle-1.4.3-lts-amd64.tar.sha256

sha256sum -c zone-bundle-1.4.3-lts-amd64.tar.sha256
```

On macOS, use `shasum -a 256 -c` for the second command. Both must pass.
Nothing in the verification contacts the network.

### 3. The extracted files

```sh
tar -xf zone-bundle-1.4.3-lts-amd64.tar
cd zone-bundle-1.4.3-lts-amd64
./install.sh verify
```

`verify` checks every file against `SHA256SUMS`. Every other `install.sh`
command runs it first and stops on the first mismatch, before it calls Docker
or crane.

## Installing with Compose

From the extracted bundle:

```sh
./install.sh compose --directory /opt/zone
```

| Option | Default | Meaning |
|---|---|---|
| `--directory DIR` | `./zone` | Where the Zone source goes. Use the same directory for every upgrade. |
| `--port PORT` | `5000`, or the port already in `.env` | The loopback port of the temporary registry. |

`compose`:

1. checks the bundle and the Compose version;
2. extracts `source.tar` into the directory, keeping any `.env` already there;
3. stops if there's no `.env` yet (see below);
4. loads `images/registry.tar` and runs it on `127.0.0.1:<port>` with a
   temporary volume;
5. pushes every image into it, by digest and by tag, and checks that each
   digest comes out unchanged;
6. copies `docker-compose.airgap.yml` into the directory;
7. adds `ZONE_AIRGAP_REGISTRY=127.0.0.1:<port>` and
   `ZONE_LICENSE_RENEWAL=false` to `.env`, each only if it isn't set already;
8. runs `docker compose -f docker-compose.yml -f docker-compose.airgap.yml pull`
   for every service the bundle covers, in every profile;
9. removes the temporary registry and its volume. It does this on failure
   too, and when interrupted.

On a fresh host the first run stops at step 3, because `.env` holds the
secrets that `scripts/setup.sh` generates. Create it, then run `compose`
again:

```sh
cd /opt/zone
./scripts/setup.sh --skip-models
```

`--skip-models` writes the secrets and the basic auth file and downloads
nothing.

Docker now holds every image under
`127.0.0.1:<port>/<repository>@<digest>`, the names the override uses. The
override sets `pull_policy: missing`, so Compose finds them locally and
never contacts the registry again. The registry name is part of each image's
name, so leave `ZONE_AIRGAP_REGISTRY` as `compose` wrote it. Later runs
reuse the port it names, and refuse a `--port` that disagrees with it.

Start Zone from the directory:

```sh
make up
```

`make up` and `./scripts/compose.sh up -d` add `docker-compose.airgap.yml`
whenever it's in the directory, and keep doing so with any `PROFILES`. With a
plain `docker compose`, name both files every time:

```sh
docker compose -f docker-compose.yml -f docker-compose.airgap.yml up -d
```

Don't set `COMPOSE_FILE` for this, and don't copy the override over
`docker-compose.override.yml`: `scripts/compose.sh` passes its own file list
and ignores `COMPOSE_FILE`.

The `vpn`, `monitoring` and `bundled-ollama` profiles' images are all loaded,
so `make up PROFILES=monitoring` works offline. `make update` pulls from the
internet; don't use it here.

### Compose against your own registry

To serve several hosts from one internal registry, push the images there
(see the next section) instead of running `compose`. On each host, extract
`source.tar` into the Zone directory, copy `compose/docker-compose.airgap.yml`
next to it, run `scripts/setup.sh --skip-models`, and set in `.env`:

```
ZONE_AIRGAP_REGISTRY=registry.internal:5443/mirror
ZONE_LICENSE_RENEWAL=false
```

Then `./scripts/compose.sh pull` and `make up`. `install.sh compose` refuses
a `ZONE_AIRGAP_REGISTRY` that isn't on `127.0.0.1`, so it can't replace these
images by accident.

## Installing with Helm

### 1. Push the images

Log in to your registry first. crane reads Docker's credentials, so either
`docker login registry.internal:5443` or
`bin/crane auth login registry.internal:5443 -u <user> --password-stdin`
works. Then:

```sh
./install.sh push --registry registry.internal:5443/mirror
```

The value is `HOST[:PORT][/PREFIX]`, with no scheme. Each image goes to
`<registry>/<repository>`, for example
`registry.internal:5443/mirror/zone/server`, by digest and by tag, and `push`
stops if any digest comes out different from the one in `images.txt`. Add
`--insecure` for a registry served over plain HTTP or with a certificate
crane doesn't trust.

### 2. Write the values

```sh
./install.sh helm-values --registry registry.internal:5443/mirror --output values
```

It writes `values/zone-apps.yaml`, `values/zone-infra.yaml` and
`values/zone-ai.yaml`, setting every image key by key to the pushed
repository and `<tag>@<digest>`:

```yaml
server:
  image:
    repository: "registry.internal:5443/mirror/zone/server"
    tag: "1.4.3-lts@sha256:…"
```

| Chart | Keys |
|---|---|
| `zone-apps` | `server.image`, `migration.image`, `manager.image` (repository and tag), `initContainers.postgres.image` (one string) |
| `zone-infra` | `valkey.image` (repository and tag), `postgres.image` (one string) |
| `zone-ai` | `ollama.image`, `ollama.init.image`, `litellm.image` (repository and tag) |

Only `zone-apps` has a registry-wide setting, `global.imageRegistry`, and it
is joined to each repository with no separator, so a value there must end
with a slash: `registry.internal:5443/mirror/`, not
`registry.internal:5443/mirror`. The generated values don't use it, because
`zone-infra` and `zone-ai` have no such setting and need the per-key values
anyway. Leave `global.imageRegistry` empty when you use them; a value left
over from pulling LTS images straight from Zone's registry would be added in
front of the full repositories a second time.

### 3. Install the charts

Create the namespace and secrets as in [the Helm quick start](../helm/QUICK_START.md),
then install from the bundle's charts, putting the generated file after your
own values so its images win:

```sh
helm upgrade --install zone-infra helm/zone-infra-*.tgz --namespace zone \
  -f values/zone-infra.yaml
helm upgrade --install zone-ai helm/zone-ai-*.tgz --namespace zone \
  -f values/zone-ai.yaml
helm upgrade --install zone-apps helm/zone-apps-*.tgz --namespace zone \
  -f my-zone-apps.yaml -f values/zone-apps.yaml \
  --set-string 'commonEnv.ZONE_LICENSE_RENEWAL=false'
```

If the registry needs credentials, create a pull secret and list it under
`global.imagePullSecrets` in your `zone-apps` values. `zone-infra` and
`zone-ai` have no pull secret setting. The simplest fix is to let the nodes
authenticate to the registry in their container runtime's configuration.
Otherwise, Valkey and the `zone-ai` pods run as the namespace's `default`
service account, which can carry the secret:

```sh
kubectl -n zone patch serviceaccount default \
  -p '{"imagePullSecrets":[{"name":"zone-registry"}]}'
```

That doesn't reach the PostgreSQL pods, which CloudNativePG runs under a
service account of its own.

One image isn't covered by values yet: the `wait-for-ollama` init container
in `zone-ai`'s LiteLLM Deployment names
`curlimages/curl:8.22.0@sha256:58adaa4e…` directly, so it ignores the
generated values. `push` puts the same image in your registry as
`curlimages/curl`. Until the chart takes it as a value, have your container
runtime mirror `docker.io` to your registry, or rewrite the image with a Helm
post-renderer.

## Licensing offline

A license is verified on the server with keys built into Zone, so an
air-gapped server needs nothing from Zone to use it:

1. Open **Settings → This server → License**. Only server admins see it.
2. Paste the key.
3. Click **Add license** (**Replace license** if one is already installed).

The renewal check is the only request Zone makes about its license. Turn it
off with `ZONE_LICENSE_RENEWAL=false`, or the server tries every day from 14
days before expiry and records `unreachable`. `install.sh compose` writes it
to `.env` when it isn't set; on Helm, set it as above. It's read at startup.
Renewed keys arrive by email: paste each one before the current key's 30-day
grace period ends. See [LICENSING.md](LICENSING.md#air-gapped-servers) and
[CONFIGURATION.md](CONFIGURATION.md#-license-renewal).

## Upgrading

### Compose

1. Back up: `make backup`. It copies the volumes in an `alpine` container and
   pulls `alpine` when it isn't present, which the bundle doesn't carry. Copy
   it across once: `docker pull alpine && docker save alpine -o alpine.tar`
   on a connected machine, then `docker load -i alpine.tar` on the host.
2. Download and check the new bundle as above.
3. Run its installer into the same directory:
   `./install.sh compose --directory /opt/zone`. It keeps `.env` and the
   registry port, replaces the source and the override, and loads the new
   images.
4. `make up`. Compose recreates the containers whose image changed, and
   `zone-server` applies any new database migrations when it starts.

The previous release's images stay in Docker until you remove them with
`docker image prune`. Migrations only go forward, so going back to an older
release means restoring the backup (`make restore`).

### Helm

Push the new bundle's images, write its values, and run the same
`helm upgrade --install` commands with its charts. `zone-server` applies the
migrations at startup.

## What isn't included

- **Ollama models.** Only the Ollama image is in the bundle. `ollama-init`
  and `make pull-models` download models from the internet, so on an
  air-gapped host they fail. Copy the models into the `zone_ollama_data`
  volume (Ollama keeps them under `/root/.ollama/models`) from a connected
  machine, or point `OLLAMA_BASE_URL` at an Ollama server inside your
  network.
- **ComfyUI.** The `bundled-comfyui` profile builds a CUDA image from source,
  and `comfyui-model-setup` downloads its models, so neither is in the bundle
  and neither works offline.
- **Web search.** SearXNG's image is loaded, but it searches the public web,
  so chat web search and page fetches return nothing on an air-gapped host.
- **Automatic license renewal**, described above.
- **The CloudNativePG operator**, for Helm installs.
