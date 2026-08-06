use std::path::PathBuf;

use crate::error::Result;
use crate::storage::atomic_write_string;

use super::types::*;
use super::StarMapStore;

impl StarMapStore {
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub(super) fn load_recovery_from_disk(&mut self) {
        let path = self.metadata_dir().join("recovery.json");
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(log) = serde_json::from_str::<Vec<LoadDiagnostic>>(&content) {
                    self.recovery_log = log;
                }
            }
        }
    }

    pub(super) fn flush_recovery_to_disk(&self) -> Result<()> {
        let dir = self.metadata_dir();
        std::fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(&self.recovery_log)?;
        let path = dir.join("recovery.json");
        atomic_write_string(&path, &json)?;
        Ok(())
    }

    pub(super) fn metadata_dir(&self) -> PathBuf {
        self.starmap_dir().join("metadata")
    }

    pub(super) fn starmap_dir(&self) -> PathBuf {
        self.workspace
            .join("app-meta")
            .join("starmaps")
            .join(&self.starmap_id)
    }
}
