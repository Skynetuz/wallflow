//! Wallpaper package format for WallFlow.
//!
//! This crate defines the wallpaper package format (manifest v0), validation
//! logic, and loading/parsing utilities. Packages are cloud-testable and do
//! not depend on any platform-specific code.
//!
//! ## Package Format v0
//!
//! A wallpaper package is a directory containing:
//!
//! - `manifest.json`: Describes the wallpaper (kind, entry, preview, etc.)
//! - `content/`: Directory with wallpaper assets (images, etc.)
//! - `preview.png`: Optional preview image
//!
//! ## Security
//!
//! All asset paths must be relative and inside the package directory.
//! Path traversal (e.g., `../`) is rejected to prevent reading files outside
//! the package directory.

mod error;
mod manifest;
mod validation;

pub use error::WallpaperPackageError;
pub use manifest::*;
pub use validation::{validate_manifest, validate_package, WallpaperValidationReport};
