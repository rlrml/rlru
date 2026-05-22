# rlru development helpers

set positional-arguments := true

rlru_cmd := env_var_or_default("RLRU_CMD", "cargo run --bin rlru --")
dioxus_desktop_cmd := env_var_or_default(
  "RLRU_DIOXUS_DESKTOP_CMD",
  "dx serve --platform desktop --package rlru-dioxus --no-default-features --features desktop",
)

run *args:
    {{rlru_cmd}} "$@"

check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

fmt:
    cargo fmt --all

dioxus-desktop *args:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${WAYLAND_DISPLAY:-}" ]]; then
      for socket in "${XDG_RUNTIME_DIR:-}"/wayland-*; do
        [[ -S "$socket" ]] || continue
        export WAYLAND_DISPLAY="${socket##*/}"
        break
      done
    fi
    if [[ -n "${WAYLAND_DISPLAY:-}" ]]; then
      export GDK_BACKEND="${GDK_BACKEND:-wayland,x11}"
    fi
    {{dioxus_desktop_cmd}} "$@"
