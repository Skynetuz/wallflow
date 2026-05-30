use serde::{Deserialize, Serialize};

/// How a static image should be fitted to the screen.
///
/// This type is the canonical definition shared by wallflow-ipc and
/// wallflow-package. Both crates re-export it from wallflow-common.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FitMode {
    #[default]
    Cover,
    Contain,
    Stretch,
    Center,
    Tile,
}

impl std::fmt::Display for FitMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FitMode::Cover => write!(f, "cover"),
            FitMode::Contain => write!(f, "contain"),
            FitMode::Stretch => write!(f, "stretch"),
            FitMode::Center => write!(f, "center"),
            FitMode::Tile => write!(f, "tile"),
        }
    }
}

impl std::str::FromStr for FitMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "cover" => Ok(FitMode::Cover),
            "contain" => Ok(FitMode::Contain),
            "stretch" => Ok(FitMode::Stretch),
            "center" => Ok(FitMode::Center),
            "tile" => Ok(FitMode::Tile),
            _ => Err(()),
        }
    }
}
