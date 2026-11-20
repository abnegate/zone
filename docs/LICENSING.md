# Selling and issuing licenses

This page is for whoever sells Zone Enterprise: how a license works, how to
keep the signing key, how to set up Polar, and how to run `zone-license`, the
signer that turns a Polar payment into a license key in the customer's inbox.
If you run Zone and want to install a license you bought, you only need
[What the customer receives](#what-the-customer-receives).

- [Overview](#overview)
- [The signing key](#the-signing-key)
- [Setting up Polar](#setting-up-polar)
- [The webhook endpoint](#the-webhook-endpoint)
- [Deploying the signer](#deploying-the-signer)
- [SMTP](#smtp)
- [What the customer receives](#what-the-customer-receives)
- [Hand-signing a license](#hand-signing-a-license)
- [Rotating the signing key](#rotating-the-signing-key)
- [Why the signer listens only to `order.paid`](#why-the-signer-listens-only-to-orderpaid)
- [Failure modes](#failure-modes)
- [CI and publishing the image](#ci-and-publishing-the-image)

## Overview

Zone has two editions. **Community** is free and has no seat, workspace or
usage caps. **Zone Enterprise** is the one paid edition: any valid license
unlocks every Enterprise feature, such as [single sign-on](SSO.md),
[agent and model policy](POLICY.md), and [audit export, streaming and
retention](AUDIT.md). It's billed per seat, yearly, through Polar, which is the
merchant of record and handles VAT, GST and US sales tax. Zone never handles
money; it only verifies a key.

### How a license works

A license is a JSON payload, signed with Ed25519 and distributed as one string.
The payload holds:

| Field | Meaning |
|---|---|
| `id` | The license's UUID. |
| `customer` | The name shown under **Settings → This server → License**. |
| `seats` | The licensed seat count. |
| `issued_at` | When the license period starts. |
| `expires_at` | When it ends. |
| `key_id` | Which signing key signed it, such as `2026-1`. |

Every Zone release compiles in a keyring of trusted public keys, in
`runner/zone_enterprise/src/production.rs`. Zone verifies a license against
that keyring when an admin pastes it and whenever the server loads it. It never
contacts anyone to do so, so air-gapped servers work the same way.

- **Seats are soft.** A seat is a person who is an active member of an active
  organization and signed in during the current calendar month (UTC). Going
  over the licensed count shows admins a banner; nobody is ever refused
  sign-in because of it. The difference is trued up at renewal.
- **Expiry has a grace period.** For 30 days after `expires_at`, the license
  still unlocks everything. After that it has lapsed: single sign-on keeps
  working and existing policies stay enforced, but changing Enterprise
  settings, audit streaming and export stop until a new license is pasted. The
  signer never adds the grace period itself; Zone adds it when it reads
  `expires_at`.
- **A license can't be recalled.** Once a key is issued, it verifies until it
  lapses. See [Revocation](#revocation).

The console's License section shows a **Buy a license** link on Community, on
a lapsed license and on a stored key that no longer verifies. It points at
`https://zone.abnegate.dev/pricing`, a placeholder until the pricing page goes
live; the pricing page links to the Polar checkout.

### The licence boundary

| Path | What | License |
|---|---|---|
| `runner/zone_enterprise` | Enterprise features, the license payload, the verifier and signing | Zone Enterprise License |
| `manager/frontend/src/features/enterprise` | Enterprise console pages, including the License section | Zone Enterprise License |
| `runner/zone_license` | The `zone-license` signer: key generation, signing, the Polar webhook and email | MIT |
| Everything else | Community | MIT |

The root [LICENSE](../LICENSE) sets this out: everything outside the two Zone
Enterprise directories is MIT, so `runner/zone_license` is MIT without a
`LICENSE` file of its own.

`zone_license` links the `zone_enterprise` crate and builds every license
through its payload type and signing function, so the signer and the verifier
can't drift apart. A built `zone-license` binary therefore contains Zone
Enterprise code. The signer's source is public because the code isn't the
secret: the private signing key is.

## The signing key

The private key is the one thing that matters. Anyone who holds it can mint a
license that every Zone release in the field accepts.

### Generating a key

Build the signer and generate a key pair, from `runner`:

```sh
cargo run --release -p zone_license -- keygen --output zone-license-2026-2.key --key-id 2026-2
```

`keygen` writes the private key to a new file with mode `0600`, and refuses to
overwrite a file that already exists. It then prints what you need to trust
the key:

- the public key as a Rust byte array, ready to paste into `KEYS` in
  `runner/zone_enterprise/src/production.rs`;
- the public key in base64, to record beside the key id;
- the key id: the one you passed with `--key-id`, or else the next free one
  for the current year, such as `2026-2`. An id may use only letters, digits,
  `-`, `_` and `.`, because it's pasted into Rust source; `keygen` refuses any
  other before it generates or prints anything.

Without `--output`, the private key goes to standard output instead. Prefer
`--output`: it keeps the key out of terminal scrollback and out of a shell
redirect that might create a file others can read.

The private key file holds one line: the base64 of the 32-byte Ed25519 seed.

### Storing it

- Keep the live copy only as a secret of the deployed signer: your platform's
  secret manager, a Kubernetes secret, or a file readable by the signer's user
  alone.
- Keep one cold backup in a password manager or an offline vault, with its key
  id and base64 public key.
- Never commit it, paste it into an issue or chat, or put it on a command
  line. `zone-license` reads it only from a file or an environment variable,
  never from an argument, so it can't appear in shell history or `ps`.

### Trusting a new key

A key is only useful once Zone releases trust it:

1. Add the key id and the byte array from `keygen` to `KEYS` in
   `runner/zone_enterprise/src/production.rs`, beside the existing entries.
   Never replace one.
2. Update the tests in the same file that pin the keyring's contents.
3. Merge and ship a release.
4. Only then sign with it.

Both `sign` and `serve` refuse a key whose id isn't in the keyring of the
release they were built from, or whose public key doesn't match the one listed
under that id. That stops you from issuing a license that customers' servers
can't verify. It also means the signer must be built from the release that
first trusts its key, or a later one.

Customers' servers trust only the keys their own release shipped with. A
license signed with a key added in a new release doesn't verify on a server
still running an older one, so leave time between shipping a key and signing
with it.

## Setting up Polar

Try every step in the [Polar sandbox](https://sandbox.polar.sh) first, with a
sandbox webhook endpoint pointed at a signer you run for the purpose. The
signer has no test mode: it only signs with a key the production keyring
trusts, so every key it sends for a sandbox order is a real, valid license.
Put sandbox orders in your own name and address, and delete the sandbox
endpoint when you're done.

### 1. The organization

Create an organization in Polar and complete its onboarding and payout
details. If **Seat-based** doesn't appear as a pricing type in the next step,
ask Polar to enable seat-based pricing for the organization.

### 2. The product and price

Create one product:

| Setting | Value |
|---|---|
| Name | `Zone Enterprise` |
| Product type | Subscription |
| Billing cycle | Yearly |
| Pricing type | Seat-based (`amount_type: seat_based`) |
| Tiering model | Fixed price per seat, unless you want volume discounts |
| Price per seat | The yearly price for one seat |
| Benefits | None |

Don't add Polar's **License Keys** benefit. Its keys are validated by calling
Polar's API, which would make every Zone server phone home and would break
air-gapped installs. The key a customer installs is Zone's own signed license,
sent by the signer.

Polar's seat-based products let a billing manager assign seats to members and
send them invitations. Zone doesn't use that: Zone counts its own seats, and
the license carries only the number of seats paid for. Customers can ignore
seat assignment in Polar's portal.

Polar caps a subscription at 1,000 seats. Sell larger deals by invoice and
[hand-sign](#hand-signing-a-license) their keys.

Copy the product's ID. It's the value for `POLAR_PRODUCT_IDS`, which the
signer requires.

### 3. The minimum seat count

The minimum comes from the price, not the checkout. It's the first tier's
`min_seats` (reported as the price's `minimum_seats`), so to sell at least
five seats, start the first tier at 5. Checkout links have no minimum-seats
setting; a link can only fix the seat count, which suits a quoted deal but not
the self-serve link.

### 4. The proration setting

Polar decides what happens when a customer changes their seat count mid-year
from the organization's subscription proration setting. A seat change made
through the API can name its own proration; leave it on the organization
default or choose the same value.

**Set it to `invoice`.** It works on every Polar API version. Polar bills or
credits the change at once and pays an order with `billing_reason`
`subscription_update`, so the signer sends a key with the new seat count
straight away, for increases and decreases alike.

| Setting | Seat increase | Seat decrease | Use it? |
|---|---|---|---|
| `invoice` | New key at once | New key at once | **Yes** |
| `auto` (API version 2027-01) | New key at once | New key at the renewal | Acceptable where offered |
| `prorate` (Polar's default) | Seats change at once, but no order, so no key until renewal | Same | No |
| `next_period` | Waits a whole period | Waits a whole period | No |
| `reset` | In preview | In preview | No |

With `auto`, a decrease waits for the renewal key. That loosens nothing: the
customer keeps the larger key they already paid for until then. With
`prorate`, a customer who adds seats pays at once but gets no new key until
renewal, and Zone shows the over-seats banner until then. With `next_period`,
added seats wait a whole year.

Turn on the setting that lets customers update their subscription from the
customer portal if you want them to change seats themselves.

### 5. No free trial

Don't offer a trial on the product or the checkout link. A trial starts the
subscription with a `subscription_create` order while the subscription is
`trialing`; the signer ignores that order, so a trial customer gets no key
until the first paid renewal, when the trial ends. Offer evaluations with a
[hand-signed](#hand-signing-a-license) short license instead.

### 6. The checkout link

Create a checkout link for the Zone Enterprise product. Leave the seat count
open on the self-serve link, so buyers choose it in checkout within the
price's tiers, and point the pricing page's buy button at it. Every buyer must
give an email address; the key is sent there.

## The webhook endpoint

In Polar, open **Settings → Webhooks** and click **Add Endpoint**:

| Field | Value |
|---|---|
| URL | `https://<signer host>/webhooks/polar`, such as `https://licenses.example.com/webhooks/polar` |
| Format | **Raw** |
| API version | `2026-10` (the default) |
| Events | `order.paid` only |
| Secret | Generate one |

Copy the secret, which starts with `whsec_`, into the signer's
`POLAR_WEBHOOK_SECRET`. Polar changed how it signs with that secret on
8 September 2026; the signer accepts both forms, so a secret from either side
of that date works as it is.

The URL must be the final `https` address. Polar treats a redirect as a failed
delivery, so don't rely on an `http` to `https` redirect or a trailing-slash
redirect.

Subscribing to more events does no harm: the signer acknowledges anything that
isn't `order.paid` with `202` and does nothing else. Subscribing to
`order.paid` alone keeps the delivery log to what matters.

## Deploying the signer

The signer is one stateless container. It needs no database: a retried or
redelivered webhook produces a byte-identical key (see
[Retries are safe](#retries-are-safe)), so it keeps nothing between requests.
One instance is enough; run two behind a load balancer only if it has to
survive a host failure.

### Building and running the image

Build from the repository root:

```sh
docker build -f runner/zone_license/Dockerfile -t zone-license .
```

The image runs `zone-license serve` by default, as a non-root user, and
exposes port 8080. Put the settings below in an env file that only you can
read, then run it:

```sh
docker run -d --name zone-license --restart unless-stopped \
  --env-file /etc/zone-license/signer.env \
  -p 127.0.0.1:8080:8080 \
  zone-license
```

To give it the private key as a file instead of a variable, mount it read-only
and point `ZONE_LICENSE_PRIVATE_KEY_FILE` at it. The signer refuses a key file
that gives its group or everyone any access, and the container runs as uid
`10001`, so the file on the host must be owned by uid `10001` with mode `0400`
or `0600`:

```sh
sudo chown 10001:10001 /etc/zone-license/zone-license-2026-2.key
sudo chmod 0400 /etc/zone-license/zone-license-2026-2.key

docker run -d --name zone-license --restart unless-stopped \
  --env-file /etc/zone-license/signer.env \
  -e ZONE_LICENSE_PRIVATE_KEY_FILE=/run/secrets/zone-license-key \
  -v /etc/zone-license/zone-license-2026-2.key:/run/secrets/zone-license-key:ro \
  -p 127.0.0.1:8080:8080 \
  zone-license
```

On Kubernetes, pass the key as `ZONE_LICENSE_PRIVATE_KEY` from the secret
rather than mounting it as a file. The kubelet writes a mounted secret owned
by root, so uid `10001` can't read it with mode `0400`, and setting `fsGroup`
to let it in adds group read, which the signer refuses:

```yaml
env:
  - name: ZONE_LICENSE_PRIVATE_KEY
    valueFrom:
      secretKeyRef:
        name: zone-license
        key: private-key
```

The same image runs the other commands:

```sh
docker run --rm zone-license --help
```

### Environment

`zone-license serve` takes no flags. It reads its settings from the
environment and checks all of them at startup. Every missing or invalid
setting, and a key the release doesn't trust, is listed in one message naming
each variable, and the process exits with a non-zero status before it listens.
Values are trimmed, and a blank value counts as unset, except
`SMTP_PASSWORD`, which is taken exactly as given.

| Variable | Default | Meaning |
|---|---|---|
| `POLAR_WEBHOOK_SECRET` | Required | The endpoint's secret from Polar, starting `whsec_`. |
| `POLAR_PRODUCT_IDS` | Required | Comma-separated Polar product IDs to issue keys for, at least one. Orders for other products are acknowledged and ignored, so nothing else the organization sells, or adds later, ever gets a key. |
| `ZONE_LICENSE_PRIVATE_KEY_FILE` | None | Path to the private key file, which must give its group and everyone else no access. Set exactly one of this and `ZONE_LICENSE_PRIVATE_KEY`. |
| `ZONE_LICENSE_PRIVATE_KEY` | None | The private key itself: the base64 of the 32-byte seed. Set exactly one of this and `ZONE_LICENSE_PRIVATE_KEY_FILE`. |
| `ZONE_LICENSE_KEY_ID` | Required | The signing key's id, such as `2026-2`. It must name a key in this release's production keyring, and that key must match the private key. |
| `ZONE_LICENSE_LISTEN` | `0.0.0.0:8080` | The IP address and port to listen on, such as `127.0.0.1:8080` or `[::]:8080`. A host name is refused. |
| `SMTP_HOST` | Required | The mail relay. See [SMTP](#smtp). |
| `SMTP_PORT` | `587` | The relay's port, from 1 to 65535. |
| `SMTP_USER` | Required | The relay account. |
| `SMTP_PASSWORD` | Required | The relay password. It may be blank, but must be set. |
| `SMTP_FROM` | Required | The address keys are sent from: a bare address, such as `licenses@example.com`, without a name. |
| `SMTP_FROM_NAME` | `Zone` | The sender name, on one line. |
| `RUST_LOG` | `info` | The log filter. See [Logs](#logs). |

### Routes

| Route | Answers |
|---|---|
| `POST /webhooks/polar` | Polar's deliveries. See [Responses](#responses). |
| `GET /health` | `200` with `{"status":"ok"}` while the process is serving. It doesn't check the mail relay. |

Point your platform's health check or uptime monitor at `/health`. Only
`/webhooks/polar` needs to be reachable from Polar. Request bodies are capped
at 2 MB, far above any Polar event.

The image has no Docker `HEALTHCHECK`: the slim runtime image carries no HTTP
client to run one with. Have your orchestrator or load balancer probe
`GET /health` from outside the container instead, such as a Kubernetes
`httpGet` liveness and readiness probe on port 8080.

### Logs

`serve` writes one JSON object per line to standard error. Each delivery logs
one line with an `outcome` of `sent`, `ignored`, `unsigned`, `malformed`,
`unaddressed`, `misconfigured`, `unsignable`, `unsendable` or `undelivered`,
matching the [responses](#responses). Once the signature checks out, the line
carries the Polar `webhook` id and, once the order is read, its `order`,
`subscription` and `license` ids, or the `product` id for `misconfigured`. A
refused signature is logged at `info`, since anyone can post to the endpoint;
an order a paying customer is left waiting on is logged as an error. An ignored
delivery also logs the event type and the `reason`. A failed send logs what
went wrong (`rejected`, `response`, `client`, `tls`, `timeout` or
`connection`), the relay's SMTP status code if it gave one, and whether the
error was `retryable`. It never logs the text of the relay's reply, because
relays often quote the recipient's address in it.

The logs never contain the license key, the webhook secret, the private key,
the SMTP password, or the customer's name or email address. Set `RUST_LOG`,
such as `RUST_LOG=debug`, to change what's logged.

### Stopping

`serve` stops on `SIGTERM` or ctrl-c. It stops accepting connections, lets a
delivery in progress finish sending its email, and then exits, so
`docker stop` and a rolling deploy don't cut a send off half way. The image
runs the binary directly as PID 1, so the signer handles `SIGTERM` itself.

### TLS and DNS

The signer speaks plain HTTP. Give it a name such as `licenses.<your domain>`,
with an `A`, `AAAA` or `CNAME` record pointing at the host or platform, and
terminate TLS in front of it:

- a reverse proxy on the same host, such as Caddy or Traefik, forwarding to
  `127.0.0.1:8080`; or
- a platform that provides HTTPS itself, such as a container service with a
  managed certificate.

Polar's deliveries come from the internet, so the proxy must accept them
without a challenge. If the name is behind Cloudflare, turn off Bot Fight Mode
for it: Polar documents that it blocks webhook deliveries with `403`, and
allow-listing Polar's addresses doesn't help.

The proxy must pass the request body through unchanged. The signature covers
the exact bytes Polar sent, so anything that re-encodes the body makes every
delivery fail with `401`.

## SMTP

The signer sends mail through an SMTP relay, with the same variable names and
rules as Zone's own mail (see
[CONFIGURATION.md](CONFIGURATION.md#smtp_host-smtp_port-smtp_user-smtp_password-smtp_from-smtp_from_name)):

- `SMTP_HOST`, `SMTP_USER` and `SMTP_PASSWORD` are required.
  `SMTP_PASSWORD` is taken exactly as given, blank included, because a relay
  may accept an empty one.
- `SMTP_PORT` defaults to 587. Port 465 connects with implicit TLS; any other
  port must upgrade with STARTTLS before signing in. **Unlike Zone, a port
  that isn't a number from 1 to 65535 stops the signer** instead of falling
  back to 587, so a typo can't send keys to the wrong port.
- **`SMTP_FROM` is required.** Unlike Zone, the signer has no fallback sender
  address: a license email has to come from an address you own. It must be a
  bare address; the name goes in `SMTP_FROM_NAME`.
- `SMTP_FROM_NAME` defaults to `Zone`, and must be a single line.

A send that hasn't finished after 7 seconds counts as failed. That leaves room
inside Polar's 10-second delivery timeout to answer `503`, so that Polar
retries.

### Deliverability

A license email that lands in spam looks like a sale that never arrived. Send
from a domain you control and set it up properly:

- **SPF**: publish a TXT record on the sending domain that authorizes your
  relay, as your provider documents it.
- **DKIM**: turn on DKIM signing at the relay and publish the public key it
  gives you.
- **DMARC**: publish a policy once SPF and DKIM pass, starting with `p=none`
  while you read the reports.

Use a transactional mail provider rather than a personal mailbox, and send a
sandbox purchase to a Gmail and an Outlook address before going live.

## What the customer receives

When an order is paid, the customer gets an email at the address on the
order. It names the customer, the seat count and the expiry date, carries the
license key, and gives the steps to install it:

1. In Zone, open **Settings → This server → License**. Only server admins see
   it.
2. Paste the key.
3. Click **Add license** (**Replace license** if one is already installed).

Zone verifies the key on the spot and unlocks the Enterprise features in
place: the same image and the same data, with no restart or reinstall.

The name on the license is the order's billing name; failing that, the
customer's billing name, then the customer's name, then their email address.
Control and invisible characters are removed and runs of whitespace become one
space, and the email shows exactly the name the license carries.

| What happens in Polar | What the customer gets |
|---|---|
| A new subscription is paid | A key for the seats bought, expiring at the end of the first year. |
| A renewal is paid | A new key for the next year. The old key keeps working until it lapses, so the new one has to be pasted before the old one's grace period ends. |
| The seat count changes, with `invoice` proration | A new key with the new seat count and the same expiry, as soon as the change is billed. |
| A renewal payment fails | Nothing until Polar collects it. If it does, the key arrives then. |
| The subscription is cancelled | Nothing. The current key runs to its expiry and then its 30-day grace period. |
| The subscription is revoked or refunded | Nothing. The current key still runs to its expiry; see [Revocation](#revocation). |

A customer who loses the email can have it sent again: redeliver the order's
`order.paid` event from Polar (see [Redelivering an
event](#redelivering-an-event)), which sends the identical key.

## Hand-signing a license

`zone-license sign` signs a license directly, without Polar. Use it for design
partners, evaluations, deals invoiced outside Polar, and orders the webhook
couldn't handle.

```sh
ZONE_LICENSE_PRIVATE_KEY_FILE=/secure/zone-license-2026-2.key \
ZONE_LICENSE_KEY_ID=2026-2 \
zone-license sign --customer "Acme Ltd" --seats 10 --expires-at 2027-06-30
```

| Flag | Meaning |
|---|---|
| `--customer <name>` | Required. The name shown in Zone. |
| `--seats <n>` | Required. The licensed seat count. |
| `--expires-at <time>` | Required. An RFC 3339 timestamp, such as `2027-06-30T00:00:00Z`, or a date, such as `2027-06-30`, meaning midnight UTC. Don't add the grace period; Zone adds it. |
| `--key-id <id>` | The signing key's id. Defaults to `ZONE_LICENSE_KEY_ID`. |
| `--private-key-file <path>` | The private key file. Without it, the key comes from `ZONE_LICENSE_PRIVATE_KEY_FILE` or `ZONE_LICENSE_PRIVATE_KEY`. |
| `--issued-at <time>` | When the license period starts, in either form `--expires-at` takes. Defaults to now; leave it out for a new license. |
| `--id <uuid>` | The license id. Defaults to a random UUID; leave it out for a new license. |

It prints the license string and nothing else, so it can be piped or copied as
it is. The private key never goes on the command line, and `sign` refuses a
key the release's keyring doesn't trust, exactly as `serve` does.

Send the key to the customer yourself, with the installation steps from
[What the customer receives](#what-the-customer-receives).

For an evaluation, sign a short license, such as 30 days. It can't be
withdrawn early: it runs to its expiry, plus the grace period.

## Rotating the signing key

Rotate when the private key is lost, suspected leaked, or simply old.

Releases trust `2026-1` and `2026-2`, and the signer signs with `2026-2`. The
private key of `2026-1` was never recovered, so it stays trusted only so that
its licenses keep verifying. To move to another key:

1. Generate the new key:

   ```sh
   zone-license keygen --output zone-license-2027-1.key --key-id 2027-1
   ```

2. Paste the entry `keygen` prints into `production::KEYS` in
   `runner/zone_enterprise/src/production.rs`, after the last key. The keyring
   is a list, so licenses signed with an older key keep verifying.
3. Add the key and its base64 public key to `PUBLISHED` in the same file's
   tests, merge, and ship a release.
4. Store the new private key as the signer's secret, set
   `ZONE_LICENSE_KEY_ID` to the new id, and redeploy the signer built from that
   release.
5. Keep the cold backup of the new key.

Don't remove a key from `KEYS` because you've stopped signing with it. A lost
private key isn't a compromised one: a key whose private half was never
published is no risk, so keep it, and every license it signed keeps verifying.
Remove a key only when its private key has leaked, and only knowing what that
costs.

### Revocation

Verification is offline, so there's no revocation list and no way to recall a
single license:

- **One license** can't be withdrawn before it lapses, whatever happens to the
  subscription. Short terms are the only control.
- **A whole key** can be withdrawn by removing it from `KEYS`. Every license it
  signed then stops verifying on servers that upgrade to that release, and
  their Enterprise features stop as if unlicensed until a new key is pasted.
  Send every paying customer a replacement key signed with the new key first.
- **Servers that don't upgrade** keep trusting a removed key for as long as
  they run the old release, so a leaked key can sign licenses for them
  indefinitely.

Guard the private key accordingly: leaking it is the one failure that can't be
fully undone.

## Why the signer listens only to `order.paid`

The signer issues a key only when money has been collected, and it has no
database, so it must decide from one event alone.

It issues a key when all of these hold:

- the event is `order.paid`;
- the order's `paid` is true;
- its `billing_reason` is `subscription_create` (a new subscription),
  `subscription_cycle` (a renewal) or `subscription_update` (a seat change);
- its product is in `POLAR_PRODUCT_IDS`;
- its subscription is neither `trialing` nor `canceled`;
- it's for a seat-based subscription with at least one seat.

It ignores, with `202` and `{"ignored":"<reason>"}`, a one-off `purchase`
(`billing_reason`), a `subscription_meter_cycle` (`billing_reason`), an unpaid
order (`unpaid`), another product (`product`), an order for a `trialing`
subscription (`trial`), an order for a `canceled` subscription (`canceled`),
and every other event type (`event_type`).

A subscription set to cancel at the end of its period ends with a final
order. Polar marks the subscription `canceled` before creating it and reports
its `billing_reason` as `subscription_cycle`, the same as a renewal, and pays
it when there's anything left to bill, such as seats added under `prorate`.
The signer ignores it, because the subscription is over. A subscription that
is only due to cancel is still `active`, and its orders issue as usual.

An order for a product in `POLAR_PRODUCT_IDS` that has no subscription, or a
subscription without seats, isn't ignored: the customer has paid for a product
you said is Zone Enterprise and would otherwise get nothing. The signer
answers `422` and logs an error with the outcome `misconfigured`, naming the
order and the product. Fix the product's pricing so it's seat-based, then
[hand-sign](#hand-signing-a-stuck-order) the order's key.

Other events look tempting but can't be trusted alone:

- **`subscription.cycled`** fires when the period rolls over, before payment,
  whether or not the payment then succeeds. Issuing on it would give a year's
  key to a customer whose card was declined.
- **`subscription.updated`** fires on the period roll before payment, on a
  seat change, and on cancellation. It doesn't carry the previous seat count,
  so the signer can't tell those cases apart without a database.

Cancellation, revocation and refunds issue nothing. The current key runs to its
expiry and Zone's grace period follows.

### The key's terms

| License field | Taken from |
|---|---|
| `seats` | The seat count of the order's subscription, never the order's own `seats`, which Polar fills only for one-time orders |
| `expires_at` | The subscription's `current_period_end` in whole seconds, with no grace added |
| `issued_at` | The subscription's `current_period_start` in whole seconds |
| `customer` | The order's billing name, else the customer's billing name, else the customer's name, else their email |
| `id` | A UUIDv5 over the subscription, the period end and the seat count |
| `key_id` | `ZONE_LICENSE_KEY_ID` |

The key goes to the customer's email address on the order.

### Retries are safe

Every term comes from the order and its subscription period, and the license
id is derived from them, so the same order always produces the same,
byte-identical key. A retry or a redelivery sends the customer the same key
again, never a second license. A seat change within a period gets a new id,
because the seat count is part of it, and so does a renewal, because the
period end is.

If a send succeeds but Polar times out before it hears back, Polar retries and
the customer receives the same email twice. Both carry the identical key.

## Failure modes

### Responses

| Situation | Status | Body | What Polar does |
|---|---|---|---|
| Key signed and sent | `202` | `{"license":"<uuid>"}` | Records a success |
| Event or order deliberately ignored | `202` | `{"ignored":"<reason>"}` | Records a success |
| Signature missing, wrong, or outside the five-minute window | `401` | | Retries; counts as a failure |
| A body that isn't a Polar event (`malformed`), or an order with a subscription period that ends before it starts (`unsignable`) | `422` | | Retries; counts as a failure |
| An order without a customer email (`unaddressed`) | `422` | | Retries; counts as a failure |
| An order for a listed product without a seat-based subscription (`misconfigured`) | `422` | | Retries; counts as a failure |
| The email can never be sent as it is: the relay refused it for good with a `5xx` reply, or the order's address can't be parsed (`unsendable`) | `422` | | Retries; counts as a failure |
| The email couldn't be sent yet: the relay was unreachable, timed out, or refused it for now with a `4xx` reply (`undelivered`) | `503` | | Retries; counts as a failure |

The signer answers `2xx` for an issuing order only after the relay has
accepted the email. A `2xx` means the customer has their key, or the event
needed none.

Polar treats every non-`2xx` answer the same way, so the split between `422`
and `503` is for you, not for Polar: `422` means retrying the same delivery
can't help, and `503` means it might.

### Polar's delivery rules

- Any `2xx` is a success. Anything else, a redirect, or no answer within 10
  seconds is a failure.
- A failed delivery is retried, 10 attempts in all with exponential backoff,
  over roughly 35 to 70 minutes.
- Deliveries are kept for 90 days and can be redelivered from the dashboard.
- After 10 consecutive events fail permanently, Polar disables the endpoint
  and emails every member of the organization. A disabled endpoint receives
  nothing until it's turned back on.

Polar recommends answering within 2 seconds and may lower its timeout. The
signer answers only once the relay does, so use a relay that accepts mail
quickly.

Polar delivers an endpoint's events in order: while an earlier event is still
pending, later ones wait behind it, up to an age limit Polar sets. So one
order that keeps failing delays every later customer's key until it succeeds,
runs out of retries or ages out. Don't wait for that. As soon as the log shows
an order that can't be issued or sent, [hand-sign](#hand-signing-a-stuck-order)
its key with `zone-license sign` and send it yourself, then fix the cause, or
redeliver the order once it's fixed.

### What to do

| Symptom | Cause | Fix |
|---|---|---|
| The signer exits at startup | A required variable is missing or invalid, both or neither private key variables are set, the key file is too readable, or the key isn't in this release's keyring | Read the message, which names the variable. For an untrusted key, run a signer built from a release that trusts it. |
| Every delivery gets `401` | Wrong `POLAR_WEBHOOK_SECRET`, a proxy changing the body, or the host clock more than five minutes off | Copy the secret from the endpoint again, pass the body through unchanged, run NTP. |
| Deliveries get `503` | The relay is down, slow, or refusing mail for now | Fix the relay; the `undelivered` log line gives the failure and the SMTP status. Polar retries, and you can redeliver anything that ran out of attempts. |
| Deliveries get `422` with an `unsendable` log line | The relay refused the message for good, such as an unknown recipient or a refused sender, or the order's address can't be parsed | If the sender or the relay account is at fault, fix it and redeliver. If the recipient is, [hand-sign](#hand-signing-a-stuck-order) the key and send it to a corrected address. |
| Deliveries get `422` | The order can't be issued, usually because it has no customer email (an `unaddressed` error in the log, naming the order) | [Hand-sign](#hand-signing-a-stuck-order) it and send the key yourself. |
| A `misconfigured` error in the log | An order for a product in `POLAR_PRODUCT_IDS` had no seat-based subscription | Make the product seat-based, then [hand-sign](#hand-signing-a-stuck-order) the order the log names. |
| Deliveries get `403` | Cloudflare Bot Fight Mode or another bot filter | Turn it off for the signer's name. |
| The endpoint is disabled | 10 consecutive events failed | Fix the cause, [re-enable it](#re-enabling-the-endpoint), then redeliver. |
| A customer says no email arrived | Spam filtering, or a mistyped address | Check SPF and DKIM, redeliver the order, or hand-sign it to a corrected address. |

A `422` counts as a failure to Polar even though retrying can't fix it. A run
of bad orders, such as 10 in a row without an email, disables the endpoint,
and good orders after them go undelivered until you re-enable it. Watch for
Polar's disabled-endpoint email.

### Re-enabling the endpoint

1. Fix whatever made the deliveries fail, and check `GET /health` on the
   public URL.
2. In Polar, open **Settings → Webhooks**, open the endpoint, and enable it.
3. Redeliver every failed delivery since it was disabled.

### Redelivering an event

Open the endpoint's deliveries under **Settings → Webhooks**, find the failed
`order.paid` delivery, and redeliver it. Polar keeps deliveries for 90 days.
Because the key is deterministic, redelivering an order that already succeeded
sends the customer the same key again.

### Hand-signing a stuck order

For an order the webhook can't handle, such as one with no email, sign its key
yourself. Take the seat count and the subscription's current period from the
order in Polar:

```sh
zone-license sign \
  --customer "Acme Ltd" \
  --seats 5 \
  --issued-at 2026-11-18T11:59:58Z \
  --expires-at 2027-11-18T11:59:58Z
```

Use the period start and end exactly as Polar shows them, so the key covers
the same period the webhook's would have. Its license id differs from the one
the webhook would have derived, which does no harm: if the webhook later
succeeds for the same order, the customer gets a second key with the same
terms, and either works. Get the customer's address from Polar or the buyer,
and send the key with the installation steps.

## CI and publishing the image

The **License Signer** workflow, `.github/workflows/license-signer.yml`, runs
on changes to the signer. Its `test` job runs the `zone_license` tests, its
`clippy` job lints them, and its `image` job builds the Docker image and checks
that the binary starts in it.

Pushing a tag such as `license-signer-v1.0.0` also runs its `publish` job,
once the others pass, which pushes the image to
`ghcr.io/abnegate/zone-license` as `1.0.0` and `latest`. Only that job can
write packages. The image holds no secrets: the signing key, the webhook secret
and the SMTP password all reach the container at runtime.
