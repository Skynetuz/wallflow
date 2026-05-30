use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::manifest::{
    validate_relative_path, WallpaperKind, WallpaperManifest, SUPPORTED_SCHEMA_VERSIONS,
};

/// Result of validating a wallpaper manifest or package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WallpaperValidationReport {
    /// Whether the validation passed with no errors.
    pub valid: bool,
    /// List of error messages (empty if valid).
    pub errors: Vec<String>,
    /// List of warning messages (non-fatal issues).
    pub warnings: Vec<String>,
    /// The package ID that was validated.
    pub package_id: String,
    /// The kind declared in the manifest.
    pub kind: String,
}

impl WallpaperValidationReport {
    fn new(package_id: &str, kind: &str) -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            package_id: package_id.to_string(),
            kind: kind.to_string(),
        }
    }

    fn add_error(&mut self, message: String) {
        self.valid = false;
        self.errors.push(message);
    }

    fn add_warning(&mut self, message: String) {
        self.warnings.push(message);
    }
}

/// Validate a parsed manifest without checking filesystem paths.
///
/// Checks:
/// - Schema version is supported
/// - Required fields are present (id, title, kind)
/// - Wallpaper kind is supported for this version
/// - Entry image path is not empty
/// - All asset paths are relative and do not contain traversal
pub fn validate_manifest(manifest: &WallpaperManifest) -> WallpaperValidationReport {
    let mut report = WallpaperValidationReport::new(&manifest.id, &manifest.kind.to_string());

    // Schema version
    if !SUPPORTED_SCHEMA_VERSIONS.contains(&manifest.schema_version) {
        report.add_error(format!(
            "unsupported schema_version: {} (supported: {:?})",
            manifest.schema_version, SUPPORTED_SCHEMA_VERSIONS
        ));
    }

    // Required fields
    if manifest.id.trim().is_empty() {
        report.add_error("missing required field: id".into());
    }
    if manifest.title.trim().is_empty() {
        report.add_error("missing required field: title".into());
    }

    // Kind support
    match &manifest.kind {
        WallpaperKind::StaticImage => {
            // Validate entry
            if manifest.entry.image.trim().is_empty() {
                report.add_error("static_image entry requires a non-empty 'image' path".into());
            }
        }
        WallpaperKind::Video => {
            report.add_error(format!(
                "unsupported wallpaper kind for v0: {} (video support coming in a future version)",
                manifest.kind
            ));
        }
        WallpaperKind::Web => {
            report.add_error(format!(
                "unsupported wallpaper kind for v0: {} (web support coming in a future version)",
                manifest.kind
            ));
        }
    }

    // Validate entry image path for traversal
    if !manifest.entry.image.trim().is_empty() {
        if let Err(e) = validate_relative_path(&manifest.entry.image) {
            report.add_error(format!("entry image path is invalid: {e}"));
        }
    }

    // Validate preview path if present
    if let Some(preview) = &manifest.preview {
        if preview.trim().is_empty() {
            report.add_warning("preview path is specified but empty".into());
        } else if let Err(e) = validate_relative_path(preview) {
            report.add_error(format!("preview path is invalid: {e}"));
        }
    }

    if report.valid {
        debug!(package_id = %manifest.id, "manifest validation passed");
    } else {
        warn!(package_id = %manifest.id, errors = ?report.errors, "manifest validation failed");
    }

    report
}

/// Validate a full wallpaper package, including filesystem checks.
///
/// In addition to all manifest checks, this also verifies:
/// - Entry asset file exists
/// - Preview file exists (if specified)
pub fn validate_package(package: &crate::WallpaperPackage) -> WallpaperValidationReport {
    let mut report = validate_manifest(&package.manifest);

    // Check entry image exists
    if !package.manifest.entry.image.trim().is_empty() {
        let image_path = package.directory.join(&package.manifest.entry.image);
        if !image_path.exists() {
            report.add_error(format!(
                "entry image file not found: {} (resolved to {})",
                package.manifest.entry.image,
                image_path.display()
            ));
        }
    }

    // Check preview exists if specified
    if let Some(preview) = &package.manifest.preview {
        if !preview.trim().is_empty() {
            let preview_path = package.directory.join(preview);
            if !preview_path.exists() {
                report.add_warning(format!(
                    "preview file not found: {} (resolved to {})",
                    preview,
                    preview_path.display()
                ));
            }
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::*;

    fn valid_static_manifest() -> WallpaperManifest {
        WallpaperManifest {
            schema_version: 0,
            id: "test-static".into(),
            title: "Test Static Wallpaper".into(),
            description: "A test wallpaper".into(),
            author: "Test".into(),
            kind: WallpaperKind::StaticImage,
            entry: StaticImageWallpaper {
                image: "content/wallpaper.png".into(),
                fit: FitMode::Cover,
                background: "#000000".into(),
                opacity: None,
            },
            preview: Some("preview.png".into()),
            tags: vec!["test".into()],
            version: None,
        }
    }

    #[test]
    fn valid_manifest_parses() {
        let manifest = valid_static_manifest();
        let report = validate_manifest(&manifest);
        assert!(
            report.valid,
            "expected valid, got errors: {:?}",
            report.errors
        );
        assert_eq!(report.package_id, "test-static");
    }

    #[test]
    fn missing_id_fails() {
        let mut manifest = valid_static_manifest();
        manifest.id = "".into();
        let report = validate_manifest(&manifest);
        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.contains("id")));
    }

    #[test]
    fn missing_title_fails() {
        let mut manifest = valid_static_manifest();
        manifest.title = "".into();
        let report = validate_manifest(&manifest);
        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.contains("title")));
    }

    #[test]
    fn unsupported_schema_version_fails() {
        let mut manifest = valid_static_manifest();
        manifest.schema_version = 99;
        let report = validate_manifest(&manifest);
        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.contains("schema_version")));
    }

    #[test]
    fn unsupported_video_kind_fails() {
        let mut manifest = valid_static_manifest();
        manifest.kind = WallpaperKind::Video;
        let report = validate_manifest(&manifest);
        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.contains("video")));
    }

    #[test]
    fn unsupported_web_kind_fails() {
        let mut manifest = valid_static_manifest();
        manifest.kind = WallpaperKind::Web;
        let report = validate_manifest(&manifest);
        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.contains("web")));
    }

    #[test]
    fn path_traversal_in_image_rejected() {
        let mut manifest = valid_static_manifest();
        manifest.entry.image = "../etc/passwd".into();
        let report = validate_manifest(&manifest);
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.contains("traversal") || e.contains("invalid")));
    }

    #[test]
    fn empty_image_path_fails() {
        let mut manifest = valid_static_manifest();
        manifest.entry.image = "".into();
        let report = validate_manifest(&manifest);
        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.contains("image")));
    }

    #[test]
    fn preview_is_optional() {
        let mut manifest = valid_static_manifest();
        manifest.preview = None;
        let report = validate_manifest(&manifest);
        assert!(
            report.valid,
            "preview should be optional, got errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn fit_mode_serialization() {
        let fit = FitMode::Cover;
        let json = serde_json::to_string(&fit).expect("serialize");
        assert_eq!(json, "\"cover\"");

        let fit = FitMode::Contain;
        let json = serde_json::to_string(&fit).expect("serialize");
        assert_eq!(json, "\"contain\"");
    }

    #[test]
    fn fit_mode_deserialization() {
        let fit: FitMode = serde_json::from_str("\"cover\"").expect("deserialize");
        assert_eq!(fit, FitMode::Cover);

        let fit: FitMode = serde_json::from_str("\"stretch\"").expect("deserialize");
        assert_eq!(fit, FitMode::Stretch);
    }

    #[test]
    fn manifest_json_roundtrip() {
        let manifest = valid_static_manifest();
        let json = serde_json::to_string_pretty(&manifest).expect("serialize");
        let decoded: WallpaperManifest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(manifest, decoded);
    }

    #[test]
    fn path_traversal_absolute_rejected() {
        let result = validate_relative_path("/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn path_traversal_parent_dir_rejected() {
        let result = validate_relative_path("../../secret");
        assert!(result.is_err());
    }

    #[test]
    fn valid_relative_path_ok() {
        let result = validate_relative_path("content/wallpaper.png");
        assert!(result.is_ok());
    }

    #[test]
    fn preview_path_traversal_rejected() {
        let mut manifest = valid_static_manifest();
        manifest.preview = Some("../../secret.png".into());
        let report = validate_manifest(&manifest);
        assert!(!report.valid);
        assert!(report
            .errors
            .iter()
            .any(|e| e.contains("traversal") || e.contains("invalid")));
    }

    #[test]
    fn validate_package_missing_image_file() {
        let dir = std::env::temp_dir().join("wallflow-test-missing-image");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::remove_file(dir.join("manifest.json"));
        let _ = std::fs::remove_dir_all(dir.join("content"));

        let manifest = valid_static_manifest();
        let json = serde_json::to_string_pretty(&manifest).expect("serialize");
        std::fs::write(dir.join("manifest.json"), &json).expect("write manifest");

        let package = crate::WallpaperPackage::load(&dir).expect("load");
        let report = validate_package(&package);
        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.contains("not found")));
    }

    #[test]
    fn validate_package_with_image_file() {
        let dir = std::env::temp_dir().join("wallflow-test-with-image");
        let _ = std::fs::create_dir_all(dir.join("content"));
        let _ = std::fs::create_dir_all(&dir);

        let manifest = valid_static_manifest();
        let json = serde_json::to_string_pretty(&manifest).expect("serialize");
        std::fs::write(dir.join("manifest.json"), &json).expect("write manifest");
        // Create a dummy image file
        std::fs::write(dir.join("content/wallpaper.png"), b"fake-png-data").expect("write image");
        // Create a dummy preview file
        std::fs::write(dir.join("preview.png"), b"fake-preview-data").expect("write preview");

        let package = crate::WallpaperPackage::load(&dir).expect("load");
        let report = validate_package(&package);
        assert!(
            report.valid,
            "expected valid, got errors: {:?}",
            report.errors
        );
    }
}
