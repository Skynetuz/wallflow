# Prompt 001 — foundation review

Use this after copying the generated repository into a real Rust environment.

```text
You are a senior Rust systems engineer. Review the current WallFlow foundation repository.

Goal:
Make the workspace compile cleanly on Windows 10 22H2+ / Windows 11 22H2+ without changing the architecture.

Hard constraints:
- Keep the workspace crate boundaries.
- Do not implement Web wallpapers.
- Do not add Linux support beyond explicit UnsupportedPlatform errors.
- Do not move renderer lifecycle logic into UI.
- No unwrap/expect/todo/unimplemented in production code.
- unsafe only inside platform modules and every unsafe block must have a SAFETY comment.

Tasks:
1. Run cargo fmt --all.
2. Run cargo clippy --workspace --all-targets -- -D warnings.
3. Run cargo test --workspace.
4. Fix compile errors, clippy warnings and test failures.
5. Pay special attention to windows crate signatures in wallflow-monitor and wallflow-desktop.
6. Do not broaden scope.

Return:
- exact files changed;
- why each change was needed;
- remaining known risks;
- commands executed and their output summary.
```
