# rlru development helpers

set positional-arguments := true

rlru_cmd := env_var_or_default("RLRU_CMD", "cargo run --bin rlru --")
dioxus_desktop_cmd := env_var_or_default(
  "RLRU_DIOXUS_DESKTOP_CMD",
  "dx serve --platform desktop --package rlru-dioxus --no-default-features --features desktop",
)
windows_target := "x86_64-pc-windows-gnu"

run *args:
    {{rlru_cmd}} "$@"

check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

release-tag:
    #!/usr/bin/env bash
    set -euo pipefail

    git diff --quiet
    git diff --cached --quiet

    version="$(cargo pkgid -p rlru | sed 's/.*[#@]//')"
    tag="v${version}"

    git fetch --tags origin

    if git rev-parse "refs/tags/$tag" >/dev/null 2>&1; then
      echo "tag $tag already exists" >&2
      exit 1
    fi

    git tag -a "$tag" -m "$tag"
    git push origin "$tag"

fmt:
    cargo fmt --all

windows-cli profile="debug":
    #!/usr/bin/env bash
    set -euo pipefail
    release_arg=()
    if [[ "{{profile}}" == "release" ]]; then
      release_arg=(--release)
    fi
    cargo build --target {{windows_target}} -p rlru "${release_arg[@]}"

windows-dioxus profile="debug":
    #!/usr/bin/env bash
    set -euo pipefail
    target_profile="{{profile}}"
    release_arg=()
    if [[ "$target_profile" == "release" ]]; then
      release_arg=(--release)
    fi
    cargo build \
      --target {{windows_target}} \
      -p rlru-dioxus \
      --no-default-features \
      --features desktop \
      "${release_arg[@]}"
    loader="$(find "target/{{windows_target}}/${target_profile}/build" -path '*/out/x64/WebView2Loader.dll' -print -quit)"
    if [[ -n "$loader" ]]; then
      cp "$loader" "target/{{windows_target}}/${target_profile}/WebView2Loader.dll"
    fi

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

dioxus-android-build *args:
    nix run .#dioxus-android-debug -- "$@"

dioxus-android-release *args:
    nix run .#dioxus-android-release -- "$@"
