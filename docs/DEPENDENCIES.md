# Dependency checks

CI uses cargo-deny 0.20.2 against the locked Rust workspace. A configuration or
scan failure fails the job. Run the same checks locally from `runner`:

```sh
python3 ../scripts/test-dependencies.py
cargo deny --locked check --warn unmaintained --warn notice
```

The command preserves the existing warning policy for unmaintained and notice
advisories. Vulnerabilities, unlicensed crates and licenses outside the explicit
policy remain errors. The removed `vulnerability`, `notice`, `unlicensed` and
`copyleft` configuration fields are unsupported by cargo-deny 0.20.2. See its
[advisory configuration](https://embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html),
[license configuration](https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html)
and [lint overrides](https://embarkstudios.github.io/cargo-deny/cli/check.html).

## Reviewed license scope

The global license allowlist is unchanged. The following exceptions cover only
the exact versions already in `runner/Cargo.lock`, inspected on 2026-09-09. They
make previously undeclared dependency coverage explicit; they are not blanket
license approval. New packages, versions or license expressions must pass the
policy again. In cargo-deny 0.20.2, `crate@version` selects an exact version.

| Package and version                                    | SPDX expression              | Dependency path                                                     | Inspected packaged evidence                                                                 |
| ------------------------------------------------------ | ---------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| cssparser 0.36.0, 0.37.0, 0.38.0                       | MPL-2.0                      | Tauri/dom_query and scraper                                         | Cargo.toml and LICENSE                                                                      |
| cssparser-macros 0.6.1, 0.7.0                          | MPL-2.0                      | cssparser                                                           | Cargo.toml and LICENSE                                                                      |
| dtoa-short 0.3.5                                       | MPL-2.0                      | cssparser                                                           | Cargo.toml and LICENSE                                                                      |
| selectors 0.36.1, 0.38.0, 0.41.0                       | MPL-2.0                      | Tauri/dom_query and scraper                                         | Cargo.toml and lib.rs MPL-2.0 notice                                                        |
| option-ext 0.2.0                                       | MPL-2.0                      | dirs-sys through dirs/hf-hub                                        | Cargo.toml and LICENSE.txt                                                                  |
| libfuzzer-sys 0.4.13                                   | (MIT OR Apache-2.0) AND NCSA | rav1e's cfg(fuzzing) dependency through ravif/image                 | Cargo.toml, LICENSE-MIT, LICENSE-APACHE and README's NCSA attribution for libfuzzer sources |
| quoted_printable 0.5.2                                 | 0BSD                         | lettre                                                              | Cargo.toml and LICENSE                                                                      |
| webpki-root-certs 1.0.11, webpki-roots 0.26.11, 1.0.11 | CDLA-Permissive-2.0          | rustls-platform-verifier/reqwest, ureq/hf-hub and tokio-tungstenite | Cargo.toml and LICENSE                                                                      |
| xxhash-rust 0.8.18                                     | BSL-1.0                      | redis                                                               | Cargo.toml and LICENSE                                                                      |
| zone_enterprise (workspace member)                     | LicenseRef-Zone-Enterprise   | first-party; zone_server, zone_license                              | runner/zone_enterprise/Cargo.toml and LICENSE                                               |

The first-party `zone_enterprise` exception is selected by name, not
`crate@version`, because its version follows the workspace.

The prior `copyleft = "warn"` expressed a warning policy for MPL dependencies.
The NCSA, 0BSD, CDLA-Permissive-2.0 and BSL-1.0 entries are newly explicit,
version-scoped policy decisions for existing dependencies. Their upstream source
is [libfuzzer](https://github.com/rust-fuzz/libfuzzer/tree/719e4efb9b8857ebaa782ae59376c8cbb78fed0f),
[quoted-printable](https://github.com/staktrace/quoted-printable/tree/b22a680c5ebe97c2f1e5d41b2017839c73508c7e),
[webpki-roots](https://github.com/rustls/webpki-roots/tree/0a553dbc8b3f18ea05c4f881cffa3f2d005d0d30)
and [xxhash-rust](https://github.com/DoumanAsh/xxhash-rust/tree/f93abc7ce0363e6910601e89ada2240f9ed7def6),
pinned by the downloaded crates' `.cargo_vcs_info.json`. Package checksums remain
pinned by Cargo.lock. Replacing these dependencies would change email, parsing,
desktop, TLS, image or Redis behavior beyond this CI configuration repair. No
dependency version, feature, source or lockfile changes accompany these entries.

`scripts/test-dependencies.py` runs the real checker against temporary crates: a
reviewed name/version/license passes, while a new version, another package,
another license and absent licensing all fail. The full workspace scan separately
checks the actual dependency graph.

## Locked but unbuilt libsqlite3-sys

`runner/.cargo/audit.toml` ignores RUSTSEC-2026-0459 (libsqlite3-sys before
0.39.0). libsqlite3-sys 0.37.0 is in Cargo.lock only through the optional sqlite
backend of sqlx-macros-core, which no feature in this workspace enables, and
sqlx 0.9.0 caps it below 0.38.0, so it cannot move to the fixed release.
cargo-audit reads the lockfile and can ignore only by advisory ID; cargo-deny
walks the built graph and needs no exception, so the Dependency Check job fails
if sqlite is ever enabled. Check from `runner` that it stays unbuilt:

```sh
cargo tree --workspace --all-features --target all -e all -i libsqlite3-sys
```

It reports nothing to print. Remove the ignore once a sqlx release accepts libsqlite3-sys
0.39.0 or later.

## Patched without a policy exception

Two advisories were fixed directly rather than ignored, since both had a
same-series patch already permitted by every dependent's version requirement:
`cargo update -p rustix` moved the lockfile from 1.1.4 to 1.1.6 (RUSTSEC-2026-0486,
an xattr function marking unwritten bytes as initialized), and
`cargo update -p lettre` moved it from 0.11.23 to 0.11.24 (RUSTSEC-2026-0560
through -0565, real vulnerabilities, not just informational advisories).

Separately, `zvariant_derive` (RUSTSEC-2026-0372, unmaintained, no patched
version) and `glib` (RUSTSEC-2024-0429, unsound, fixed only in >=0.20.0 which no
dependent in the graph permits yet) also appear in `cargo audit`/`cargo deny`
output. Neither needs a policy exception: both are informational-severity
advisories that neither tool fails the build on by default, confirmed by
running `cargo audit` and `cargo deny check` locally with no ignore entry for
either ID.

## jsonwebtoken crypto backend

`jsonwebtoken` 11.0.0 is built with `aws_lc_rs` only, the same aws-lc-rs that
rustls already links, so no new native build requirement comes with it. Its
`rust_crypto` backend would bring back `rsa` (RUSTSEC-2023-0071, no fixed
release), which the GitHub App's RS256 signing in
`runner/zone_server/src/services/github_app/jwt.rs` would exercise with a private
key, and `crypto-bigint` 0.5 through p256/p384 (RUSTSEC-2026-0387). Enabling both
features also makes jsonwebtoken panic on its first sign or verify, because it
cannot pick a default provider.

Check from `runner` that neither crate comes back:

```sh
cargo tree --workspace --all-features --target all -e all -i rsa
cargo tree --workspace --all-features --target all -e all -i crypto-bigint
```

Both report no match.

## Frontend advisory exception

`GHSA-gcqx-w48m-w2h4` is excepted in `manager/frontend`'s audit script. The
advisory describes prototype poisoning in deepmerge's `mergeObject()` when a
caller merges an attacker-supplied object, and covers every release from 4.2.0
through 4.3.1, the latest, so there is nothing to upgrade to.

The single path is `vite-plugin-pwa > workbox-build >
@rollup/plugin-node-resolve > deepmerge`, a build-time devDependency that is
never bundled into the manager frontend. Its one call merges the plugin's own
constant defaults into an empty object when the module loads; no
configuration, package metadata or request data reaches it.

This exception must be revisited if deepmerge publishes a patched release, if
the plugin starts merging caller-supplied objects, or if deepmerge reaches a
runtime dependency. All other frontend advisories remain fatal.

## Cross-major frontend overrides

The root `package.json` pins `glob` and `zod` across every workspace. Bun's
`overrides` apply to the whole tree, so each pin also lands on one package
built for an older major. Both pins are needed for `bun run audit` in
`manager/frontend` to pass. The `glob` pin also reaches `nyc` and
`vite-plugin-pwa > workbox-build`, which runs in the console's PWA build, so
`bun run build` must keep passing against it. The `zod` pin reaches only the
Lighthouse CI tooling (`@lhci/cli`), never the bundled console.

**`glob` 14.0.3.** Without it, `nyc > glob@13` and `vite-plugin-pwa >
workbox-build > glob@11` fail the audit on `GHSA-3g3m-2fjr-6pw4` (AbortSignal
cancellation bypass, `>=9.0.0 <=14.0.1`). The pin also gives `@lhci/cli >
chrome-launcher@0.13.4 > rimraf@3`, which expects glob 7, glob 14. rimraf
only calls glob when a path has glob magic. chrome-launcher removes the
`mktemp -d -t lighthouse.XXXXXXX` profile directory it created with the async
call, and that path has none, so glob 14's `hasMagic` answers and rimraf
removes the directory itself. `rimraf.sync` with a pattern also works, because
glob 14 still has `sync`. The error path differs: an async rimraf on a path
with glob magic, for example a `TMPDIR` containing `*` or `[`, calls
`glob(...)`, which is not a function in glob 14, and throws `TypeError` out of
an fs callback instead of deleting the directory.

**`zod` 4.6.7.** The console depends on zod 4 directly. Without the override,
`@lhci/cli > lighthouse > puppeteer-core > chromium-bidi@12` resolves zod
3.25.76, which fails the audit on `GHSA-mrhx-c7xc-m3w5` (memory exhaustion on
oversized constrained collections, `<4.6.6`); no zod 3 release is patched.
puppeteer-core loads chromium-bidi only for a WebDriver BiDi connection, and
Lighthouse drives Chrome over CDP, so the module is never loaded. If it were,
valid messages would parse as before, but the error path differs:
`parseObject` reads `parseResult.error.errors`, which zod 4 removed in favour
of `issues`, so a malformed BiDi message would throw `TypeError` instead of
`InvalidArgumentException`.

Drop either pin once a release outside the advisory's range satisfies every
dependent: `nyc` and `workbox-build` on glob 14.0.2 or later, and
`chromium-bidi` on zod 4. Revisit both if Lighthouse starts using WebDriver
BiDi or if chrome-launcher is called with a user-supplied profile path.

## Syslog TLS

`zone_server` depends directly on `rustls` 0.23.46 (no default features;
`aws-lc-rs`, `std` and `tls12`), `tokio-rustls` 0.26.4 (no default features)
and `rustls-platform-verifier` 0.7.0, each pinned exactly, to deliver audit
events to syslog over TLS (see [AUDIT.md](AUDIT.md#syslog)). All three were
already in `runner/Cargo.lock` at those versions through reqwest, and they use
the aws-lc-rs provider reqwest and jsonwebtoken already link, so no package,
version or license changes. The `audit` feature of `zone_enterprise` adds
`hex` 0.4.3 and `hmac` 0.13.0 for webhook signatures and SigV4, also already
locked at those versions.

## License signer

`zone_license` links `zone_enterprise` with its `signing` feature, the reason
`zone_enterprise`'s exception above names it. It depends directly on `lettre`
0.11.23 with the same features `abnegate-notify` enables (`builder`,
`hostname`, `smtp-transport` and `tokio1-native-tls`, no defaults), to send a
license email with both text and HTML parts, and on `hmac` 0.13.0 and `sha2`
0.11.0 to verify Polar's webhook signatures. All three were already locked at
those versions, so no package, version or license changes. See
[LICENSING.md](LICENSING.md).

## aiusg

`aiusg` 0.5.0 reads each signed-in Claude and ChatGPT login's plan usage, so
agent routing can tell a limited login from one with headroom. It is pinned
exactly in `runner/zone_server/Cargo.toml` with `default-features = false`,
which leaves out its `cli` feature and everything only the command line needs:
`clap`, `crossterm`, `webbrowser`, `rmcp`, `schemars`, `keyring` and
`rusqlite`. The library half brings in `async-compression` 0.4.50,
`compression-codecs` 0.4.45 and `compression-core` 0.4.33, through the `gzip`
feature it enables on reqwest; no locked version changes.

Check from `runner` that the command line half stays out:

```sh
cargo tree -p aiusg -e normal --prefix none | grep -E '^(crossterm|webbrowser|rusqlite|keyring|clap|rmcp|schemars) '
cargo tree -p zone_server -i rusqlite
```

The first prints nothing and the second reports no match. Enabling any aiusg
feature must pass that check again.

## SAML implementation

SAML sign-in in `zone_enterprise` (see [SSO.md](SSO.md#saml-20)) uses its
own narrow implementation of the XML parsing, exclusive canonicalization and
XML signature verification that SAML needs, in
`runner/zone_enterprise/src/sso/saml`. It adds five direct dependencies to
the `sso` feature, each pinned exactly:

| Package and version | SPDX expression | Used for |
|---|---|---|
| quick-xml 0.41.0 | MIT | The XML tokenizer under Zone's own tree builder and namespace resolution |
| rustls-webpki 0.103.16 (no default features; `std`) | ISC | Reading the subject public key of an identity provider's X.509 certificate |
| rustls-pki-types 1.15.1 | MIT OR Apache-2.0 | `CertificateDer`, which rustls-webpki takes and doesn't re-export |
| aws-lc-rs 1.18.1 (default features) | ISC AND (Apache-2.0 OR ISC) | RSA PKCS#1 v1.5 verification with SHA-256, SHA-384 and SHA-512, the digests, constant-time comparison and the RSA key size check |
| flate2 1.1.10 | MIT OR Apache-2.0 | Raw DEFLATE for the HTTP-Redirect binding |

All five were already in `runner/Cargo.lock` at those versions: quick-xml
through plist, flate2 through png, tiff, ureq and compression-codecs, and
aws-lc-rs, rustls-webpki and rustls-pki-types through rustls, reqwest and
jsonwebtoken. The lockfile change only adds dependency edges: the five
above to `zone_enterprise`, and a `flate2 = "=1.1.10"` dev-dependency to
`zone_server`, whose integration tests inflate the AuthnRequest that Zone
sends. No package, version or license changes, so the license policy above
needs no new entry.
`base64`, `chrono`, `rand`, `reqwest` and `url` were already `sso`
dependencies.

### Advisories

- quick-xml 0.41.0 is the release patched for RUSTSEC-2026-0194 and
  RUSTSEC-2026-0195; both advisories list `>= 0.41.0` as patched.
- rustls-webpki 0.103.16 is patched for RUSTSEC-2026-0382.
- The RustSec advisory database (updated 2026-11-19) and the GitHub
  advisory database (queried 2026-11-21) have no advisories for any of the
  rejected candidates below or for roxmltree.

### Candidates not used

| Candidate | Why not |
|---|---|
| [samael](https://crates.io/crates/samael) 0.0.22 | Its default `xmlsec` feature pulls in openssl, openssl-sys, bindgen and libxml 0.3, plus the system libraries libxml2 and xmlsec1. That would mean `libxmlsec1-dev`, `libxml2-dev` and clang on the Ubuntu CI runners, Homebrew packages on the macOS job and runtime libraries in the manager image. [Issue #82](https://github.com/njaremko/samael/issues/82) reports that signature verification fails with xmlsec 1.2.3x because `xmlSecAddIDs` isn't registered. It is still on 0.0.x. |
| [saml-rs](https://crates.io/crates/saml-rs) 0.8.0 | Created 2026-06-14, with eight releases in five months, 7 stars and 84 open issues. It depends on bergshamra. |
| [bergshamra](https://crates.io/crates/bergshamra) 0.12.0 | Created 2026-02. Its default features pull in clap, PKCS#11 and post-quantum algorithms, and it brings its own XML stack (`uppsala`, `kryptering`): a large and new attack surface. |
| [xml_c14n](https://crates.io/crates/xml_c14n), [libxml](https://crates.io/crates/libxml) | Wrappers around the system libxml2. |
| [xml-canonicalization](https://crates.io/crates/xml-canonicalization) 0.1.2 | Built on pest and regex, about 9,000 downloads, and no exclusive canonicalization guarantees. |
| [roxmltree](https://crates.io/crates/roxmltree) | Refuses DTDs by default, but has no public accessor for an element's prefix, which exclusive canonicalization has to render. |

### Namespace resolution

Zone reads XML with quick-xml's plain `Reader`, not its `NsReader`, and
resolves namespaces with its own scope stack. quick-xml 0.41.0's
`NamespaceResolver` counts open elements in a `u16` (`nesting_level` in
`src/name.rs`) and increments it with `+= 1`, so a document nested more than
65,535 elements deep panics in a debug build and wraps around in a release
build, where the resolver then loses track of which declarations are in
scope. Zone's
resolver has no such limit, and every walk over the tree, from building it to
canonicalizing it, is iterative, so a deeply nested document can't overflow
the stack either. The SAML tests parse and canonicalize a document nested
100,000 elements deep.
