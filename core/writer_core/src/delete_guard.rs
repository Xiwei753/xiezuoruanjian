use std::path::{Path, PathBuf};
use crate::error::{Error, Result};

pub fn validate_id_segment(id: &str) -> Result<&str> {
    let id = id.trim();
    if id.is_empty() {
        return Err(Error::InvalidDeleteTarget("ID cannot be empty".to_string()));
    }
    if id.contains('/') || id.contains('\\') || id == ".." || id == "." {
        return Err(Error::InvalidDeleteTarget(format!("ID contains invalid characters: {}", id)));
    }
    Ok(id)
}

pub fn validate_delete_target(
    workspace_path: &Path,
    target_path: &Path,
    expected_marker_file: &str,
) -> Result<PathBuf> {
    // Canonicalize workspace path to prevent .. escaping
    let workspace_canon = workspace_path
        .canonicalize()
        .map_err(|e| Error::InvalidDeleteTarget(format!("Failed to canonicalize workspace: {}", e)))?;

    // The target_path might not be canonicalizable if we only rely on canonicalize because some parts might be symlinks,
    // but target MUST exist before deleting.
    if !target_path.exists() {
        return Err(Error::InvalidDeleteTarget("Target path does not exist".to_string()));
    }

    let target_canon = target_path
        .canonicalize()
        .map_err(|e| Error::InvalidDeleteTarget(format!("Failed to canonicalize target: {}", e)))?;

    if target_canon == workspace_canon {
        return Err(Error::RefuseToDeleteWorkspaceRoot);
    }

    if !target_canon.starts_with(&workspace_canon) {
        return Err(Error::InvalidDeleteTarget("Target is outside the workspace".to_string()));
    }

    let marker = target_canon.join(expected_marker_file);
    if !marker.exists() {
        return Err(Error::InvalidDeleteTarget(format!("Marker file {} not found in target", expected_marker_file)));
    }

    Ok(target_canon)
}
