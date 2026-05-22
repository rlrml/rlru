# rlru

Rust-first Rocket League replay uploader.

This is a reboot of Rockpload with stricter TOML configuration, explicit local
state paths, testable auth/upload boundaries, and a Dioxus client scaffold.

## Development

```bash
direnv allow
just check
just run -- --help
just dioxus-desktop
```

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

The default storage targets include Rocky, Ballchasing, and a local Rocket
Sense server at `http://127.0.0.1:8080/api/v1`. For Rocket Sense uploads, set
`ROCKET_SENSE_TOKEN` to a Rocket Sense bearer token before running `rlru sync`.
