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
