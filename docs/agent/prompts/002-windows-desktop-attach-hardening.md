# Prompt 002 — Windows desktop attach hardening

```text
You are a senior Win32/Rust engineer. Harden the WallFlow `wallflow-desktop` crate.

Goal:
Make `find_desktop_worker()` and `attach_window_to_desktop()` robust on Windows 10 22H2+ and Windows 11 22H2+.

Constraints:
- Do not change public function names unless necessary.
- Keep all Win32 code inside `wallflow-desktop`.
- Add detailed tracing logs for discovery decisions.
- Do not panic on missing WorkerW; return typed errors.
- Add a small manual diagnostic CLI path if useful, but do not add UI.

Implement:
1. Correct WorkerW/Progman discovery.
2. Explorer restart tolerance strategy.
3. GetLastError-based diagnostics where applicable.
4. A `DesktopAttachReport` struct with discovered HWNDs and selected strategy.
5. Unit-test pure selection logic if extracted.

Acceptance:
- cargo test --workspace passes.
- Manual Windows test prints worker handle and can attach a dummy window.
```
