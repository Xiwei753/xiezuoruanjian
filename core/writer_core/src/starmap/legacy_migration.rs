use crate::error::{Error, Result};
use std::fs;
use std::path::{Path, PathBuf};

fn backup_path(p: &Path) -> PathBuf {
    PathBuf::from(format!("{}.legacy.backup.json", p.display()))
}

/// Isolate old MindMap data by renaming files to `.legacy.backup.json`.
///
/// This is NOT a real migration — it does not create StarMap data.
/// It only prevents old MindMap files from being read at runtime.
/// Old files are preserved as backups, never deleted.
pub fn quarantine_project_mind_map(workspace: &Path, project_id: &str) -> Result<Option<String>> {
    let mm_dir = workspace.join("projects").join(project_id).join("mind_map");

    if !mm_dir.exists() {
        return Ok(None);
    }

    let index_path = mm_dir.join("index.json");
    if !index_path.exists() {
        return Ok(None);
    }

    let index_content = fs::read_to_string(&index_path).map_err(Error::Io)?;
    let _index: serde_json::Value = serde_json::from_str(&index_content).map_err(Error::Json)?;

    let graphs_dir = mm_dir.join("graphs");
    if graphs_dir.exists() {
        if let Ok(entries) = fs::read_dir(&graphs_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("json") {
                    let backup = backup_path(&p);
                    if !backup.exists() {
                        let _ = fs::rename(&p, &backup);
                    }
                }
            }
        }
    }

    let layouts_dir = mm_dir.join("layouts");
    if layouts_dir.exists() {
        if let Ok(entries) = fs::read_dir(&layouts_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("json") {
                    let backup = backup_path(&p);
                    if !backup.exists() {
                        let _ = fs::rename(&p, &backup);
                    }
                }
            }
        }
    }

    let root_mm_json = workspace
        .join("projects")
        .join(project_id)
        .join("mind_map.json");
    if root_mm_json.exists() {
        let backup = backup_path(&root_mm_json);
        if !backup.exists() {
            let _ = fs::rename(&root_mm_json, &backup);
        }
    }

    let backup = backup_path(&index_path);
    if !backup.exists() {
        let _ = fs::rename(&index_path, &backup);
    }

    Ok(Some(format!(
        "Quarantined MindMap data for project {}",
        project_id
    )))
}

pub fn list_legacy_mind_map_paths(workspace: &Path, project_id: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mm_dir = workspace.join("projects").join(project_id).join("mind_map");
    if mm_dir.exists() {
        paths.push(format!("projects/{}/mind_map/index.json", project_id));
        if mm_dir.join("graphs").exists() {
            paths.push(format!("projects/{}/mind_map/graphs/", project_id));
        }
        if mm_dir.join("layouts").exists() {
            paths.push(format!("projects/{}/mind_map/layouts/", project_id));
        }
    }
    let root_mm = workspace
        .join("projects")
        .join(project_id)
        .join("mind_map.json");
    if root_mm.exists() {
        paths.push(format!("projects/{}/mind_map.json", project_id));
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_list_legacy_paths_empty_when_no_data() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = list_legacy_mind_map_paths(tmp.path(), "proj1");
        assert!(paths.is_empty());
    }

    #[test]
    fn test_quarantine_returns_none_when_no_mm_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = quarantine_project_mind_map(tmp.path(), "proj1").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_quarantine_renames_old_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mm_dir = tmp.path().join("projects").join("proj1").join("mind_map");
        fs::create_dir_all(mm_dir.join("graphs")).unwrap();
        fs::write(mm_dir.join("index.json"), r#"{"graphs":[]}"#).unwrap();
        fs::write(mm_dir.join("graphs").join("g1.json"), "{}").unwrap();

        let result = quarantine_project_mind_map(tmp.path(), "proj1").unwrap();
        assert!(result.is_some());

        assert!(!mm_dir.join("index.json").exists());
        assert!(mm_dir.join("index.json.legacy.backup.json").exists());
        assert!(!mm_dir.join("graphs").join("g1.json").exists());
        assert!(mm_dir
            .join("graphs")
            .join("g1.json.legacy.backup.json")
            .exists());
    }
}
