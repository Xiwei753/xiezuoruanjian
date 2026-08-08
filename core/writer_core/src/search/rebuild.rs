use std::path::Path;

use super::extractor;
use super::types::*;
use crate::error::Result;

pub fn rebuild_index(
    app_data_root: &Path,
    projects_root: &Path,
    project_id: Option<&str>,
) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();

    entries.extend(extractor::extract_chapter_entries(
        projects_root,
        project_id,
    )?);
    entries.extend(extractor::extract_starmap_entries(
        app_data_root,
        project_id,
    )?);
    entries.extend(extractor::extract_setting_entries(app_data_root)?);

    Ok(entries)
}
