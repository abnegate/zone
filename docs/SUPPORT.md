# Support

> **DRAFT, to be confirmed.** The severity levels, the response and update
> targets, the business hours and the contact address below are a proposal.
> Nothing here is a commitment until this notice is removed.

Support depends on the edition a server runs, and every paid request is tied
to a license ID. This page covers who gets what, how to find the license ID,
what to send, and what support covers.

- [Support by edition](#support-by-edition)
- [Enterprise severity levels](#enterprise-severity-levels)
- [Your license ID](#your-license-id)
- [What to send](#what-to-send)
- [What's in scope](#whats-in-scope)
- [What's out of scope](#whats-out-of-scope)

## Support by edition

| Edition | Channel | Targets |
|---|---|---|
| **Enterprise** | Email to `<support address, to be confirmed>` | The [severity levels](#enterprise-severity-levels) below |
| **Team** | Email to `<support address, to be confirmed>` | Best effort, with no response target |
| **Community** | [GitHub issues](https://github.com/bitecodehq/zone/issues) | None. Maintainers and other users answer when they can |

A Team or Enterprise license gets its plan's support while it's active and
through its 30-day grace period, the same window in which it unlocks its
features. Once it has lapsed, the server runs Community and gets Community
support. See [LICENSING.md](LICENSING.md#editions) for the editions.

Don't report a security vulnerability in a public GitHub issue. Follow
[SECURITY.md](../SECURITY.md#reporting-security-issues) instead. Enterprise
customers can also raise one as a severity 1 or 2 request.

## Enterprise severity levels

You choose the severity when you open a request. If the impact turns out to be
different, we agree a new severity with you.

| Severity | Meaning | Examples |
|---|---|---|
| **1, critical** | Production Zone is down or unusable for everyone, or data is being lost, corrupted or exposed, and there is no workaround. | The server won't start after an upgrade. Nobody can sign in. A migration is failing part-way and leaving the database inconsistent. Stored credentials may have been exposed. |
| **2, high** | A major feature is broken for many people, or Zone runs degraded, and there is no reasonable workaround. | Agents fail to start in every workspace. Sign-in through your identity provider fails and only break-glass accounts get in. A valid Enterprise license can't pull LTS images or download a bundle. A bundle fails `install.sh verify`. |
| **3, medium** | A feature is partly broken, or a workaround exists. Production work continues. | One integration, such as GitHub sync, fails. Audit export is delayed. A console page shows wrong data. |
| **4, low** | A question, a documentation problem, a cosmetic issue or a feature request. | How to size a host. A wrong step in a guide. A layout glitch. |

### Response and update targets (DRAFT, to be confirmed)

Times are in business hours: **09:00 to 17:00 `<timezone, to be confirmed>`,
Monday to Friday, excluding public holidays in `<jurisdiction, to be
confirmed>`**. A business day is one such day.

| Severity | First response | Updates after that |
|---|---|---|
| **1, critical** | 4 business hours | Every business day until it's resolved or downgraded |
| **2, high** | 1 business day | Every 2 business days |
| **3, medium** | 2 business days | Every 5 business days |
| **4, low** | 5 business days | When the status changes |

- A response is a person replying about your request, not an automatic
  acknowledgement.
- These are response targets only. There is no guarantee of a fix or a
  resolution time. A fix ships in a new release, or as an LTS backport when it
  meets [the backport criteria](LTS.md#what-gets-backported-draft-to-be-confirmed).
- The clock starts when a request arrives with a valid license ID, so quote it
  every time.
- Put the severity in the subject line, for example `Sev 1: server won't
  start after upgrading to 1.4.3-lts`.

## Your license ID

Support is tied to the license ID, a UUID such as
`6f1c2a9e-3b4d-4e5f-8a7b-9c0d1e2f3a4b`. It is not secret. **Never send the
license key**: it unlocks the server and is also the password for LTS images
and bundle downloads. If you sent it by mistake, say so: we can cut off every
key on your subscription from LTS images and bundles and move you to a new
subscription and key, though a server holding the leaked key keeps running on
it until it expires.

A renewal, a seat change or a plan change issues a new license with a new ID.
Quote the ID of the license the server has installed now.

### In the console

1. Open **Settings → This server → License**. Only server admins see it.
2. The **License ID** row is the first detail. Click **Copy** next to it.

The row is shown only when a Team or Enterprise license verifies. If the page
warns that a stored key can't be verified, quote the ID from the key instead,
as below.

### On an air-gapped server

The same page works with no network access, because Zone verifies the license
on the server. Copy the ID from **Settings → This server → License** as above.

If the console isn't reachable, read the ID from the license key itself. A key
is two base64url parts joined by a dot; the first is the license's JSON
payload, which holds the `id`. With the key saved in `zone-license.key`:

```sh
payload=$(tr -d ' \t\r\n' < zone-license.key | cut -d . -f 1 | tr '_-' '/+')
while [ $(( ${#payload} % 4 )) -ne 0 ]; do payload="$payload="; done
printf '%s' "$payload" | base64 -d | grep -o '"id": *"[^"]*"'
```

It prints `"id":"<license ID>"`. The key's line breaks don't matter. The ID is
also the user name you log in with for [LTS images](LTS.md#pulling) and
[bundle downloads](AIRGAP.md#downloading), so it is already in your
`docker login`, netrc file or `zone-lts` pull secret if you use either.

## What to send

1. **The license ID**, as above.
2. **The version.** On Compose, `docker compose images manager console` shows
   the image tags, such as `1.4.3-lts`. The server reports its own version at
   `/health`:

   ```sh
   docker compose exec manager curl -fsS http://localhost:8000/health
   kubectl -n zone exec deploy/zone-apps-server -- curl -fsS http://localhost:8000/health
   ```

   On an LTS release, give the full `X.Y.Z-lts` tag. On an air-gapped install,
   the bundle's `VERSION` file holds it too.
3. **The topology**: single-node Compose, single-node Helm, or the HA reference
   architecture, plus anything you changed from the documented setup, such as
   an external database or a different ingress.
4. **Logs** from around the time of the problem, redacted as below:

   ```sh
   docker compose logs --since 2h --timestamps manager console litellm > zone-logs.txt
   kubectl -n zone logs deploy/zone-apps-server --since=2h --timestamps > zone-logs.txt
   ```

5. **What happened**: the steps that lead to the problem, what you expected,
   what you saw instead, when it started, and what changed just before, such as
   an upgrade, a new license or a configuration change.

### Redacting logs

Logs can carry email addresses, user and workspace names, file paths,
fragments of prompts, and in other services' logs, credentials. Before sending
them:

- Never send `.env`, your Helm values' secrets, Kubernetes secrets, or the
  license key.
- Replace tokens, keys, passwords and email addresses. This is a starting
  point, not a guarantee:

  ```sh
  sed -E \
    -e 's/[A-Za-z0-9_-]{20,}(\.[A-Za-z0-9_-]{20,})+/[TOKEN]/g' \
    -e 's/([Bb]earer|[Tt]oken|[Pp]assword|[Ss]ecret|[Aa]pi[_-]?[Kk]ey)([=: "]+)[^ ",]+/\1\2[REDACTED]/g' \
    -e 's/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/[EMAIL]/g' \
    zone-logs.txt > zone-logs.redacted.txt
  ```

- Read the result before you send it. No pattern catches everything, and you
  know what's confidential in your data better than a script does.
- Replace a name with the same placeholder everywhere it appears, such as
  `user-1`, so the log still shows which lines belong together.

## What's in scope

For Team and Enterprise:

- **The single-node Compose install** from `docker-compose.yml`, including an
  air-gapped install from a bundle ([AIRGAP.md](AIRGAP.md)).
- **The single-node Helm install** of the `zone-infra`, `zone-ai` and
  `zone-apps` charts ([the Helm quick start](../helm/QUICK_START.md)).
- **The HA reference architecture** in [HA.md](HA.md), deployed as described
  there.
- **LTS releases** during their support window ([LTS.md](LTS.md)), and the
  latest regular release. On an older regular release, we may ask you to
  upgrade first.
- Zone's own features, installation, upgrades, licensing, LTS image pulls and
  bundle downloads, and Zone's side of its integrations, such as single
  sign-on with your identity provider or GitHub sync.

## What's out of scope

- **Modified Zone**: forks, patched source, or images Zone didn't build. An
  image mirrored by digest from a bundle or the LTS registry is Zone's image.
- **Unsupported topologies**: anything other than the three installs above,
  such as several Compose hosts, a different orchestrator, or bundled
  components swapped for versions Zone doesn't pin.
- **Third-party model quality**: what a model says, how accurate or fast it is,
  and outages or limits at a model provider. We help when Zone sends a model
  the wrong request or handles its answer wrongly.
- **Your own infrastructure**: hosts, GPUs and drivers, networks, DNS, TLS
  certificates, proxies, firewalls, Kubernetes clusters, storage, container
  registries, identity providers and backups.
- **Third-party services' own faults**, such as GitHub, Polar or your identity
  provider.
- **Releases past their window**: an LTS line whose 12 months have ended, or a
  regular release older than the latest one once you've been asked to upgrade.
- **Recovering data with no backup.** See
  [backup and restore](OPERATIONS.md#backup-and-restore).
- **Custom work**: writing code, prompts or integrations for you, training, or
  on-site visits.
