use super::workspace::*;
use crate::workspace::RecentEdit;

#[test]
fn test_recent_edit_dto_conversion() {
    let edit = RecentEdit {
        project_id: "proj_123".to_string(),
        volume_id: "vol_456".to_string(),
        chapter_id: "chap_789".to_string(),
        timestamp: "2023-10-01T12:00:00Z".to_string(),
    };
    let dto: RecentEditDto = edit.clone().into();
    assert_eq!(dto.project_id, edit.project_id);
    assert_eq!(dto.volume_id, edit.volume_id);
    assert_eq!(dto.chapter_id, edit.chapter_id);
    assert_eq!(dto.timestamp, edit.timestamp);
}

#[test]
fn test_workspace_diagnostics_dto_contract() {
    let dto = crate::api::types::WorkspaceDiagnosticsDto {
        has_workspace: true,
        workspace_path: "/workspace".to_string(),
        core_initialized: true,
        path_exists: true,
        is_dir: true,
        manifest_path: "/workspace/manifest.json".to_string(),
        manifest_exists: true,
        projects_path: "/workspace/projects".to_string(),
        projects_dir_exists: true,
        app_meta_exists: true,
        writable: true,
        writable_error: "".to_string(),
        validate_workspace: true,
        tree_count: 5,
        last_workspace_path: "/last".to_string(),
        create_project_available: true,
    };
    let json = serde_json::to_value(&dto).unwrap();
    assert!(json.get("hasWorkspace").is_some());
    assert!(json.get("coreInitialized").is_some());
    assert!(json.get("createProjectAvailable").is_some());
    assert!(json.get("manifestExists").is_some());
    assert!(json.get("projectsDirExists").is_some());
    assert!(json.get("treeCount").is_some());
    assert!(json.get("validateWorkspace").is_some());
    assert!(json.get("writableError").is_some());
    let deserialized: crate::api::types::WorkspaceDiagnosticsDto =
        serde_json::from_value(json).unwrap();
    assert_eq!(dto, deserialized);
}

#[test]
fn test_recent_edit_dto_serialization_roundtrip() {
    let dto = RecentEditDto {
        project_id: "p1".to_string(),
        volume_id: "v1".to_string(),
        chapter_id: "c1".to_string(),
        timestamp: "2023-10-10".to_string(),
    };
    let json = serde_json::to_value(&dto).unwrap();
    assert!(json.get("project_id").is_some());
    assert!(json.get("volume_id").is_some());
    assert!(json.get("chapter_id").is_some());
    let deserialized: RecentEditDto = serde_json::from_value(json).unwrap();
    assert_eq!(dto, deserialized);
}
