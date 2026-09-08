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

| Package and version                         | SPDX expression              | Dependency path                                     | Inspected packaged evidence                                                                 |
| ------------------------------------------- | ---------------------------- | --------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| cssparser 0.36.0, 0.37.0                    | MPL-2.0                      | Tauri/dom_query and scraper                         | Cargo.toml and LICENSE                                                                      |
| cssparser-macros 0.6.1, 0.7.0               | MPL-2.0                      | cssparser                                           | Cargo.toml and LICENSE                                                                      |
| dtoa-short 0.3.5                            | MPL-2.0                      | cssparser                                           | Cargo.toml and LICENSE                                                                      |
| selectors 0.36.1, 0.38.0                    | MPL-2.0                      | Tauri/dom_query and scraper                         | Cargo.toml and lib.rs MPL-2.0 notice                                                        |
| option-ext 0.2.0                            | MPL-2.0                      | dirs-sys through dirs/hf-hub                        | Cargo.toml and LICENSE.txt                                                                  |
| libfuzzer-sys 0.4.13                        | (MIT OR Apache-2.0) AND NCSA | rav1e's cfg(fuzzing) dependency through ravif/image | Cargo.toml, LICENSE-MIT, LICENSE-APACHE and README's NCSA attribution for libfuzzer sources |
| quoted_printable 0.5.2                      | 0BSD                         | lettre                                              | Cargo.toml and LICENSE                                                                      |
| webpki-root-certs 1.0.9, webpki-roots 1.0.9 | CDLA-Permissive-2.0          | rustls-platform-verifier/reqwest and ureq/hf-hub    | Cargo.toml and LICENSE                                                                      |
| xxhash-rust 0.8.18                          | BSL-1.0                      | redis                                               | Cargo.toml and LICENSE                                                                      |

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

## Existing RSA advisory exception

`RUSTSEC-2023-0071` is already excepted in `runner/.cargo/audit.toml`; cargo-deny
now applies the same exception with a current rationale. The locked RSA 0.9.10
paths are jsonwebtoken 11.0.0 through zone_server and jsonwebtoken 10.4.0 through
octocrab 0.54.1 in zone_context. Server JWT creation and validation use
`Header::default()` / `Validation::default()` (HS256) with secret-based keys in
`runner/zone_server/src/auth/jwt.rs`; they do not use RSA private keys. Octocrab
has no source callers in zone_context. The earlier audit comment's SQLx-MySQL
path is no longer present in this graph.

The [advisory](https://rustsec.org/advisories/RUSTSEC-2023-0071) describes timing
exposure of RSA private-key operations and has no patched release. This existing
exception must be revisited if an RSA private-key operation or an Octocrab caller
is introduced. All other vulnerability advisories remain fatal; no new advisory
ID is suppressed by this change.
