# psynet

[![crates.io](https://img.shields.io/crates/v/psynet.svg)](https://crates.io/crates/psynet)
[![docs.rs](https://img.shields.io/docsrs/psynet)](https://docs.rs/psynet)
[![license](https://img.shields.io/crates/l/psynet.svg)](#license)

A Rust client for Psyonix's **PsyNet** RPC backend — the API that powers Rocket
League's online services (`api.rlpp.psynet.gg`). It handles request signing, the
WebSocket transport, and typed RPC calls used to look up player profiles, match
history, and ranks.

`psynet` is developed as part of [rlru](https://github.com/rlrml/rlru), a
Rust-first Rocket League replay uploader, but it has no dependency on the rest of
that project and can be used on its own.

## What it does

- Signs PsyNet RPC requests (HMAC over the request body) the way the game client
  does, so calls are accepted by the live backend.
- Opens and manages the authenticated WebSocket session.
- Exposes typed calls for the endpoints rlru needs: authenticate a player, pull
  match history (with per-match rank/MMR metadata), and look up player profiles.

## Usage

```toml
[dependencies]
psynet = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use psynet::{PlayerId, PlayerPlatform, PsyNetClient};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = PsyNetClient::new();

    // Authenticate with a Rocket League account id + access token. This returns
    // an authenticated RPC session over a managed WebSocket.
    let rpc = client.auth_player("account_id", "access_token").await?;

    // Pull recent matches — each entry carries a replay URL and rank metadata.
    for entry in rpc.get_match_history().await? {
        println!("{} -> {}", entry.match_info.match_guid, entry.replay_url);
    }

    // Look up profiles for specific players across platforms.
    let players = vec![PlayerId::new(PlayerPlatform::Epic, "some-epic-id")];
    let profiles = rpc.get_profiles(players).await?;
    println!("fetched {} profiles", profiles.len());

    rpc.close().await?;
    Ok(())
}
```

The main entry points are [`PsyNetClient`](https://docs.rs/psynet/latest/psynet/struct.PsyNetClient.html)
(transport + signing) and the [`PsyNetRpc`](https://docs.rs/psynet/latest/psynet/struct.PsyNetRpc.html)
session it hands back. See the [API documentation](https://docs.rs/psynet) for
the full surface.

## License

Licensed under either of
[Apache License, Version 2.0](https://github.com/rlrml/rlru/blob/main/LICENSE-APACHE)
or [MIT license](https://github.com/rlrml/rlru/blob/main/LICENSE-MIT) at your
option.
