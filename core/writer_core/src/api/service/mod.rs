use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::api::error::WriterError;
use crate::api::types::*;
use crate::api::ResultEnvelope;
use crate::facade::WriterCore;

pub type ApiResult<T> = Result<T, WriterError>;

pub struct WriterCoreApi {
    pub(crate) workspace_path: PathBuf,
}

impl WriterCoreApi {
    pub fn new<P: AsRef<Path>>(workspace_path: P) -> Self {
        Self {
            workspace_path: workspace_path.as_ref().to_path_buf(),
        }
    }

    pub(crate) fn core(&self) -> WriterCore {
        WriterCore::new(&self.workspace_path)
    }

    pub(crate) fn json_string<T: Serialize>(value: &T) -> ApiResult<String> {
        serde_json::to_string(value).map_err(Into::into)
    }

    pub(crate) fn non_negative_counter(name: &str, value: i32) -> ApiResult<u32> {
        if value < 0 {
            return Err(WriterError::Other(format!(
                "negative writing event counter: {}={}",
                name, value
            )));
        }
        Ok(value as u32)
    }

    fn probe_workspace_writable(path: &Path, is_dir: bool) -> (bool, String) {
        if !is_dir {
            return (
                false,
                "path does not exist or is not a directory".to_string(),
            );
        }

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let test_file = path.join(format!(
            ".writer_write_test_{}_{}",
            std::process::id(),
            nonce
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&test_file)
        {
            Ok(mut file) => {
                if let Err(error) = file.write_all(b"test") {
                    let _ = std::fs::remove_file(&test_file);
                    return (false, error.to_string());
                }
                drop(file);
                let remove_result = std::fs::remove_file(&test_file);
                if let Err(error) = remove_result {
                    return (false, error.to_string());
                }
                (true, String::new())
            }
            Err(error) => (false, error.to_string()),
        }
    }
}

mod action_ops;
mod project_ops;
mod starmap_ops;
mod workspace_ops;
mod writing_stats_ops;



#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ChangedEntityDto;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn record_writing_event_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let result = api
            .record_writing_event(
                "dev-1",
                "proj1",
                "vol1",
                "chap1",
                "human_typed",
                10,
                0,
                0,
                0,
                0,
                "session-1",
            )
            .unwrap();

        assert!(result);
    }

    #[test]
    fn process_writing_event_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let result = api
            .process_writing_event(
                "dev-1",
                "android",
                "proj1",
                "vol1",
                "chap1",
                "old",
                "old text",
                0,
                "session-1",
            )
            .unwrap();

        assert!(result);
    }

    #[test]
    fn flush_writing_stats_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        assert!(api.flush_writing_stats().unwrap());
    }

    #[test]
    fn flush_recent_edits_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        assert!(api.flush_recent_edits().unwrap());
    }

    #[test]
    fn workspace_diagnostics_reports_core_owned_state() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let diagnostics = api.get_workspace_diagnostics(true, 3).unwrap();

        assert!(diagnostics.has_workspace);
        assert!(diagnostics.path_exists);
        assert!(diagnostics.is_dir);
        assert!(diagnostics.manifest_exists);
        assert!(diagnostics.projects_dir_exists);
        assert!(diagnostics.writable);
        assert!(diagnostics.validate_workspace);
        assert_eq!(diagnostics.tree_count, 3);
        assert!(diagnostics.create_project_available);
    }

    #[test]
    fn workspace_diagnostics_envelope_uses_camel_case_fields() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let result = api.get_workspace_diagnostics(true, 0);
        let json = ResultEnvelope::from_api_result(result).to_json_string();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["success"], true);
        assert_eq!(value["data"]["pathExists"], true);
        assert_eq!(value["data"]["createProjectAvailable"], true);
        assert!(value["data"].get("path_exists").is_none());
    }

    #[test]
    fn record_writing_event_for_platform_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let result = api
            .record_writing_event_for_platform(
                "dev-1",
                "linux",
                "proj1",
                "vol1",
                "chap1",
                "human_typed",
                10,
                0,
                0,
                0,
                0,
                "session-1",
            )
            .unwrap();

        assert!(result);
    }

    #[test]
    fn record_writing_event_rejects_negative_counter() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let err = api
            .record_writing_event(
                "dev-1",
                "proj1",
                "vol1",
                "chap1",
                "human_typed",
                0,
                -1,
                0,
                0,
                0,
                "session-1",
            )
            .unwrap_err();

        assert!(matches!(
            err,
            WriterError::Other(message)
                if message.contains("negative writing event counter")
                    && message.contains("deleted_chars=-1")
        ));
    }

    #[test]
    fn process_writing_event_propagates_core_error() {
        let temp_dir = tempdir().unwrap();
        let workspace_file = temp_dir.path().join("not_a_directory");
        File::create(&workspace_file).unwrap();
        let api = WriterCoreApi::new(&workspace_file);

        let err = api
            .process_writing_event(
                "dev-1",
                "android",
                "proj1",
                "vol1",
                "chap1",
                "old",
                "old text",
                0,
                "session-1",
            )
            .unwrap_err();

        assert!(matches!(err, WriterError::Io(_)));
    }

    #[test]
    fn compute_starmap_edge_renders_uses_core_geometry() {
        let api = WriterCoreApi::new("");
        let graph = StarMapGraphDto {
            schema_version: 1,
            id: "graph".to_string(),
            starmap_id: "map".to_string(),
            title: "Map".to_string(),
            nodes: vec![],
            edges: vec![StarMapEdgeDto {
                id: "edge-1".to_string(),
                from: Some("a".to_string()),
                to: Some("b".to_string()),
                kind: StarMapEdgeKindDto::RelatedTo,
                label: None,
                payload: None,
                from_target: None,
                to_target: None,
                from_endpoint: None,
                to_endpoint: None,
                from_endpoint_path: None,
                to_endpoint_path: None,
                created_at: 0,
                updated_at: 0,
            }],
            embeds: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let layout = StarMapLayoutDto {
            kind: StarMapLayoutKindDto::Freeform,
            nodes: vec![
                StarMapLayoutNodeDto {
                    node_id: "a".to_string(),
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                    radius: 16.0,
                    collapsed: false,
                    z_index: 0,
                    scale: 1.0,
                    depth: 0.0,
                    focus_weight: 1.0,
                    orbit_group: None,
                },
                StarMapLayoutNodeDto {
                    node_id: "b".to_string(),
                    x: 200.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                    radius: 16.0,
                    collapsed: false,
                    z_index: 0,
                    scale: 1.0,
                    depth: 0.0,
                    focus_weight: 1.0,
                    orbit_group: None,
                },
            ],
        };

        let renders = api.compute_starmap_edge_renders(graph, layout).unwrap();

        assert_eq!(renders.len(), 1);
        assert_eq!(renders[0].edge_id, "edge-1");
        assert!((renders[0].start_x - 92.0).abs() < 0.1);
        assert!((renders[0].end_x - 208.0).abs() < 0.1);
    }

    #[test]
    fn hit_test_starmap_node_returns_top_z_node() {
        let api = WriterCoreApi::new("");
        let layout = StarMapLayoutDto {
            kind: StarMapLayoutKindDto::Freeform,
            nodes: vec![
                StarMapLayoutNodeDto {
                    node_id: "lower".to_string(),
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                    radius: 16.0,
                    collapsed: false,
                    z_index: 1,
                    scale: 1.0,
                    depth: 0.0,
                    focus_weight: 1.0,
                    orbit_group: None,
                },
                StarMapLayoutNodeDto {
                    node_id: "upper".to_string(),
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                    radius: 16.0,
                    collapsed: false,
                    z_index: 2,
                    scale: 1.0,
                    depth: 0.0,
                    focus_weight: 1.0,
                    orbit_group: None,
                },
            ],
        };

        let hit = api.hit_test_starmap_node(layout, 50.0, 50.0).unwrap();

        assert_eq!(hit.as_deref(), Some("upper"));
    }

    #[test]
    fn save_chapter_content_envelope_returns_success_with_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch 1").unwrap();

        let result = api.save_chapter_content(&project.id, &volume.id, &chapter.id, "Hello World");
        let json = match result {
            Ok(receipt) => ResultEnvelope::success_with_changes(
                receipt,
                Vec::new(),
                vec![ChangedEntityDto { entity_type: "ChapterSaved".to_string(), entity_id: Some(chapter.id.clone()) }],
            ),
            Err(error) => ResultEnvelope::<crate::api::types::ChapterSaveReceiptDto>::error(error),
        }.to_json_string();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["success"], true);
        assert!(value["data"].is_object());
        assert_eq!(value["data"]["content_len"], 11u32);
        assert!(value["changedEntities"].is_array());
        assert_eq!(value["changedEntities"][0]["entityType"], "ChapterSaved");
        assert_eq!(value["changedEntities"][0]["entityId"], chapter.id);
    }

    #[test]
    fn create_project_volume_chapter_envelope_returns_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let project = api.create_project("Test").unwrap();
        let project_id = project.id.clone();
        let project_json = ResultEnvelope::success_with_changes(
            project,
            Vec::new(),
            vec![ChangedEntityDto { entity_type: "ProjectCreated".to_string(), entity_id: Some(project_id.clone()) }],
        ).to_json_string();
        let project_value: serde_json::Value = serde_json::from_str(&project_json).unwrap();
        assert_eq!(project_value["success"], true);
        assert_eq!(project_value["changedEntities"][0]["entityType"], "ProjectCreated");
        assert_eq!(project_value["changedEntities"][0]["entityId"], project_id);

        let volume = api.create_volume(&project_id, "Vol 1").unwrap();
        let volume_id = volume.id.clone();
        let volume_json = ResultEnvelope::success_with_changes(
            volume,
            Vec::new(),
            vec![ChangedEntityDto { entity_type: "VolumeCreated".to_string(), entity_id: Some(volume_id.clone()) }],
        ).to_json_string();
        let volume_value: serde_json::Value = serde_json::from_str(&volume_json).unwrap();
        assert_eq!(volume_value["success"], true);
        assert_eq!(volume_value["changedEntities"][0]["entityType"], "VolumeCreated");
        assert_eq!(volume_value["changedEntities"][0]["entityId"], volume_id);

        let chapter = api.create_chapter(&project_id, &volume_id, "Ch 1").unwrap();
        let chapter_id = chapter.id.clone();
        let chapter_json = ResultEnvelope::success_with_changes(
            chapter,
            Vec::new(),
            vec![ChangedEntityDto { entity_type: "ChapterCreated".to_string(), entity_id: Some(chapter_id.clone()) }],
        ).to_json_string();
        let chapter_value: serde_json::Value = serde_json::from_str(&chapter_json).unwrap();
        assert_eq!(chapter_value["success"], true);
        assert_eq!(chapter_value["changedEntities"][0]["entityType"], "ChapterCreated");
        assert_eq!(chapter_value["changedEntities"][0]["entityId"], chapter_id);
    }

    #[test]
    fn project_volume_chapter_mutation_envelopes_return_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch 1").unwrap();
        let volume_ids: Vec<String> = api
            .list_volumes(&project.id)
            .unwrap()
            .into_iter()
            .map(|volume| volume.id)
            .collect();

        let cases: Vec<(String, &str, Option<String>)> = vec![
            {
                let result = api.rename_project(&project.id, "Renamed");
                let json = match result {
                    Ok(data) => ResultEnvelope::success_with_changes(data, Vec::new(), vec![ChangedEntityDto { entity_type: "ProjectRenamed".to_string(), entity_id: Some(project.id.clone()) }]),
                    Err(error) => ResultEnvelope::<bool>::error(error),
                }.to_json_string();
                (json, "ProjectRenamed", Some(project.id.clone()))
            },
            {
                let result = api.reorder_projects(&vec![project.id.clone()]);
                let json = match result {
                    Ok(data) => ResultEnvelope::success_with_changes(data, Vec::new(), vec![ChangedEntityDto { entity_type: "ProjectsReordered".to_string(), entity_id: None }]),
                    Err(error) => ResultEnvelope::<bool>::error(error),
                }.to_json_string();
                (json, "ProjectsReordered", None)
            },
            {
                let result = api.rename_volume(&project.id, &volume.id, "Vol 2");
                let json = match result {
                    Ok(data) => ResultEnvelope::success_with_changes(data, Vec::new(), vec![ChangedEntityDto { entity_type: "VolumeRenamed".to_string(), entity_id: Some(volume.id.clone()) }]),
                    Err(error) => ResultEnvelope::<bool>::error(error),
                }.to_json_string();
                (json, "VolumeRenamed", Some(volume.id.clone()))
            },
            {
                let result = api.reorder_volumes(&project.id, &volume_ids);
                let json = match result {
                    Ok(data) => ResultEnvelope::success_with_changes(data, Vec::new(), vec![ChangedEntityDto { entity_type: "VolumesReordered".to_string(), entity_id: Some(project.id.clone()) }]),
                    Err(error) => ResultEnvelope::<bool>::error(error),
                }.to_json_string();
                (json, "VolumesReordered", Some(project.id.clone()))
            },
            {
                let result = api.rename_chapter(&project.id, &volume.id, &chapter.id, "Ch 2");
                let json = match result {
                    Ok(data) => ResultEnvelope::success_with_changes(data, Vec::new(), vec![ChangedEntityDto { entity_type: "ChapterRenamed".to_string(), entity_id: Some(chapter.id.clone()) }]),
                    Err(error) => ResultEnvelope::<bool>::error(error),
                }.to_json_string();
                (json, "ChapterRenamed", Some(chapter.id.clone()))
            },
            {
                let result = api.reorder_chapters(&project.id, &volume.id, &vec![chapter.id.clone()]);
                let json = match result {
                    Ok(data) => ResultEnvelope::success_with_changes(data, Vec::new(), vec![ChangedEntityDto { entity_type: "ChaptersReordered".to_string(), entity_id: Some(volume.id.clone()) }]),
                    Err(error) => ResultEnvelope::<bool>::error(error),
                }.to_json_string();
                (json, "ChaptersReordered", Some(volume.id.clone()))
            },
            {
                let result = api.update_chapter_note(&project.id, &volume.id, &chapter.id, "note");
                let json = match result {
                    Ok(data) => ResultEnvelope::success_with_changes(data, Vec::new(), vec![ChangedEntityDto { entity_type: "ChapterNoteUpdated".to_string(), entity_id: Some(chapter.id.clone()) }]),
                    Err(error) => ResultEnvelope::<bool>::error(error),
                }.to_json_string();
                (json, "ChapterNoteUpdated", Some(chapter.id.clone()))
            },
            {
                let result = api.clear_chapter_content(&project.id, &volume.id, &chapter.id);
                let json = match result {
                    Ok(data) => ResultEnvelope::success_with_changes(data, Vec::new(), vec![ChangedEntityDto { entity_type: "ChapterCleared".to_string(), entity_id: Some(chapter.id.clone()) }]),
                    Err(error) => ResultEnvelope::<crate::api::ChapterSaveReceiptDto>::error(error),
                }.to_json_string();
                (json, "ChapterCleared", Some(chapter.id.clone()))
            },
        ];

        for (json, entity_type, entity_id) in cases {
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(
                value["success"], true,
                "{entity_type} envelope failed: {json}"
            );
            assert_eq!(value["changedEntities"][0]["entityType"], entity_type);
            match entity_id {
                Some(id) => assert_eq!(value["changedEntities"][0]["entityId"], id),
                None => assert!(value["changedEntities"][0].get("entityId").is_none()),
            }
        }
    }

    #[test]
    fn delete_chapter_envelope_returns_success_with_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch 1").unwrap();

        let result = api.delete_chapter(&project.id, &volume.id, &chapter.id);
        let json = match result {
            Ok(_) => ResultEnvelope::success_with_changes(
                true,
                Vec::new(),
                vec![ChangedEntityDto { entity_type: "ChapterDeleted".to_string(), entity_id: Some(chapter.id.clone()) }],
            ),
            Err(error) => ResultEnvelope::<bool>::error(error),
        }.to_json_string();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["success"], true);
        assert_eq!(value["data"], true);
        assert_eq!(value["changedEntities"][0]["entityType"], "ChapterDeleted");
        assert_eq!(value["changedEntities"][0]["entityId"], chapter.id);
    }

    #[test]
    fn delete_project_envelope_returns_success_with_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("Test").unwrap();

        let result = api.delete_project(&project.id);
        let json = match result {
            Ok(_) => ResultEnvelope::success_with_changes(
                true,
                Vec::new(),
                vec![ChangedEntityDto { entity_type: "ProjectDeleted".to_string(), entity_id: Some(project.id.clone()) }],
            ),
            Err(error) => ResultEnvelope::<bool>::error(error),
        }.to_json_string();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["success"], true);
        assert_eq!(value["changedEntities"][0]["entityType"], "ProjectDeleted");
        assert_eq!(value["changedEntities"][0]["entityId"], project.id);
    }

    #[test]
    fn save_sync_config_envelope_returns_success_with_changed_entities() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let config = crate::api::types::SyncConfigDto {
            enabled: true,
            backend_type: "github_api".to_string(),
            remote_url: "https://github.com/test/repo.git".to_string(),
            transport: "https_token".to_string(),
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            proxy_enabled: false,
            proxy_type: "auto".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            username: "".to_string(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let result = api.save_sync_config(config);
        let json = match result {
            Ok(data) => ResultEnvelope::success_with_changes(
                data,
                vec!["sync_config.json".to_string()],
                vec![ChangedEntityDto { entity_type: "SyncConfigSaved".to_string(), entity_id: None }],
            ),
            Err(error) => ResultEnvelope::<bool>::error(error),
        }.to_json_string();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["success"], true);
        assert_eq!(value["data"], true);
        assert_eq!(value["changedEntities"][0]["entityType"], "SyncConfigSaved");
    }

    #[test]
    fn end_to_end_api_chapter_write_reopen_verify() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("E2E Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch 1").unwrap();

        let content = "端到端测试内容。\n第二行文本。\n第三行。";
        let receipt = api
            .save_chapter_content(&project.id, &volume.id, &chapter.id, content)
            .unwrap();

        assert_eq!(receipt.content_len, content.len() as u32);
        assert!(receipt.word_count > 0);
        assert!(!receipt.content_hash.is_empty());

        let reopened = api
            .open_chapter(&project.id, &volume.id, &chapter.id)
            .unwrap();
        assert_eq!(reopened.content, content);
        assert_eq!(reopened.meta.hash, receipt.content_hash);
        assert_eq!(reopened.meta.word_count, receipt.word_count);
    }

    #[test]
    fn api_save_chapter_content_with_options_allow_empty_true() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("E2E Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch 1").unwrap();

        let initial_content = "Some initial content";
        api.save_chapter_content(&project.id, &volume.id, &chapter.id, initial_content)
            .unwrap();

        let receipt = api
            .save_chapter_content_with_options(&project.id, &volume.id, &chapter.id, "", true)
            .unwrap();

        assert_eq!(receipt.content_len, 0);
        assert_eq!(receipt.word_count, 0);

        let reopened = api
            .open_chapter(&project.id, &volume.id, &chapter.id)
            .unwrap();
        assert_eq!(reopened.content, "");
    }

    #[test]
    fn api_save_chapter_content_with_options_allow_empty_false() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());
        let project = api.create_project("E2E Test").unwrap();
        let volume = api.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch 1").unwrap();

        let initial_content = "Some initial content";
        api.save_chapter_content(&project.id, &volume.id, &chapter.id, initial_content)
            .unwrap();

        let result = api.save_chapter_content_with_options(
            &project.id,
            &volume.id,
            &chapter.id,
            "",
            false,
        );
        assert!(result.is_err());
    }
}