//! Versioned module manifests. Relative object paths resolve beside the manifest.
use crate::args::Argument;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub object: PathBuf,
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default)]
    pub arguments: Vec<Argument>,
}

fn default_entry() -> String {
    "go".into()
}

impl Manifest {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes = crate::read_input(path)?;
        let mut manifest: Self = toml::from_str(std::str::from_utf8(&bytes)?)?;
        anyhow::ensure!(
            manifest.schema == 1,
            "unsupported manifest schema {}; expected 1",
            manifest.schema
        );
        anyhow::ensure!(
            !manifest.name.trim().is_empty(),
            "manifest name must not be empty"
        );
        anyhow::ensure!(
            !manifest.entry.is_empty() && !manifest.entry.contains('\0'),
            "invalid entrypoint"
        );
        if manifest.object.is_relative() {
            manifest.object = path
                .parent()
                .unwrap_or(Path::new("."))
                .join(&manifest.object);
        }
        Ok(manifest)
    }
}
