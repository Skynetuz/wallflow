# WallFlow roadmap

## MVP-0 Foundation

- Workspace structure.
- Shared domain model.
- Config load/save.
- IPC protocol contracts.
- Monitor diffing.
- Windows monitor enumeration first pass.
- Windows desktop attach first pass.
- Media backend abstraction.
- Renderer process smoke binary.
- CLI diagnostics.

## MVP-1 Windows proof

- Compile and test on Windows.
- Harden WorkerW/Progman discovery.
- Add dummy native window renderer.
- Attach renderer window behind desktop icons.
- Restart renderer after crash.

## MVP-2 static wallpaper

- Add winit/wgpu static renderer.
- Per-monitor placement.
- Fullscreen detection pause policy.

## MVP-3 video wallpaper

- Implement Media Foundation backend.
- Hardware decode where available.
- Muted looping video.
- Fallback on Windows N missing media features.

## MVP-4 UI

- Tauri 2 + React UI.
- Library list.
- Monitor list.
- Apply wallpaper.
- Diagnostics panel.

## v1

- Package format.
- Web wallpapers as isolated renderer.
- App rules.
- Playlists.
- Hotkeys.
- Updater.

## v2

- Linux X11 first.
- Selective Wayland support.
- Online catalog.
- Audio-reactive API.
- Plugin sandboxing.
