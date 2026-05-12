use crate::error::{Error, Result};
use std::path::Path;

pub fn move_chapter_to_trash(_workspace_path: &Path, _chapter_id: &str) -> Result<()> {
    Err(Error::NotImplemented)
}
