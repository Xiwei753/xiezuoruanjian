//! # 删除安全守卫（Core 层）
//!
//! 所有删除操作（项目、卷、章节）必须经过此模块验证。
//!
//! ## 安全机制
//!
//! 1. **ID 段验证**：防止路径穿越攻击（`..`、`/`、`\`）
//! 2. **符号链接检查**：拒绝删除符号链接指向的目标
//! 3. **根目录保护**：绝对不允许删除根目录
//! 4. **标记文件验证**：确认目标是正确的业务目录（通过 marker file）
//!
//! ## 调用方
//!
//! - `project::delete_project()` → `validate_delete_target(..., "project.json")`
//! - `volume::delete_volume()` → `validate_delete_target(..., "volume.json")`
//! - `chapter::delete_chapter()` → `validate_delete_target(..., "chapter.meta.json")`

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// 验证 ID 段是否安全（不含路径分隔符和 `..`）。
///
/// 所有从外部接收的 ID（project_id、volume_id、chapter_id）都必须先经过此验证。
pub fn validate_id_segment(id: &str) -> Result<&str> {
    let id = id.trim();
    if id.is_empty() {
        return Err(Error::InvalidDeleteTarget("ID cannot be empty".to_string()));
    }
    if id.contains('/') || id.contains('\\') || id == ".." || id == "." {
        return Err(Error::InvalidDeleteTarget(format!(
            "ID contains invalid characters: {}",
            id
        )));
    }
    Ok(id)
}

/// 验证删除目标是否合法。
///
/// 检查项：
/// 1. 目标路径存在且是目录（非符号链接）
/// 2. 目标不在根目录
/// 3. 目标在根目录内部（防止 `..` 逃逸）——通过 `canonicalize` 比较
/// 4. 目标包含预期的标记文件（如 `project.json`），且标记文件不是符号链接
///
/// 返回 canonicalize 后的目标路径，后续删除操作应使用此路径而非原始路径，
/// 防止 TOCTOU 竞态。
pub fn validate_delete_target(
    root_path: &Path,
    target_path: &Path,
    expected_marker_file: &str,
) -> Result<PathBuf> {
    // Canonicalize root path to prevent .. escaping
    let root_canon = root_path.canonicalize().map_err(|e| {
        Error::InvalidDeleteTarget(format!("Failed to canonicalize root: {}", e))
    })?;

    // Prevent deleting if the target itself is a symlink
    if let Ok(meta) = std::fs::symlink_metadata(target_path) {
        if meta.file_type().is_symlink() {
            return Err(Error::InvalidDeleteTarget(
                "Target is a symlink, refusing to delete".to_string(),
            ));
        }
        if !meta.file_type().is_dir() {
            return Err(Error::InvalidDeleteTarget(
                "Target is not a directory".to_string(),
            ));
        }
    } else {
        return Err(Error::InvalidDeleteTarget(
            "Target path does not exist or cannot be accessed".to_string(),
        ));
    }

    let target_canon = target_path
        .canonicalize()
        .map_err(|e| Error::InvalidDeleteTarget(format!("Failed to canonicalize target: {}", e)))?;

    if target_canon == root_canon {
        return Err(Error::RefuseToDeleteRoot);
    }

    if !target_canon.starts_with(&root_canon) {
        return Err(Error::InvalidDeleteTarget(
            "Target is outside the root".to_string(),
        ));
    }

    let marker = target_canon.join(expected_marker_file);
    if let Ok(marker_meta) = std::fs::symlink_metadata(&marker) {
        if marker_meta.file_type().is_symlink() {
            return Err(Error::InvalidDeleteTarget(format!(
                "Marker file {} is a symlink",
                expected_marker_file
            )));
        }
        if !marker_meta.file_type().is_file() {
            return Err(Error::InvalidDeleteTarget(format!(
                "Marker file {} is not a regular file",
                expected_marker_file
            )));
        }
    } else {
        return Err(Error::InvalidDeleteTarget(format!(
            "Marker file {} not found in target",
            expected_marker_file
        )));
    }

    Ok(target_canon)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_validate_id_segment() {
        assert_eq!(validate_id_segment("valid_id").unwrap(), "valid_id");
        assert_eq!(validate_id_segment(" valid_id ").unwrap(), "valid_id");

        match validate_id_segment("") {
            Err(Error::InvalidDeleteTarget(msg)) => assert_eq!(msg, "ID cannot be empty"),
            _ => panic!("Expected InvalidDeleteTarget error for empty ID"),
        }

        match validate_id_segment("   ") {
            Err(Error::InvalidDeleteTarget(msg)) => assert_eq!(msg, "ID cannot be empty"),
            _ => panic!("Expected InvalidDeleteTarget error for spaces"),
        }

        match validate_id_segment("with/slash") {
            Err(Error::InvalidDeleteTarget(msg)) => {
                assert_eq!(msg, "ID contains invalid characters: with/slash")
            }
            _ => panic!("Expected InvalidDeleteTarget error for slash"),
        }

        match validate_id_segment("with\\backslash") {
            Err(Error::InvalidDeleteTarget(msg)) => {
                assert_eq!(msg, "ID contains invalid characters: with\\backslash")
            }
            _ => panic!("Expected InvalidDeleteTarget error for backslash"),
        }

        match validate_id_segment("..") {
            Err(Error::InvalidDeleteTarget(msg)) => {
                assert_eq!(msg, "ID contains invalid characters: ..")
            }
            _ => panic!("Expected InvalidDeleteTarget error for .."),
        }

        match validate_id_segment(".") {
            Err(Error::InvalidDeleteTarget(msg)) => {
                assert_eq!(msg, "ID contains invalid characters: .")
            }
            _ => panic!("Expected InvalidDeleteTarget error for ."),
        }
    }

    #[test]
    fn test_validate_delete_target_success() {
        let workspace = tempdir().unwrap();
        let target = workspace.path().join("target_dir");
        fs::create_dir(&target).unwrap();
        let marker = target.join("marker.txt");
        fs::write(&marker, "marker").unwrap();

        let res = validate_delete_target(workspace.path(), &target, "marker.txt");
        assert!(res.is_ok());
    }

    #[test]
    fn test_validate_delete_target_missing_target() {
        let workspace = tempdir().unwrap();
        let target = workspace.path().join("target_dir");

        let res = validate_delete_target(workspace.path(), &target, "marker.txt");
        assert!(res.is_err());
    }

    #[test]
    fn test_validate_delete_target_not_dir() {
        let workspace = tempdir().unwrap();
        let target = workspace.path().join("target_file.txt");
        fs::write(&target, "content").unwrap();

        let res = validate_delete_target(workspace.path(), &target, "marker.txt");
        assert!(res.is_err());
    }

    #[test]
    fn test_validate_delete_target_is_workspace_root() {
        let workspace = tempdir().unwrap();
        let marker = workspace.path().join("marker.txt");
        fs::write(&marker, "marker").unwrap();

        let res = validate_delete_target(workspace.path(), workspace.path(), "marker.txt");
        assert!(res.is_err());
        // Can be more specific to ensure Error::RefuseToDeleteRoot
    }

    #[test]
    fn test_validate_delete_target_outside_workspace() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let target = outside.path().join("target_dir");
        fs::create_dir(&target).unwrap();
        let marker = target.join("marker.txt");
        fs::write(&marker, "marker").unwrap();

        let res = validate_delete_target(workspace.path(), &target, "marker.txt");
        assert!(res.is_err());
    }

    #[test]
    fn test_validate_delete_target_missing_marker() {
        let workspace = tempdir().unwrap();
        let target = workspace.path().join("target_dir");
        fs::create_dir(&target).unwrap();

        let res = validate_delete_target(workspace.path(), &target, "marker.txt");
        assert!(res.is_err());
    }

    #[test]
    fn test_validate_delete_target_marker_not_file() {
        let workspace = tempdir().unwrap();
        let target = workspace.path().join("target_dir");
        fs::create_dir(&target).unwrap();
        let marker = target.join("marker.txt");
        fs::create_dir(&marker).unwrap(); // marker is a directory

        let res = validate_delete_target(workspace.path(), &target, "marker.txt");
        assert!(res.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_validate_delete_target_symlinks() {
        use std::os::unix::fs::symlink;
        let workspace = tempdir().unwrap();
        let actual_target = workspace.path().join("actual_target");
        fs::create_dir(&actual_target).unwrap();

        let symlink_target = workspace.path().join("symlink_target");
        symlink(&actual_target, &symlink_target).unwrap();

        // 1. Target is a symlink
        let res = validate_delete_target(workspace.path(), &symlink_target, "marker.txt");
        assert!(res.is_err()); // "Target is a symlink, refusing to delete"

        // 2. Marker is a symlink
        let marker_target = workspace.path().join("real_marker.txt");
        fs::write(&marker_target, "content").unwrap();
        let marker_symlink = actual_target.join("marker.txt");
        symlink(&marker_target, &marker_symlink).unwrap();

        let res = validate_delete_target(workspace.path(), &actual_target, "marker.txt");
        assert!(res.is_err()); // "Marker file ... is a symlink"
    }

    #[cfg(unix)]
    #[test]
    fn test_validate_delete_target_symlink_traversal() {
        use std::os::unix::fs::symlink;
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_target = outside.path().join("outside_target");
        fs::create_dir(&outside_target).unwrap();
        let marker = outside_target.join("marker.txt");
        fs::write(&marker, "marker").unwrap();

        // Create a symlink inside the workspace pointing to the outside target
        let symlink_target = workspace.path().join("symlinked_target");
        symlink(&outside_target, &symlink_target).unwrap();

        // Ensure validate_delete_target rejects the symlink
        let res = validate_delete_target(workspace.path(), &symlink_target, "marker.txt");
        assert!(res.is_err());
        match res {
            Err(Error::InvalidDeleteTarget(msg)) => {
                assert!(msg.contains("Target is a symlink"));
            }
            _ => panic!("Expected InvalidDeleteTarget error"),
        }
    }
}
