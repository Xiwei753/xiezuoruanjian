use crate::error::{Error, Result};
use std::path::Path;

pub fn backup_project(_workspace_path: &Path, _project_id: &str) -> Result<()> {
    Err(Error::NotImplemented)
}
