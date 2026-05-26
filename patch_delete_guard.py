import re

with open("core/writer_core/src/delete_guard.rs", "r") as f:
    content = f.read()

replacement = """
    // Prevent deleting if the target itself is a symlink
    if let Ok(meta) = std::fs::symlink_metadata(target_path) {
        if meta.file_type().is_symlink() {
            return Err(Error::InvalidDeleteTarget("Target is a symlink, refusing to delete".to_string()));
        }
        if !meta.file_type().is_dir() {
            return Err(Error::InvalidDeleteTarget("Target is not a directory".to_string()));
        }
    } else {
        return Err(Error::InvalidDeleteTarget("Target path does not exist or cannot be accessed".to_string()));
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
    if let Ok(marker_meta) = std::fs::symlink_metadata(&marker) {
        if marker_meta.file_type().is_symlink() {
            return Err(Error::InvalidDeleteTarget(format!("Marker file {} is a symlink", expected_marker_file)));
        }
        if !marker_meta.file_type().is_file() {
            return Err(Error::InvalidDeleteTarget(format!("Marker file {} is not a regular file", expected_marker_file)));
        }
    } else {
        return Err(Error::InvalidDeleteTarget(format!("Marker file {} not found in target", expected_marker_file)));
    }
"""

pattern = r"    // The target_path might not be canonicalizable.*?return Err\(Error::InvalidDeleteTarget\(format!\(\"Marker file \{\} not found in target\", expected_marker_file\)\)\);\n    \}"

content = re.sub(pattern, replacement, content, flags=re.DOTALL)

with open("core/writer_core/src/delete_guard.rs", "w") as f:
    f.write(content)

