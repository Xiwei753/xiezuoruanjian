use crate::error::Result;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use uuid::Uuid;

pub fn atomic_write_string(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension(format!("tmp.{}", Uuid::new_v4()));
    {
        let file = File::create(&tmp_path)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(content.as_bytes())?;
        writer.flush()?;
        let file = writer.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
    }

    fs::rename(&tmp_path, path)?;

    Ok(())
}
