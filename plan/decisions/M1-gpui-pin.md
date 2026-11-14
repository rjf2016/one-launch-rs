# F1: the GPUI pin

**Decision: `gpui-pre` from crates.io at an exact version, not a git rev of Zed.** The pair is `gpui-pre` 0.3.12 with `gpui-kit` 0.9.1, on Rust 1.100.0. Looked up on crates.io and GitHub on 2026-11-14.

## The pins

| What | Pin | Where it was read |
| --- | --- | --- |
| `gpui-pre` | `=0.3.12` | `https://crates.io/api/v1/crates/gpui-pre`: `max_stable_version` and `newest_version` 0.3.12, created 2026-11-09, `license` Apache-2.0, `published_by` huacnlee, `description` "Zed's GPU-accelerated UI framework (gpui-pre snapshot of zed@d2a4fb6)" |
| `gpui-pre-platform` | `=0.3.12` | `https://crates.io/api/v1/crates/gpui-pre-platform`: the same version, day, licence and publisher; "Zed's `gpui_platform` crate (gpui-pre snapshot of zed@d2a4fb6)" |
| `gpui-kit` | `=0.9.1`, default features off | `https://crates.io/api/v1/crates/gpui-kit`: newest version 0.9.1, created 2026-11-09, Apache-2.0, repository `longbridge/gpui-kit`, the same publisher. `https://crates.io/api/v1/crates/gpui-kit/0.9.1/dependencies`: `gpui-pre`, `gpui-pre-macros` and `gpui-pre-platform` each at `=0.3.12` |
| The Zed commit | `d2a4fb6ded7c470be25d1541281da3882495b65b`, committed 2026-11-08 | The short hash is in both descriptions; `https://api.github.com/repos/zed-industries/zed/commits/d2a4fb6` gives the full one |
| Rust | 1.100.0 | See "The Rust version" |

Both crates are the newest on crates.io, and `gpui-kit` 0.9.1 requires exactly `gpui-pre` 0.3.12, so this is the newest pair. `cargo build --locked -p ol-ui-kit` compiled it from an empty `target/` in 51 s on the owner's PC: 328 crates, no error, no system component missing.

## Why crates.io and not a git rev

- Git sources are banned, and both `deny.toml` and `cargo xtask check` enforce it.
- `gpui-kit` requires `gpui-pre` by exact version. A `gpui` from git would be a second lineage in the same build, and the two do not share traits.
- A crates.io version is immutable and `Cargo.lock` records its checksum.

What this accepts: `gpui-pre` is published by the GPUI Kit maintainer, not by Zed. Its manifest names `zed-industries/zed` as the repository, but nothing proves the tarball equals Zed's tree at that commit; that was not checked here. The exact pin and the lockfile checksum mean the code cannot change under us. The fallback stays the one in `../design/architecture.md`, risk 3: vendor the pinned crates.

## What the names are

- The package `gpui-pre` builds a library named `gpui`, and `gpui-pre-platform` one named `gpui_platform`. `[workspace.dependencies]` uses those two names as keys, as GPUI Kit does, because GPUI's macros expand to paths under `gpui::`.
- `gpui-kit` is a facade over three layers: `gpui-pre`; `gpui-base`, the unstyled layer, always on; and `gpui-component` with `gpui-kit-assets`, the styled layer, behind the default features `component` and `assets`.

## Features

| Crate | Features | Why |
| --- | --- | --- |
| `gpui-pre` | default (`font-kit`, `wayland`, `x11`, `windows-manifest`) | `gpui-kit` turns the defaults on whatever we ask for, and `gpui-pre-platform` turns `windows-manifest` on for Windows |
| `gpui-pre-platform` | default | |
| `gpui-kit` | none | `gpui-base` holds everything `architecture.md` takes from Kit: `Input` with `masked`, `Textarea`, the scrollbar and the focus trap (`gpui-base-0.9.1/src/input/`, `scrollbar.rs`, `focus_trap.rs`). The styled layer is what `architecture.md` says not to use. Leaving it out drops 96 packages from the lockfile (866 to 770) |

Two things a later ticket may need, each a `deps` ticket because it changes the lockfile:

- `gpui-kit` feature `component`, if S4 finds that the unstyled input is not enough.
- `gpui-pre` feature `test-support`, which `#[gpui::test]` and `TestAppContext` need. Tried in a scratch copy: it adds 44 packages to the lockfile. The design names the test API but not the feature, so it is not declared.

Never to be turned on: `gpui-pre` feature `screen-capture`. It builds `zed-scap`, which depends on `sysinfo`.

## The Rust version

- `gpui-pre` and `gpui-kit` declare no `rust-version`, and `longbridge/gpui-kit` has no toolchain file at `v0.9.1`.
- Zed builds that commit with 1.98.1 (`rust-toolchain.toml` at `d2a4fb6`).
- The highest `rust-version` declared by a crate in the Windows build is 1.89 (`uuid`, `rusqlite`, `aes`, `smol_str`); in the whole lockfile 1.92 (`oo7`, Linux only).

The pin is 1.100.0 (`rustc 1.100.0 (0ef1bc386 2026-11-09)`), the current stable: it is above all three, and it is the version the build above ran on. No older version was tried, so the lowest Rust that builds the pair is not known. `rust-toolchain.toml` names the version and the two components, `clippy` and `rustfmt`.

## Where GPUI meets the ban list

`deny.toml` checks the graph that builds for `x86_64-pc-windows-msvc`. Two banned crates are in it through GPUI, and each is allowed under the crates that bring it and nowhere else:

| Crate | Brought by | What it does there |
| --- | --- | --- |
| `chrono` 0.4.45 | `gpui-pre`, `gpui-pre-scheduler`, `gpui-base` | Their own date handling. Our crates use `time` |
| `winreg` 0.56.0 | `embed-resource` 3.0.12, a build dependency of `gpui-pre` | The build script reads the Windows SDK's path from `HKLM` to find the resource compiler. It runs at build time and is not linked into the app |

Two more are named in `Cargo.lock` and are not part of the Windows build: `sysinfo` 0.31.4, under `zed-scap`, which `gpui-pre` takes only with its feature `screen-capture`; and `tokio` 1.54.1, under `wasm-bindgen-futures`, for the web target. The build above compiled neither. `cargo deny` passes them because they are not in the graph it checks, and fails as soon as a member depends on one or turns `screen-capture` on.

## Licences

Every crate of the GPUI family in the Windows build is Apache-2.0: `gpui-pre`, `gpui-pre-platform`, `gpui-pre-windows`, `gpui-pre-macros`, `gpui-pre-collections`, `gpui-pre-refineable`, `gpui-pre-derive-refineable`, `gpui-pre-scheduler`, `gpui-pre-shared-string`, `gpui-pre-sum-tree`, `gpui-pre-util`, `gpui-pre-http-client`, `gpui-pre-ztracing`, `gpui-pre-ztracing-macro`, `gpui-kit`, `gpui-base`. No crate in that build is under the GPL, the LGPL or the MPL.

The approved crates, as pinned:

| Crate | Version | Licence |
| --- | --- | --- |
| `aes-gcm` | 0.11.1 | Apache-2.0 OR MIT |
| `anyhow` | 1.0.104 | MIT OR Apache-2.0 |
| `async-channel` | 2.5.0 | Apache-2.0 OR MIT |
| `async-task` | 4.7.1 | Apache-2.0 OR MIT |
| `async-trait` | 0.1.92 | MIT OR Apache-2.0 |
| `base64` | 0.23.1 | MIT OR Apache-2.0 |
| `ed25519-dalek` | 3.0.0 | BSD-3-Clause |
| `event-listener` | 5.4.2 | Apache-2.0 OR MIT |
| `futures-lite` | 2.6.1 | Apache-2.0 OR MIT |
| `getrandom` | 0.4.3 | MIT OR Apache-2.0 |
| `gpui-kit` | 0.9.1 | Apache-2.0 |
| `gpui-pre` | 0.3.12 | Apache-2.0 |
| `gpui-pre-platform` | 0.3.12 | Apache-2.0 |
| `image` | 0.25.10 | MIT OR Apache-2.0 |
| `regex` | 1.13.1 | MIT OR Apache-2.0 |
| `rusqlite` | 0.42.0 | MIT |
| `semver` | 1.0.28 | MIT OR Apache-2.0 |
| `serde` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 |
| `sha2` | 0.11.1 | MIT OR Apache-2.0 |
| `thiserror` | 2.0.21 | MIT OR Apache-2.0 |
| `time` | 0.3.55 | MIT OR Apache-2.0 |
| `ureq` | 3.5.2 | MIT OR Apache-2.0 |
| `uuid` | 1.29.2 | Apache-2.0 OR MIT |
| `windows` | 0.62.2 | MIT OR Apache-2.0 |
| `winresource` | 0.1.34 | MIT |
| `zeroize` | 1.9.1 | Apache-2.0 OR MIT |

`tray-icon` and `muda` are left out until S1 decides.

Crates in the Windows build that offer neither Apache-2.0 nor MIT, which is why `deny.toml` accepts six more licences:

| Licence | Crates | Through |
| --- | --- | --- |
| BSD-3-Clause | `ed25519-dalek`, `curve25519-dalek`, `subtle` | `ed25519-dalek`; `subtle` also under `ureq`'s rustls |
| BSD-3-Clause | `exr`, `lebe`, `instant`, `sha1_smol`, `tiny-skia`, `tiny-skia-path` | GPUI: its `image` features, `gpui-base`, its `uuid` features, `resvg` |
| BSD-2-Clause | `arrayref` | GPUI, under `tiny-skia` |
| ISC | `ring` (Apache-2.0 AND ISC), `rustls-webpki`, `untrusted` | `ureq` on rustls |
| CDLA-Permissive-2.0 | `webpki-roots`, the root certificates | `ureq` on rustls |
| Zlib | `foldhash` | `hashbrown`, under `serde_json` with `preserve_order`, `rusqlite` and GPUI |
| Zlib | `slotmap` | GPUI |
| Unicode-3.0 | the `icu_*` crates and their helpers (`litemap`, `potential_utf`, `tinystr`, `writeable`, `yoke`, `zerofrom`, `zerotrie`, `zerovec`), `unicode-properties` | GPUI |
| Unicode-3.0 | `unicode-ident` ((MIT OR Apache-2.0) AND Unicode-3.0) | every proc macro, `serde`'s and `thiserror`'s included |

## How to repeat

```
cargo metadata --locked --format-version 1
cargo build --locked -p ol-ui-kit
cargo deny check
cargo xtask check
```

To move the pin, change the three versions in `Cargo.toml` together, regenerate `Cargo.lock`, and run the four commands again. `gpui-kit`'s dependency list on crates.io says which `gpui-pre` it needs.
