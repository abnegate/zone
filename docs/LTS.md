# Long-term support releases

> **DRAFT, to be confirmed.** The support window, the branch cadence and the
> backport criteria below are a proposal. Nothing here is a commitment until
> this notice is removed. No LTS branch has been cut yet.

Zone Enterprise includes long-term support (LTS) releases: a release line that
receives only security and data-safety fixes for a year, published as signed
images in a registry that only a valid Enterprise license can pull from. This
page covers the policy, how to pull and verify the images, how to run them
with Compose and Helm, and how maintainers cut a branch.

- [Policy](#policy)
- [Versions and tags](#versions-and-tags)
- [Images](#images)
- [Pulling](#pulling)
- [Verifying signatures](#verifying-signatures)
- [Running with Compose](#running-with-compose)
- [Running with Helm](#running-with-helm)
- [Running the runner](#running-the-runner)
- [Cutting a branch](#cutting-a-branch)
- [Releasing a backport](#releasing-a-backport)

## Policy

### Support window (DRAFT, to be confirmed)

- Every six months, a `release/X.Y-lts` branch is cut from the commit tagged
  `vX.Y.0`.
- Each LTS line is supported for **12 months** from its first LTS tag, so two
  lines overlap for six months and there is always time to move to the next
  one.
- When a line's 12 months end, it gets no more tags. Its images stay pullable
  by any license that may pull, but they get no further fixes.

### What gets backported (DRAFT, to be confirmed)

Only these changes go onto an LTS branch:

- **Security fixes** for Zone itself and for the dependencies it ships.
- **Data loss and corruption fixes**: anything that loses, corrupts or exposes
  stored data, including migrations that do so.
- **Base image CVE bumps**: moving a pinned base image, toolchain or bundled
  binary to a patched release of the same line.

New features, behaviour changes, refactors and ordinary bug fixes are not
backported; they ship in the next regular release and the next LTS line.

Every backport is a pull request into `release/X.Y-lts`, and it merges only
with CI green. Direct pushes to an LTS branch are refused by the branch
ruleset.

## Versions and tags

| What | Form | Example | Notes |
|---|---|---|---|
| Branch | `release/X.Y-lts` | `release/1.4-lts` | Cut from `vX.Y.0`. |
| Git tag | `vX.Y.Z-lts` | `v1.4.3-lts` | Must point at a commit on `release/X.Y-lts`. No leading zeros. |
| Image tag, immutable | `X.Y.Z-lts` | `1.4.3-lts` | Published once and never moved or overwritten. |
| Image tag, moving | `X.Y-lts` | `1.4-lts` | Always the newest release of the line. |
| Cargo version | `X.Y.Z` | `1.4.3` | What `zone-server --version` reports. |

**A note on semver.** `1.4.3-lts` is a semver *prerelease* string, so tools
that sort by semver place it before `1.4.3`. The suffix is used only as a git
tag and an image tag, never as a package version: the binaries inside carry
plain `X.Y.Z`. Compare LTS versions by line and patch number, not by semver
precedence.

The publishing workflow (`.github/workflows/lts.yml`) runs only for a
`vX.Y.Z-lts` tag. `scripts/lts-version.sh` refuses a malformed tag and a tag
whose commit is not on `release/X.Y-lts`, and the workflow refuses to replace
an `X.Y.Z-lts` image that is already published.

## Images

LTS images are published for `linux/amd64` and `linux/arm64` to
`registry.bitecode.ltd`:

| Image | Built from | Compose service | Helm value |
|---|---|---|---|
| `registry.bitecode.ltd/zone/server` | `manager/Dockerfile`: the Zone server, plus the runner at `/app/zone-runner` | `manager` | `zone-apps` `server.image` |
| `registry.bitecode.ltd/zone/manager` | `manager/frontend/Dockerfile`: the console | `console` | `zone-apps` `manager.image` |
| `registry.bitecode.ltd/zone/litellm` | `litellm/Dockerfile`: Zone's LiteLLM wrapper | `litellm` | not used; `zone-ai` runs upstream LiteLLM |

Every other image Zone runs, such as PostgreSQL, Valkey, Traefik and Ollama,
still comes from its usual public registry. ComfyUI is not part of LTS.

The registry is read-only: it serves pulls and nothing else. Every pull is
recorded with the license ID that made it.

## Pulling

Any Zone Enterprise license that is active, or within its 30-day grace period,
may pull. Team and Community cannot, and neither can a lapsed or revoked
license.

- **User name:** the license ID, the UUID shown under
  **Settings → This server → License**.
- **Password:** the license key itself.

### Docker

```sh
printf '%s' "$ZONE_LICENSE_KEY" \
  | docker login registry.bitecode.ltd -u 3f0b8c2e-1d5a-4c9e-9b7f-6a2d4e8c1f03 --password-stdin
docker pull registry.bitecode.ltd/zone/server:1.4-lts
```

Docker stores the credential and exchanges it for a short-lived pull token on
each pull. Read the key from a file or a secret manager rather than typing it
on the command line, where it lands in shell history.

### Kubernetes

Create an image pull secret in each namespace that runs Zone:

```sh
kubectl create secret docker-registry zone-lts \
  --namespace zone \
  --docker-server=registry.bitecode.ltd \
  --docker-username=3f0b8c2e-1d5a-4c9e-9b7f-6a2d4e8c1f03 \
  --docker-password="$ZONE_LICENSE_KEY"
```

Then reference it from the pods, as in [Running with Helm](#running-with-helm).

### containerd and nerdctl

containerd, and so nerdctl and the kubelet on containerd nodes, first POSTs to
the registry's token endpoint and falls back to GET when the POST is refused.
The endpoint answers the POST with `405 Method Not Allowed` and the GET with
the token, so these clients work unchanged; each new token costs one extra
round trip.

### After a renewal

A renewal, a seat change or a plan change issues a new license with a new
license ID and a new key. The old key, with the old ID, keeps pulling until its
own grace period ends, 30 days after its expiry, and then stops. Automatic
renewal updates the license inside Zone only: it cannot reach your Docker
credential store or your Kubernetes secrets. Before the old key's grace period
ends, read the new ID under **Settings → This server → License**, run
`docker login` again with the new ID and key, and replace each `zone-lts`
secret:

```sh
kubectl create secret docker-registry zone-lts \
  --namespace zone \
  --docker-server=registry.bitecode.ltd \
  --docker-username="$ZONE_LICENSE_ID" \
  --docker-password="$ZONE_LICENSE_KEY" \
  --dry-run=client --output yaml \
  | kubectl apply -f -
```

## Verifying signatures

Every image index is signed keylessly with [cosign](https://docs.sigstore.dev)
by the publishing workflow, using GitHub's OIDC identity for the tag that
published it. The workflow verifies each signature through a second registry
address before it finishes, so a signature never depends on the address it was
made through. To verify an image yourself, sign in with `docker login` first,
since cosign reads Docker's credentials, then:

```sh
cosign verify \
  --certificate-identity https://github.com/bitecodehq/zone/.github/workflows/lts.yml@refs/tags/v1.4.3-lts \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  registry.bitecode.ltd/zone/server:1.4.3-lts
```

The moving `X.Y-lts` tag was signed by whichever patch release last moved it,
so match the identity by pattern instead:

```sh
cosign verify \
  --certificate-identity-regexp '^https://github\.com/bitecodehq/zone/\.github/workflows/lts\.yml@refs/tags/v1\.4\.[0-9]+-lts$' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  registry.bitecode.ltd/zone/server:1.4-lts
```

Use cosign v3.1.4 or later. Pin by digest in production: the job summary of
each release run lists every index digest.

## Running with Compose

LTS needs Docker Compose 2.24 or later, for the `!reset` tag that stops
Compose building the Zone images from source.

1. Check out the matching tag, so the Compose files and the templates they
   mount match the images: `git checkout v1.4.3-lts`.
2. Sign in, as in [Pulling](#pulling).
3. Add the line to `.env`, either the moving tag or a pinned release:

   ```sh
   ZONE_LTS_VERSION=1.4-lts
   ```

4. Start Zone the usual way, with `make up` or `./scripts/compose.sh up -d`.

`make up` and `scripts/compose.sh` add `docker-compose.lts.yml` automatically
whenever `ZONE_LTS_VERSION` is set, after the profile overlays and before
`docker-compose.override.yml`, so profiles such as `vpn` and `monitoring` work
as before. That file points `manager`, `console` and `litellm` at the LTS
images, removes their `build` sections and sets `pull_policy: always`, so
every `up` checks the registry for a newer `X.Y-lts`. `ZONE_LTS_REGISTRY`
replaces `registry.bitecode.ltd`, for example with a mirror.

With a bare `docker compose`, name the files yourself:

```sh
docker compose -f docker-compose.yml -f docker-compose.lts.yml up -d
```

Don't copy `docker-compose.lts.yml` over `docker-compose.override.yml`; keep
the override file for your own changes. Without `ZONE_LTS_VERSION`, the LTS
file refuses to render rather than guess a version.

## Running with Helm

Only `zone-apps` runs Zone-built images. Its `global.imageRegistry` is joined
to each repository as it is, so it needs the trailing slash:

```yaml
global:
  imageRegistry: registry.bitecode.ltd/
  imagePullSecrets:
    - name: zone-lts

server:
  image:
    tag: 1.4.3-lts

manager:
  image:
    tag: 1.4.3-lts
```

`manager.image` is the console (`zone/manager`). `zone-infra` and `zone-ai`
run only third-party images, so they need no changes for LTS.

## Running the runner

The runner ships inside `zone/server`, at `/app/zone-runner`:

```sh
docker run --rm --entrypoint /app/zone-runner \
  registry.bitecode.ltd/zone/server:1.4-lts --help
```

Standalone LTS runner binaries are out of scope: the binaries on GitHub
releases come from regular releases only.

## Cutting a branch

For maintainers. Every six months (DRAFT), from the `vX.Y.0` commit:

```sh
git fetch origin --tags
git switch --create release/1.4-lts v1.4.0
git push origin release/1.4-lts
git tag --sign v1.4.0-lts --message 'Zone 1.4.0 LTS' v1.4.0
git push origin v1.4.0-lts
```

Pushing the tag starts the **LTS** workflow:

1. **Check the tag** runs `scripts/lts-version.sh` against the fetched
   `release/*-lts` branches and stops unless the tag is `vX.Y.Z-lts` and its
   commit is on `release/X.Y-lts`.
2. **Image** builds each image natively on amd64 and arm64 and pushes it by
   digest to the registry's storage.
3. **Manifest, tags and signatures** joins the two platforms into one index
   per image, refuses an `X.Y.Z-lts` tag that already holds other builds, tags
   `X.Y.Z-lts` and `X.Y-lts`, signs each index, verifies each signature, and
   lists the digests in the job summary.
4. **Bundle** builds the [air-gapped bundle](AIRGAP.md) for the release.

[DISTRIBUTION.md](DISTRIBUTION.md) covers the registry, its storage and the
token endpoint behind it.

The image, manifest and bundle jobs run in the `distribution` environment, so
each waits for a reviewer's approval. The workflow needs these repository
variables:

| Variable | Meaning |
|---|---|
| `ZONE_DISTRIBUTION_ROLE` | The IAM role GitHub assumes through OIDC to write the registry's storage. |
| `ZONE_DISTRIBUTION_BUCKET` | The S3 bucket behind the registry. |
| `ZONE_DISTRIBUTION_REGION` | That bucket's region. |
| `ZONE_LTS_RUNNER_AMD64` | Optional. The amd64 runner label, by default `ubuntu-24.04`. |
| `ZONE_LTS_RUNNER_ARM64` | Optional. The arm64 runner label, by default `ubuntu-24.04-arm`. |

No secrets are needed: AWS credentials come from OIDC and last only for the
job.

## Releasing a backport

1. Branch from `release/X.Y-lts`, cherry-pick the fix from `main` with
   `git cherry-pick -x`, and open a pull request into `release/X.Y-lts`.
2. Merge it once CI is green and the change meets
   [the backport criteria](#what-gets-backported-draft-to-be-confirmed).
3. Tag the merge commit with the next patch, `vX.Y.(Z+1)-lts`, and push the
   tag.

A tag that fails can be fixed and retagged under the next patch number. An
`X.Y.Z-lts` image that was published is never replaced.
