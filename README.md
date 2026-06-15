# rlru

[![CI](https://github.com/rlrml/rlru/actions/workflows/distributable-binaries.yml/badge.svg)](https://github.com/rlrml/rlru/actions/workflows/distributable-binaries.yml)
[![crates.io](https://img.shields.io/crates/v/rlru.svg)](https://crates.io/crates/rlru)
[![docs.rs](https://img.shields.io/docsrs/rlru)](https://docs.rs/rlru)
[![license](https://img.shields.io/crates/l/rlru.svg)](#license)

Rust-first Rocket League replay uploader.

rlru uses strict TOML configuration, explicit local state paths, testable
auth/upload boundaries, and a Dioxus client scaffold.

## Crates

This repository is a Cargo workspace. The reusable pieces are published to
crates.io as their own crates:

| Crate | Description | crates.io | docs.rs |
| --- | --- | --- | --- |
| [`rlru`](Cargo.toml) | CLI + library for uploading Rocket League replays | [![crates.io](https://img.shields.io/crates/v/rlru.svg)](https://crates.io/crates/rlru) | [![docs.rs](https://img.shields.io/docsrs/rlru)](https://docs.rs/rlru) |
| [`psynet`](crates/psynet) | Standalone client for Psyonix's PsyNet RPC backend (Rocket League online services) | [![crates.io](https://img.shields.io/crates/v/psynet.svg)](https://crates.io/crates/psynet) | [![docs.rs](https://img.shields.io/docsrs/psynet)](https://docs.rs/psynet) |

`crates/psynet` ([README](crates/psynet/README.md)) is independent of the rest of
rlru and can be depended on directly. The `rlru-dioxus` desktop client also lives
in this workspace but is not published. All published crates currently share a
single version number.

## Library usage

`rlru` is a library as well as a CLI — the binary is a thin wrapper over the
public API, so you can drive config, syncing, and uploads from your own Rust
code.

```toml
[dependencies]
rlru = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use rlru::Config;
use rlru::paths::AppPaths;
use rlru::sync::SyncService;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let config = Config::load_or_default(&paths.config_file())?;

    // Run one sync pass: discover new replays and push them to every
    // configured upload destination.
    let summary = SyncService::new(paths, config).run_once().await?;
    println!("uploaded {} replays", summary.uploaded);
    Ok(())
}
```

Key building blocks:

- [`rlru::Config`](src/config.rs) — strict TOML config, accounts, and upload
  destinations (`Config::load`, `validate`, helpers like
  `UploadDestinationConfig::ballchasing()`).
- [`rlru::sync::SyncService`](src/sync.rs) — the sync engine
  (`run_once`, `run_once_with_options`, `current_history`).
- [`rlru::upload`](src/upload.rs) — the upload destination abstraction.
- [`rlru::psynet`](crates/psynet) — the PsyNet client is re-exported as
  `rlru::psynet`, so `rlru` users don't need a separate dependency to talk to
  PsyNet directly.

Full API docs are on [docs.rs/rlru](https://docs.rs/rlru) and
[docs.rs/psynet](https://docs.rs/psynet).

## Screenshots

![Overview](docs/screenshots/overview.png)

![History](docs/screenshots/history.png)

![Accounts](docs/screenshots/accounts.png)

![Upload Destinations](docs/screenshots/upload-destinations.png)

## Development

```bash
direnv allow
just check
just run -- --help
just dioxus-desktop
```

## Releases

Releases are driven entirely by the `vX.Y.Z` tag that matches the `rlru` Cargo
package version. All published crates (`rlru`, `psynet`) share that single
version number.

**Fully automatic (recommended):** bump the `version` in `Cargo.toml`,
`crates/psynet/Cargo.toml`, and `crates/rlru-dioxus/Cargo.toml` and merge to
`main`. The [`auto-tag-release`](.github/workflows/auto-tag-release.yml) workflow
notices the new version and pushes the matching `vX.Y.Z` tag for you, which kicks
off the release pipeline.

> For the auto-created tag to trigger the downstream publish jobs, add a
> Personal Access Token (with `contents: write`) as the `RELEASE_PAT` repository
> secret — a tag pushed with the default `GITHUB_TOKEN` will not start new
> workflow runs. Without it, the tag is still created; just re-push it manually.

**Manual:** cut the tag yourself from a clean `main` checkout:

```bash
just release-tag
```

Either way, the tagged run:

- uploads downloadable assets to the GitHub Releases page:
  - `rlru-cli-linux-x86_64.tar.gz`
  - `rlru-cli-windows-x86_64.zip`
  - `rlru-dioxus-linux-x86_64.AppImage`
- publishes `psynet` then `rlru` to crates.io, when the `CARGO_REGISTRY_TOKEN`
  repository secret is configured (GitHub release assets are still created when
  that secret is absent).

## Windows Builds From Linux

The dev shell includes the Fenix `x86_64-pc-windows-gnu` Rust target and the
MinGW linker toolchain. Build Windows executables from Linux with:

```bash
just windows-cli release
just windows-dioxus release
```

The CLI executable lands under
`target/x86_64-pc-windows-gnu/release/rlru.exe`. The Dioxus desktop executable
lands under `target/x86_64-pc-windows-gnu/release/rlru-dioxus.exe`, with
`WebView2Loader.dll` copied beside it.

## Configuration

Print the effective default configuration:

```bash
rlru config defaults
```

Validate a configuration file:

```bash
rlru --config ~/.config/rlru/config.toml config validate
```

Tokens are stored separately from TOML config under the XDG config directory.

### Accounts and Epic auth

Add and authenticate an Epic Games account with the device-code flow:

```bash
rlru account add "My Epic Account" --authenticate --open
```

The command writes the account to `config.toml`, selects it, opens Epic's login
page, prints the device code, waits for approval, and stores the refresh token
under the account id in the local token directory.

In the Dioxus desktop app, use the Accounts screen with `Epic Auth` checked, or
click `Authenticate` on an existing Epic account. The app opens Epic's login
page; paste the authorization code back into rlru to save durable local auth.

If the account is already in the config, authenticate it separately:

```bash
rlru auth --account "My Epic Account" device --open --wait
```

Useful account commands:

```bash
rlru account list
rlru account select "My Epic Account"
rlru account remove "My Epic Account"
```

The default upload destinations include Rocky, Ballchasing, and Rocket Sense at
`https://rocket-sense.duckdns.org/api/v1`. For Rocket Sense uploads, set
`ROCKET_SENSE_TOKEN` to a Rocket Sense bearer token before running `rlru sync`,
or configure a command that prints the token to stdout:

```toml
[storage.auth]
kind = "bearer_command"
command = ["pass", "show", "rocket-sense/token"]
```

### Upload names

Replays are uploaded with a templated filename, which most destinations show as
the replay's name. The default produces names like
`2024-01-15.14.30 SaltySphinx Ranked Doubles Win`. Customize it in `[behavior]`:

```toml
[behavior]
upload_name_template = "{YEAR}-{MONTH}-{DAY}.{HOUR}.{MIN} {PLAYER} {MODE} {WINLOSS}"
```

`{PLAYER}` and `{WINLOSS}` are from the synced account's perspective. Available
placeholders:

| Token | Meaning |
| --- | --- |
| `{YEAR}` `{MONTH}` `{DAY}` | Match date (local time, zero-padded) |
| `{HOUR}` `{MIN}` `{SEC}` | Match time (local time, zero-padded) |
| `{PLAYER}` | Synced account's in-match name (falls back to the account name) |
| `{MODE}` | Playlist name (e.g. `Ranked Doubles`, `Tournament`) |
| `{MAP}` | Map name (e.g. `DFH Stadium`) |
| `{WINLOSS}` | `Win` / `Loss` / `Draw` for the synced account |
| `{SCORE}` | Final score as `team0-team1` |
| `{MATCH_ID}` | PsyNet match GUID |

The rendered name is sanitized for use as a filename and gets a `.replay`
extension automatically. Set `upload_name_template = ""` to keep the legacy
match-id filename.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
