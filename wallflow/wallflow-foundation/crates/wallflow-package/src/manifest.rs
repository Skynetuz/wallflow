use serde::{Deserialize, Serialize};

use crate::error::WallpaperPackageError;
pub use wallflow_common::FitMode;

/// Current manifest schema version.
pub const MANIFEST_SCHEMA_VERSION: u32 = 0;

/// Supported schema versions for parsing.
pub const SUPPORTED_SCHEMA_VERSIONS: &[u32] = &[0];

/// Unique identifier for a wallpaper package.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WallpaperPackageId(pub String);

impl WallpaperPackageId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// Semantic version for a wallpaper package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WallpaperPackageVersion(pub String);

impl WallpaperPackageVersion {
    pub fn new(version: impl Into<String>) -> Self {
        Self(version.into())
    }
}

impl Default for WallpaperPackageVersion {
    fn default() -> Self {
        Self("0.1.0".into())
    }
}

/// The kind of wallpaper, as declared in the package manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WallpaperKind {
    StaticImage,
    Video,
    Web,
}

impl std::fmt::Display for WallpaperKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WallpaperKind::StaticImage => write!(f, "static_image"),
            WallpaperKind::Video => write!(f, "video"),
            WallpaperKind::Web => write!(f, "web"),
        }
    }
}

/// Static image wallpaper entry configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticImageWallpaper {
    /// Path to the image file, relative to the package root.
    pub image: String,
    /// How the image should be fitted to the screen.
    #[serde(default)]
    pub fit: FitMode,
    /// Background color as a CSS color string (e.g., "#000000").
    #[serde(default = "default_background")]
    pub background: String,
    /// Optional opacity (0-255). None means fully opaque.
    #[serde(default)]
    pub opacity: Option<u8>,
}

fn default_background() -> String {
    "#000000".into()
}

/// A wallpaper asset referenced by the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WallpaperAsset {
    /// Relative path within the package directory.
    pub path: String,
    /// Optional MIME type hint.
    #[serde(default)]
    pub mime_type: Option<String>,
}

/// A preview image for the wallpaper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WallpaperPreview {
    /// Relative path to the preview image within the package.
    pub path: String,
}

/// Setting schema entry for future user-configurable options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WallpaperSettingSchema {
    /// Setting key name.
    pub key: String,
    /// Human-readable label.
    pub label: String,
    /// Setting type (e.g., "bool", "range", "color").
    pub kind: String,
    /// Default value as a JSON value.
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

/// The wallpaper package manifest (v0).
///
/// This is the root document that describes a wallpaper package. It is
/// stored as `manifest.json` in the package root directory.
///
/// # Example
///
/// ```json
/// {
///   "schema_version": 0,
///   "id": "example-static-wallpaper",
///   "title": "Example Static Wallpaper",
///   "description": "Static wallpaper package for WallFlow MVP.",
///   "author": "WallFlow",
///   "kind": "static_image",
///   "entry": {
///     "image": "content/wallpaper.png",
///     "fit": "cover",
///     "background": "#000000"
///   },
///   "preview": "preview.png",
///   "tags": ["static", "mvp"]
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallpaperManifest {
    /// Schema version of the manifest format.
    pub schema_version: u32,
    /// Unique identifier for this wallpaper package.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Author or publisher name.
    #[serde(default)]
    pub author: String,
    /// Wallpaper kind declared at the top level for quick filtering.
    pub kind: WallpaperKind,
    /// Entry point configuration (kind-specific).
    pub entry: StaticImageWallpaper,
    /// Relative path to preview image (optional).
    #[serde(default)]
    pub preview: Option<String>,
    /// Tags for categorization and search.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Package version (optional for v0).
    #[serde(default)]
    pub version: Option<WallpaperPackageVersion>,
}

/// A loaded wallpaper package (manifest + resolved directory path).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallpaperPackage {
    /// The resolved directory path of the package.
    pub directory: std::path::PathBuf,
    /// The parsed manifest.
    pub manifest: WallpaperManifest,
}

impl WallpaperPackage {
    /// Load and parse a wallpaper package from the given directory.
    ///
    /// Reads `manifest.json` from the directory, parses it, and returns
    /// a `WallpaperPackage` with the resolved path and manifest.
    pub fn load(directory: &std::path::Path) -> Result<Self, WallpaperPackageError> {
        let manifest_path = directory.join("manifest.json");
        let content = std::fs::read_to_string(&manifest_path)?;
        let manifest: WallpaperManifest = serde_json::from_str(&content)?;

        Ok(Self {
            directory: directory.to_path_buf(),
            manifest,
        })
    }

    /// Parse a manifest from a JSON string.
    pub fn parse_manifest(json: &str) -> Result<WallpaperManifest, WallpaperPackageError> {
        let manifest: WallpaperManifest = serde_json::from_str(json)?;
        Ok(manifest)
    }

    /// Resolve an asset path relative to the package directory.
    ///
    /// Returns an error if the path attempts traversal outside the package.
    pub fn resolve_asset_path(
        &self,
        relative_path: &str,
    ) -> Result<std::path::PathBuf, WallpaperPackageError> {
        validate_relative_path(relative_path)?;
        Ok(self.directory.join(relative_path))
    }
}

/// Validate that a relative path does not contain path traversal sequences.
pub fn validate_relative_path(path: &str) -> Result<(), WallpaperPackageError> {
    // Reject absolute paths
    if std::path::Path::new(path).is_absolute() {
        return Err(WallpaperPackageError::PathTraversal(format!(
            "absolute path not allowed: {path}"
        )));
    }

    // Reject path traversal components
    let components = std::path::Path::new(path).components();
    for component in components {
        match component {
            std::path::Component::ParentDir => {
                return Err(WallpaperPackageError::PathTraversal(format!(
                    "path traversal (..) not allowed: {path}"
                )));
            }
            std::path::Component::RootDir => {
                return Err(WallpaperPackageError::PathTraversal(format!(
                    "root directory reference not allowed: {path}"
                )));
            }
            _ => {}
        }
    }

    Ok(())
}
