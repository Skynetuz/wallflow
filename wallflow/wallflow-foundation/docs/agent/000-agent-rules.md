# Agent rules for WallFlow

Use this file as non-negotiable context for every AI-agent coding task.

1. Do not turn WallFlow into a monolith.
2. Do not put renderer lifecycle logic into the UI.
3. Do not add Linux implementation until Windows MVP is stable.
4. Do not add WebView wallpapers in MVP.
5. Keep all unsafe Win32 code inside platform modules.
6. Every unsafe block must have a `SAFETY:` comment.
7. Avoid new dependencies unless the task explicitly allows them.
8. Public APIs must be serializable when they cross process/UI boundaries.
9. Every crate must compile independently as part of the workspace.
10. Add tests for pure logic: config, protocol framing, state machines, topology diffing.
11. Use typed errors; do not return stringly errors from library code.
12. No `unwrap`, `expect`, `todo`, `unimplemented`, or hidden placeholder paths in production code.

When implementing platform code, write the pure logic first, then the OS bindings.
