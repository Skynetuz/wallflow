# WallFlow foundation

WallFlow is a Windows-first live wallpaper engine foundation written in Rust.

This repository is intentionally structured as a commercial-style desktop utility rather than a single prototype binary:

- `wallflow-common` — shared typed domain model.
- `wallflow-config` — persistent application configuration.
- `wallflow-ipc` — framed JSON IPC protocol contracts.
- `wallflow-monitor` — display enumeration and topology diffing.
- `wallflow-desktop` — platform-specific desktop attachment layer.
- `wallflow-media` — media backend abstraction; Media Foundation is the Windows production target.
- `wallflow-core` — orchestration, renderer lifecycle and watchdog primitives.
- `wallflow-renderer` — renderer process entry point skeleton.
- `wallflow-cli` — developer CLI for diagnostics.

## Target

MVP target is Windows 10 22H2+ and Windows 11 22H2+.
Linux support is intentionally stubbed with `UnsupportedPlatform` errors until the Windows foundation is stable.

## Current state

This is a foundation scaffold, not a finished Wallpaper Engine replacement. It gives the agent a strict structure, typed APIs and platform isolation. The risky Win32 pieces are localized in `wallflow-desktop` and `wallflow-monitor`.

The code was generated in an environment without Rust installed, so run these commands on a development machine:

```powershell
rustup update stable
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

On Windows, run:

```powershell
cargo run -p wallflow-cli -- monitors
cargo run -p wallflow-renderer -- --monitor primary --wallpaper none
```

## Design rule

The UI must never directly manage live wallpaper windows. UI sends commands to Core. Core owns renderer lifecycles. Renderers are isolated processes.
