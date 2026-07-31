use super::workspace::*;
use crate::workspace::RecentEdit;
use serde_json::json;

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
    assert_eq!(
        json,
        json!({
            "hasWorkspace": true,
            "workspacePath": "/workspace",
            "coreInitialized": true,
            "pathExists": true,
            "isDir": true,
            "manifestPath": "/workspace/manifest.json",
            "manifestExists": true,
            "projectsPath": "/workspace/projects",
            "projectsDirExists": true,
            "appMetaExists": true,
            "writable": true,
            "writableError": "",
            "validateWorkspace": true,
            "treeCount": 5,
            "lastWorkspacePath": "/last",
            "createProjectAvailable": true
        })
    );
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
    assert_eq!(
        json,
        json!({
            "projectId": "p1",
            "volumeId": "v1",
            "chapterId": "c1",
            "timestamp": "2023-10-10"
        })
    );
    let deserialized: RecentEditDto = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(dto, deserialized);
    let as_object = json.as_object().unwrap();
    assert_eq!(as_object.len(), 4, "RecentEditDto must have exactly 4 JSON keys");
}

#[test]
fn test_workspace_summary_dto_json_key_contract() {
    let dto = crate::api::types::WorkspaceSummaryDto {
        path: "/ws".to_string(),
        is_valid: true,
        projects: vec![crate::api::types::ProjectDto {
            id: "p1".to_string(),
            title: "P".to_string(),
            created_at: "2023-01-01".to_string(),
            updated_at: "2023-01-02".to_string(),
        }],
        recent_edits: vec![RecentEditDto {
            project_id: "p1".to_string(),
            volume_id: "v1".to_string(),
            chapter_id: "c1".to_string(),
            timestamp: "2023-01-01".to_string(),
        }],
    };
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(json["path"], "/ws");
    assert_eq!(json["isValid"], true);
    assert_eq!(json["projects"][0]["id"], "p1");
    assert_eq!(json["projects"][0]["title"], "P");
    assert_eq!(json["projects"][0]["createdAt"], "2023-01-01");
    assert_eq!(json["projects"][0]["updatedAt"], "2023-01-02");
    assert_eq!(json["recentEdits"][0]["projectId"], "p1");
    assert_eq!(json["recentEdits"][0]["volumeId"], "v1");
    assert_eq!(json["recentEdits"][0]["chapterId"], "c1");
    assert_eq!(json["recentEdits"][0]["timestamp"], "2023-01-01");
}
