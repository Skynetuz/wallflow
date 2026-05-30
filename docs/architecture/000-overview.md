# WallFlow architecture overview

WallFlow is a Windows-first live wallpaper engine. The MVP focuses on reliability, resource control and clean process isolation.

## Process model

```text
wallflow-ui       Tauri UI, no business ownership of renderers
wallflow-core     long-lived orchestrator
wallflow-renderer isolated wallpaper renderer process
wallflow-cli      diagnostics and automation
```

`wallflow-core` owns state, configuration, monitor topology and renderer lifecycles. `wallflow-renderer` should be disposable. If one renderer crashes, Core restarts or disables only that renderer group.

## Platform model

Windows support is first-class in MVP. Linux is deferred. All platform-specific behavior must sit behind a crate boundary and use `cfg(target_os = "...")`.

## Rendering model

MVP supports:

1. static fallback image;
2. native renderer process skeleton;
3. media backend abstraction;
4. Windows Media Foundation backend target.

Web wallpapers are not part of MVP. They must be isolated in a separate renderer binary later.

## Reliability model

Core must provide:

- watchdog heartbeats;
- renderer restart policy;
- safe mode;
- structured logs;
- config rollback path;
- unsupported-platform errors instead of silent no-op behavior.
