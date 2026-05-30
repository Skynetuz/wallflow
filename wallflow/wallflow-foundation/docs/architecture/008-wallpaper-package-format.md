# 008 – Wallpaper Package Format v0

> Stage: `005-cloud-safe-wallpaper-package-and-apply-contract`
> Date: 2026-05-30

## Overview

This document specifies the wallpaper package format (v0) for WallFlow. A wallpaper
package is a directory containing a manifest file and associated assets. The format
is designed to be extensible, cloud-testable, and safe against path traversal attacks.

## Why Static Image First

Static image wallpapers are the simplest kind to implement and validate. They require
no GPU rendering pipeline, no video codecs, and no web runtime. By implementing the
full package → validate → apply → confirm lifecycle for static images first, we
establish the complete infrastructure (package format, validation, IPC payloads,
renderer state machine) that all future wallpaper kinds will reuse.

Video wallpapers (stage 008+) will add Media Foundation decoding. Web wallpapers
(stage 010+) will add an isolated web runtime. Both will follow the same package
structure and validation pipeline.

## Package Structure

```
my-wallpaper/
├── manifest.json       # Required: package metadata and entry config
├── content/            # Required: wallpaper assets
│   └── wallpaper.png   # The wallpaper image
└── preview.png         # Optional: preview thumbnail
```

## Manifest Format (v0)

The manifest is a JSON file named `manifest.json` in the package root directory.

### Required Fields

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | `u32` | Must be `0` for v0 manifests |
| `id` | `string` | Unique package identifier (e.g., "mountain-sunset") |
| `title` | `string` | Human-readable wallpaper name |
| `kind` | `string` | Wallpaper kind: `"static_image"` (v0) |
| `entry` | `object` | Wallpaper entry configuration (kind-specific) |
| `entry.image` | `string` | Relative path to the image file |
| `entry.fit` | `string` | Fit mode: `"cover"`, `"contain"`, `"stretch"`, `"center"`, `"tile"` |
| `entry.background` | `string` | Background color as CSS string (e.g., `"#000000"`) |

### Optional Fields

| Field | Type | Description |
|-------|------|-------------|
| `description` | `string` | Human-readable description |
| `author` | `string` | Author or publisher name |
| `preview` | `string` | Relative path to preview image |
| `tags` | `string[]` | Tags for categorization |
| `version` | `string` | Semantic version of the package |
| `entry.opacity` | `u8` | Wallpaper opacity (0-255, default: 255/fully opaque) |

### Example

```json
{
  "schema_version": 0,
  "id": "example-static-wallpaper",
  "title": "Example Static Wallpaper",
  "description": "Static wallpaper package for WallFlow MVP.",
  "author": "WallFlow",
  "kind": "static_image",
  "entry": {
    "image": "content/wallpaper.png",
    "fit": "cover",
    "background": "#000000"
  },
  "preview": "preview.png",
  "tags": ["static", "mvp"]
}
```

## Fit Modes

| Mode | Description |
|------|-------------|
| `cover` | Scale to fill the screen, cropping edges. Default. |
| `contain` | Scale to fit within the screen, letterboxing if needed. |
| `stretch` | Stretch to fill the screen, ignoring aspect ratio. |
| `center` | Display at original size, centered on screen. |
| `tile` | Repeat the image to fill the screen. |

## Validation Checks

The package validator performs these checks:

1. **Schema version**: Must be 0 (the only supported version).
2. **Required fields**: `id`, `title`, `kind` must be non-empty.
3. **Wallpaper kind**: Must be `"static_image"` for v0.
4. **Entry image path**: Must be non-empty.
5. **Path traversal**: All asset paths must be relative and inside the package.
   - Absolute paths are rejected.
   - `..` (parent directory) components are rejected.
   - Root directory references are rejected.
6. **Entry asset existence**: The image file must exist on disk.
7. **Preview optional**: If a preview path is specified, a warning is emitted
   if the file does not exist (not an error).

## Security: Path Traversal Prevention

All asset paths in the manifest must be relative and must not traverse outside
the package directory. This prevents malicious packages from reading or
referencing files outside their intended scope.

Rejected patterns:
- `../../etc/passwd`
- `/absolute/path`
- `content/../../../secret`

Allowed patterns:
- `content/wallpaper.png`
- `preview.png`
- `assets/bg/image.jpg`

## IPC Integration

When a wallpaper is applied via IPC, the `ApplyWallpaper` command carries the
full typed payload:

```rust
ApplyWallpaperRequest {
    wallpaper_id: WallpaperId,
    payload: WallpaperPayload::StaticImage(StaticImagePayload {
        image_path: String,  // Resolved absolute path
        fit: FitMode,
        background: String,
        opacity: Option<u8>,
    }),
    target_monitor: MonitorId,
}
```

The renderer validates the payload, records the applied state, and responds
with `WallpaperApplied` or `WallpaperApplyFailed`.

## Current Limitations

- The renderer accepts and records the applied wallpaper state but does **not**
  yet render the image to the screen. Real GPU rendering will be implemented
  in stage 006 (winit/wgpu renderer).
- Only `static_image` kind is supported. Video and web wallpapers will be
  added in future stages.
- Image format validation (is this actually a valid PNG/JPEG?) is not yet
  implemented. The dummy fixture file in the smoke test is not a real image.

## Future Extensions

- **v1 manifest**: Add `settings` schema for user-configurable options.
- **Video entry**: Add `video` kind with `entry.video`, `entry.muted`, `entry.looping`.
- **Web entry**: Add `web` kind with `entry.html` entry point.
- **Package archives**: Support `.wallflow` zip archives instead of directories.
- **Image validation**: Verify that image files are decodable.
- **Content hashing**: SHA-256 checksums for integrity verification.
