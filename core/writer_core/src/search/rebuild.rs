use std::path::Path;

use super::extractor;
use super::types::*;
use crate::error::Result;

pub fn rebuild_index(workspace: &Path, project_id: Option<&str>) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();

    entries.extend(extractor::extract_chapter_entries(workspace, project_id)?);
    entries.extend(extractor::extract_starmap_entries(workspace, project_id)?);
    entries.extend(extractor::extract_setting_entries(workspace)?);

    Ok(entries)
}
