use crate::backup;
use crate::chapter::{self, Chapter, ChapterContent};
use crate::error::Result;
use crate::index;
use crate::project::{self, Project};
use crate::settings::{self, LocalSettings, SyncableSettings};
use crate::sync;
use crate::trash;
use crate::volume::{self, Volume};
use crate::workspace;
use std::path::{Path, PathBuf};
use crate::action_registry::{ActionDescriptor, ActionResult, ActionRegistry};
use serde_json::Value;

/// The main entry point for client applications (Android, Linux).
/// This struct holds the workspace root and provides high-level methods.
pub struct WriterCore {
    workspace_path: PathBuf,
}

impl WriterCore {
    /// Initialize the core with a workspace root directory.
    pub fn new<P: AsRef<Path>>(workspace_path: P) -> Self {
        Self {
            workspace_path: workspace_path.as_ref().to_path_buf(),
        }
    }

    /// Access to the workspace path for internal modules
    pub fn workspace_path(&self) -> &Path {
        &self.workspace_path
    }

    /// Create a new workspace.
    pub fn create_workspace(&self) -> Result<()> {
        workspace::create_workspace(&self.workspace_path)
    }

    /// Read the workspace manifest.
    pub fn validate_workspace(&self) -> Result<bool> {
        workspace::validate_workspace(&self.workspace_path)
    }

    /// List all projects in the workspace.

    pub fn get_mind_map_snapshot(&self, project_id: &str) -> Result<crate::mind_map::MindMapSnapshot> {
        crate::mind_map::generate_snapshot(self, project_id)
    }

    pub fn create_mind_map_graph(&self, project_id: &str, title: &str) -> Result<crate::mind_map::graph::MindMapGraph> {
        crate::mind_map::edit::create_mind_map_graph(self, project_id, title)
    }

    pub fn list_mind_map_graphs(&self, project_id: &str) -> Result<crate::mind_map::edit::MindMapGraphsList> {
        crate::mind_map::edit::list_mind_map_graphs(self, project_id)
    }

    pub fn set_default_mind_map_graph(&self, project_id: &str, graph_id: &str) -> Result<()> {
        crate::mind_map::edit::set_default_mind_map_graph(self, project_id, graph_id)
    }

    pub fn create_mind_map_node(&self, project_id: &str, graph_id: &str, node: crate::mind_map::graph::MindMapGraphNode) -> Result<crate::mind_map::graph::MindMapGraphNode> {
        crate::mind_map::edit::create_mind_map_node(self, project_id, graph_id, node)
    }

    pub fn update_mind_map_node(&self, project_id: &str, graph_id: &str, node_id: &str, patch: crate::mind_map::edit::MindMapGraphNodePatch) -> Result<crate::mind_map::graph::MindMapGraphNode> {
        crate::mind_map::edit::update_mind_map_node(self, project_id, graph_id, node_id, patch)
    }

    pub fn delete_mind_map_node(&self, project_id: &str, graph_id: &str, node_id: &str, cascade: bool) -> Result<()> {
        crate::mind_map::edit::delete_mind_map_node(self, project_id, graph_id, node_id, cascade)
    }

    pub fn create_mind_map_edge(&self, project_id: &str, graph_id: &str, edge: crate::mind_map::graph::MindMapGraphEdge) -> Result<crate::mind_map::graph::MindMapGraphEdge> {
        crate::mind_map::edit::create_mind_map_edge(self, project_id, graph_id, edge)
    }

    pub fn update_mind_map_edge(&self, project_id: &str, graph_id: &str, edge_id: &str, patch: crate::mind_map::edit::MindMapGraphEdgePatch) -> Result<crate::mind_map::graph::MindMapGraphEdge> {
        crate::mind_map::edit::update_mind_map_edge(self, project_id, graph_id, edge_id, patch)
    }

    pub fn delete_mind_map_edge(&self, project_id: &str, graph_id: &str, edge_id: &str) -> Result<()> {
        crate::mind_map::edit::delete_mind_map_edge(self, project_id, graph_id, edge_id)
    }

    pub fn create_mind_map_anchor(&self, project_id: &str, graph_id: &str, anchor: crate::mind_map::anchor::MindMapAnchor) -> Result<crate::mind_map::anchor::MindMapAnchor> {
        crate::mind_map::edit::create_mind_map_anchor(self, project_id, graph_id, anchor)
    }

    pub fn bind_mind_map_node_to_anchor(&self, project_id: &str, graph_id: &str, node_id: &str, anchor_id: &str, link_kind: &str) -> Result<crate::mind_map::anchor::MindMapLink> {
        crate::mind_map::edit::bind_mind_map_node_to_anchor(self, project_id, graph_id, node_id, anchor_id, link_kind)
    }

    pub fn save_mind_map_layout(&self, project_id: &str, graph_id: &str, layout: crate::mind_map::layout::MindMapLayout) -> Result<()> {
        crate::mind_map::edit::save_mind_map_layout(self, project_id, graph_id, layout)
    }

    pub fn list_registered_actions(&self) -> crate::error::Result<Vec<ActionDescriptor>> {
        let registry = ActionRegistry::new();
        Ok(registry.list_registered_actions())
    }

    pub fn get_action(&self, action_id: &str) -> crate::error::Result<Option<ActionDescriptor>> {
        let registry = ActionRegistry::new();
        Ok(registry.get_action(action_id))
    }

    pub fn execute_action(&self, action_id: &str, args_json: &str, _context_json: &str) -> crate::error::Result<ActionResult> {
        let registry = ActionRegistry::new();
        let _action = registry.get_action(action_id).ok_or_else(|| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Action not found: {}", action_id),
            ))
        })?;

        let args: Value = if args_json.trim().is_empty() {
            Value::Null
        } else {
            match serde_json::from_str(args_json) { Ok(v) => v, Err(_) => return Ok(ActionResult { success: false, message: Some("invalid args json".to_string()), data: None, proposed_ui: None, requires_confirmation: None }) }
        };

        match action_id {
            "settings.editor.font_size.get" => {
                let font_size = settings::get_effective_font_size(&self.workspace_path);
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(serde_json::json!({ "fontSize": font_size, "source": "syncable" })),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.font_size.set" => {
                if let Some(font_size) = args.get("fontSize").and_then(|v| v.as_f64()) {
                    if font_size < 10.0 || font_size > 72.0 {
                        return Ok(ActionResult {
                            success: false,
                            message: Some("Font size must be between 10 and 72".to_string()),
                            data: None,
                            proposed_ui: None,
                            requires_confirmation: None,
                        });
                    }
                    settings::set_editor_font_size(&self.workspace_path, font_size)?;
                    Ok(ActionResult {
                        success: true,
                        message: Some("Font size updated".to_string()),
                        data: Some(serde_json::json!({ "fontSize": font_size, "source": "syncable" })),
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                } else {
                    Ok(ActionResult {
                        success: false,
                        message: Some("Missing or invalid fontSize parameter".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "settings.editor.auto_save.get" => {
                let settings = self.load_local_settings()?;
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(serde_json::json!({ "enabled": settings.auto_save_enabled })),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.auto_save.set" => {
                if let Some(enabled) = args.get("enabled").and_then(|v| v.as_bool()) {
                    let mut settings = self.load_local_settings()?;
                    settings.auto_save_enabled = enabled;
                    self.save_local_settings(&settings)?;
                    Ok(ActionResult {
                        success: true,
                        message: Some("Auto save setting updated".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                } else {
                    Ok(ActionResult {
                        success: false,
                        message: Some("Missing or invalid enabled parameter".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "settings.editor.auto_save_delay.set" => {
                if let Some(delay) = args.get("delayMs").and_then(|v| v.as_u64()) {
                    if delay < 500 || delay > 10000 {
                        return Ok(ActionResult {
                            success: false,
                            message: Some("Delay must be between 500 and 10000".to_string()),
                            data: None,
                            proposed_ui: None,
                            requires_confirmation: None,
                        });
                    }
                    let mut settings = self.load_local_settings()?;
                    settings.auto_save_delay_ms = delay;
                    self.save_local_settings(&settings)?;
                    Ok(ActionResult {
                        success: true,
                        message: Some("Auto save delay updated".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                } else {
                    Ok(ActionResult {
                        success: false,
                        message: Some("Missing or invalid delayMs parameter".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "settings.sync.config.get" => {
                let config = self.load_sync_config()?;
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(serde_json::to_value(config).unwrap_or(Value::Null)),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "sync.diagnostics.run" => {
                let config = self.load_sync_config()?;
                let secrets = self.load_sync_secrets()?;
                let mut diagnostics_config = config.clone();
                diagnostics_config.enabled = true; // Force enabled to run diagnostics
                let token = secrets.token.clone().unwrap_or_default();
                let mut secrets_for_diag = secrets.clone();
                secrets_for_diag.token = Some(token);
                let backend = crate::sync_service::Git2Backend;
                let result = crate::sync_service::SyncService::perform_sync_diagnostics(
                    &diagnostics_config,
                    &secrets_for_diag,
                    &backend,
                );
                match result {
                    Ok(diag) => Ok(ActionResult {
                        success: true,
                        message: Some("Diagnostics completed".to_string()),
                        data: Some(serde_json::to_value(diag).unwrap_or(Value::Null)),
                        proposed_ui: None,
                        requires_confirmation: None,
                    }),
                    Err(e) => Ok(ActionResult {
                        success: false,
                        message: Some(format!("Diagnostics failed: {}", e)),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "sync.plan.preview" => {
                let config = self.load_sync_config()?;
                let plan_result = self.perform_sync_dry_run(&config);
                match plan_result {
                    Ok(plan) => Ok(ActionResult {
                        success: true,
                        message: Some("Plan calculated".to_string()),
                        data: Some(serde_json::to_value(plan).unwrap_or(Value::Null)),
                        proposed_ui: None,
                        requires_confirmation: None,
                    }),
                    Err(e) => Ok(ActionResult {
                        success: false,
                        message: Some(format!("Plan calculation failed: {}", e)),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "settings.editor.line_spacing.get" => {
                let settings = self.load_local_settings()?;
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(serde_json::json!({ "multiplier": settings.editor_line_spacing_multiplier })),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.line_spacing.set" => {
                if let Some(multiplier) = args.get("multiplier").and_then(|v| v.as_f64()) {
                    if multiplier < 1.0 || multiplier > 3.0 {
                        return Ok(ActionResult {
                            success: false,
                            message: Some("Line spacing multiplier must be between 1.0 and 3.0".to_string()),
                            data: None,
                            proposed_ui: None,
                            requires_confirmation: None,
                        });
                    }
                    let mut settings = self.load_local_settings()?;
                    settings.editor_line_spacing_multiplier = multiplier as f32;
                    self.save_local_settings(&settings)?;
                    Ok(ActionResult {
                        success: true,
                        message: Some("Line spacing updated".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                } else {
                    Ok(ActionResult {
                        success: false,
                        message: Some("Missing or invalid multiplier parameter".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "settings.editor.auto_indent.get" => {
                let settings = self.load_local_settings()?;
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(serde_json::json!({ "enabled": settings.auto_indent_enabled, "widthChars": settings.auto_indent_width })),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.auto_indent.set" => {
                if let Some(enabled) = args.get("enabled").and_then(|v| v.as_bool()) {
                    let mut settings = self.load_local_settings()?;
                    settings.auto_indent_enabled = enabled;
                    if let Some(width) = args.get("widthChars").and_then(|v| v.as_f64()) {
                        settings.auto_indent_width = width as f32;
                    }
                    self.save_local_settings(&settings)?;
                    Ok(ActionResult {
                        success: true,
                        message: Some("Auto indent updated".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                } else {
                    Ok(ActionResult {
                        success: false,
                        message: Some("Missing or invalid enabled parameter".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "settings.editor.typing_animation.get" => {
                let settings = self.load_local_settings()?;
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(serde_json::json!({ "enabled": settings.editor_typing_animation_enabled })),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.typing_animation.set" => {
                if let Some(enabled) = args.get("enabled").and_then(|v| v.as_bool()) {
                    let mut settings = self.load_local_settings()?;
                    settings.editor_typing_animation_enabled = enabled;
                    self.save_local_settings(&settings)?;
                    Ok(ActionResult {
                        success: true,
                        message: Some("Typing animation updated".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                } else {
                    Ok(ActionResult {
                        success: false,
                        message: Some("Missing or invalid enabled parameter".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            "settings.editor.smooth_cursor.get" => {
                let settings = self.load_local_settings()?;
                Ok(ActionResult {
                    success: true,
                    message: None,
                    data: Some(serde_json::json!({ "enabled": settings.editor_smooth_cursor_enabled })),
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
            "settings.editor.smooth_cursor.set" => {
                if let Some(enabled) = args.get("enabled").and_then(|v| v.as_bool()) {
                    let mut settings = self.load_local_settings()?;
                    settings.editor_smooth_cursor_enabled = enabled;
                    self.save_local_settings(&settings)?;
                    Ok(ActionResult {
                        success: true,
                        message: Some("Smooth cursor updated".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                } else {
                    Ok(ActionResult {
                        success: false,
                        message: Some("Missing or invalid enabled parameter".to_string()),
                        data: None,
                        proposed_ui: None,
                        requires_confirmation: None,
                    })
                }
            }
            _ => {
                Ok(ActionResult {
                    success: false,
                    message: Some(format!("Action execution not implemented: {}", action_id)),
                    data: None,
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            }
        }
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        project::list_projects(&self.workspace_path)
    }

    /// Create a new project.
    pub fn create_project(&self, title: &str) -> Result<Project> {
        project::create_project(&self.workspace_path, title)
    }

    /// List volumes in a project.
    pub fn list_volumes(&self, project_id: &str) -> Result<Vec<Volume>> {
        volume::list_volumes(&self.workspace_path, project_id)
    }

    /// Create a new volume.
    pub fn create_volume(&self, project_id: &str, title: &str) -> Result<Volume> {
        volume::create_volume(&self.workspace_path, project_id, title)
    }

    /// List chapters in a volume.
    pub fn list_chapters(&self, project_id: &str, volume_id: &str) -> Result<Vec<Chapter>> {
        chapter::list_chapters(&self.workspace_path, project_id, volume_id)
    }

    /// Create a new chapter.
    pub fn create_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        title: &str,
    ) -> Result<Chapter> {
        chapter::create_chapter(&self.workspace_path, project_id, volume_id, title)
    }

    /// Read a specific project's manifest (This requires a volume lookup, simplifying to just reading a chapter).
    pub fn get_project_stats(&self, project_id: &str) -> Result<crate::project::ProjectStats> {
        crate::project::get_project_stats(&self.workspace_path, project_id)
    }

    pub fn get_recent_edits(&self) -> Result<Vec<crate::workspace::RecentEdit>> {
        crate::workspace::get_recent_edits(&self.workspace_path)
    }

    pub fn record_recent_edit(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<()> {
        crate::workspace::record_recent_edit(
            &self.workspace_path,
            project_id,
            volume_id,
            chapter_id,
        )
    }

    pub fn read_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<ChapterContent> {
        chapter::read_chapter(&self.workspace_path, project_id, volume_id, chapter_id)
    }

    /// Write content to a chapter (atomic).
    pub fn write_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        content: &str,
    ) -> Result<()> {
        chapter::save_chapter(
            &self.workspace_path,
            project_id,
            volume_id,
            chapter_id,
            content,
        )
    }

    pub fn update_chapter_note(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        note: &str,
    ) -> Result<()> {
        chapter::update_chapter_note(
            &self.workspace_path,
            project_id,
            volume_id,
            chapter_id,
            note,
        )
    }

    /// Load local settings.
    pub fn load_local_settings(&self) -> Result<LocalSettings> {
        settings::load_local_settings(&self.workspace_path)
    }

    /// Save local settings.
    pub fn save_local_settings(&self, settings: &LocalSettings) -> Result<()> {
        settings::save_local_settings(&self.workspace_path, settings)
    }

    /// Load syncable settings.
    pub fn load_syncable_settings(&self) -> Result<SyncableSettings> {
        settings::load_syncable_settings(&self.workspace_path)
    }

    /// Save syncable settings.
    pub fn save_syncable_settings(&self, settings: &SyncableSettings) -> Result<()> {
        settings::save_syncable_settings(&self.workspace_path, settings)
    }

    /// Backup project
    pub fn backup_project(&self, project_id: &str) -> Result<()> {
        backup::backup_project(&self.workspace_path, project_id)
    }

    /// Move chapter to trash
    pub fn move_chapter_to_trash(&self, chapter_id: &str) -> Result<()> {
        trash::move_chapter_to_trash(&self.workspace_path, chapter_id)
    }

    /// Update index
    pub fn update_index(&self) -> Result<()> {
        index::update_index()
    }

    /// Sync workspace
    pub fn sync_workspace(&self) -> Result<()> {
        sync::sync_workspace()
    }

    // --- Settings Registry ---
    pub fn list_registered_settings(&self) -> crate::settings_registry::SettingsRegistry {
        crate::settings_registry::SettingsRegistry::default_registry()
    }

    // --- AI Service ---
    pub fn build_ai_context(
        &self,
        reference: crate::ai_service::AiContextReference,
    ) -> crate::error::Result<String> {
        let ai = crate::ai_service::AiService::new();
        ai.build_ai_context(reference)
    }

    pub fn get_ai_request_payload(
        &self,
        conversation: &crate::ai_service::AiConversation,
        tools: Option<Vec<crate::ai_service::AiToolDefinition>>,
    ) -> crate::error::Result<serde_json::Value> {
        let ai = crate::ai_service::AiService::new();
        ai.get_ai_request_payload(conversation, tools)
    }

    // --- Graph Service ---
    pub fn load_graph(
        &self,
        project_id: Option<&str>,
    ) -> crate::error::Result<crate::graph_service::GraphDocument> {
        let graph = crate::graph_service::GraphService::new(&self.workspace_path);
        graph.load_graph(project_id)
    }

    pub fn save_graph(
        &self,
        project_id: Option<&str>,
        doc: &crate::graph_service::GraphDocument,
    ) -> crate::error::Result<()> {
        let graph = crate::graph_service::GraphService::new(&self.workspace_path);
        graph.save_graph(project_id, doc)
    }

    // --- Proofreading Service ---
    pub fn proofread_text(
        &self,
        text: &str,
    ) -> crate::error::Result<Vec<crate::proofreading_service::ProofreadingSuggestion>> {
        let pr = crate::proofreading_service::ProofreadingService::new();
        pr.proofread(text)
    }

    // --- Sync Service ---
    pub fn scan_sync_files(&self) -> crate::error::Result<Vec<crate::sync_service::SyncFileEntry>> {
        crate::sync_service::SyncService::scan_workspace_for_sync(&self.workspace_path)
    }

    pub fn build_sync_plan_from_workspace(
        &self,
    ) -> crate::error::Result<crate::sync_service::SyncPlan> {
        crate::sync_service::SyncService::build_sync_plan_from_workspace(&self.workspace_path)
    }

    pub fn load_sync_state(&self) -> crate::error::Result<crate::sync_service::SyncState> {
        crate::sync_service::SyncService::load_sync_state(&self.workspace_path)
    }

    pub fn save_sync_state(
        &self,
        state: &crate::sync_service::SyncState,
    ) -> crate::error::Result<()> {
        crate::sync_service::SyncService::save_sync_state(&self.workspace_path, state)
    }

    pub fn record_sync_conflict(
        &self,
        conflict: crate::sync_service::SyncConflict,
        local_content: Option<&str>,
    ) -> crate::error::Result<()> {
        crate::sync_service::SyncService::record_sync_conflict(
            &self.workspace_path,
            conflict,
            local_content,
        )
    }

    pub fn get_sync_ignored_paths(&self) -> crate::error::Result<Vec<String>> {
        crate::sync_service::SyncService::get_sync_ignored_paths(&self.workspace_path)
    }


    pub fn perform_sync_diagnostics(
        &self,
        config: &crate::sync_service::SyncConfig,
    ) -> crate::error::Result<crate::sync_service::SyncDiagnosticsResult> {
        let secrets = self.load_sync_secrets().unwrap_or_default();
        let backend = crate::sync_service::create_sync_backend(&config.backend_type);
        backend.diagnose(config, &secrets)
    }

    pub fn perform_sync_dry_run(
        &self,
        config: &crate::sync_service::SyncConfig,
    ) -> crate::error::Result<crate::sync_service::SyncPlan> {
        crate::sync_service::SyncService::perform_sync_dry_run(&self.workspace_path, config)
    }

    pub fn perform_sync(
        &self,
        config: &crate::sync_service::SyncConfig,
    ) -> crate::error::Result<crate::sync_service::SyncResult> {
        let secrets = self.load_sync_secrets().unwrap_or_default();
        let backend = crate::sync_service::create_sync_backend(&config.backend_type);
        backend.sync(&self.workspace_path, config, &secrets)
    }

    pub fn load_sync_secrets(&self) -> crate::error::Result<crate::sync_service::SyncSecrets> {
        let secrets_path = self
            .workspace_path
            .join("app-meta/sync/sync_secrets.local.json");
        if !secrets_path.exists() {
            return Ok(crate::sync_service::SyncSecrets::default());
        }
        let content = std::fs::read_to_string(&secrets_path)?;
        let secrets: crate::sync_service::SyncSecrets =
            serde_json::from_str(&content).map_err(|e| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        Ok(secrets)
    }

    pub fn save_sync_secrets(
        &self,
        secrets: &crate::sync_service::SyncSecrets,
    ) -> crate::error::Result<()> {
        let secrets_path = self
            .workspace_path
            .join("app-meta/sync/sync_secrets.local.json");
        if let Some(parent) = secrets_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(secrets).map_err(|e| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        let tmp_path = secrets_path.with_extension("tmp");

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true).mode(0o600);
            let mut file = options.open(&tmp_path)?;
            file.write_all(content.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&tmp_path, content)?;
        }

        std::fs::rename(tmp_path, secrets_path)?;
        Ok(())
    }

    pub fn load_sync_config(&self) -> crate::error::Result<crate::sync_service::SyncConfig> {
        let config_path = self.workspace_path.join("app-meta/sync/sync_config.json");
        if !config_path.exists() {
            return Ok(crate::sync_service::SyncConfig {
                enabled: false,
                backend_type: crate::sync_service::BackendType::Git,
                remote_url: String::new(),
                transport: crate::sync_service::SyncTransport::HttpsToken,
                branch: "main".to_string(),
                auto_sync: false,
                sync_interval_seconds: 300,
                proxy_enabled: false,
                proxy_type: "auto".to_string(),
                proxy_host: "127.0.0.1".to_string(),
                proxy_port: 7890,
                username: String::new(),
                android_has_internet_permission: true,
                android_has_access_network_state_permission: true,
            });
        }
        let content = std::fs::read_to_string(&config_path)?;
        let config: crate::sync_service::SyncConfig =
            serde_json::from_str(&content).map_err(|e| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        Ok(config)
    }

    pub fn save_sync_config(
        &self,
        config: &crate::sync_service::SyncConfig,
    ) -> crate::error::Result<()> {
        let config_path = self.workspace_path.join("app-meta/sync/sync_config.json");
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(config).map_err(|e| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        let tmp_path = config_path.with_extension("tmp");
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(tmp_path, config_path)?;
        Ok(())
    }

    pub fn validate_sync_config(
        &self,
        config: &crate::sync_service::SyncConfig,
    ) -> crate::error::Result<bool> {
        if config.enabled && config.remote_url.is_empty() {
            return Ok(false);
        }
        // token check is now separated since token is in secrets.
        // We will just validate the remote_url for config
        Ok(true)
    }

    pub fn rename_project(&self, project_id: &str, new_title: &str) -> crate::error::Result<()> {
        crate::project::rename_project(&self.workspace_path, project_id, new_title)
    }

    pub fn delete_project(&self, project_id: &str) -> crate::error::Result<()> {
        crate::project::delete_project(&self.workspace_path, project_id)
    }

    pub fn reorder_projects(&self, ordered_ids: &[String]) -> crate::error::Result<()> {
        crate::project::reorder_projects(&self.workspace_path, ordered_ids)
    }

    pub fn rename_volume(
        &self,
        project_id: &str,
        volume_id: &str,
        new_title: &str,
    ) -> crate::error::Result<()> {
        crate::volume::rename_volume(&self.workspace_path, project_id, volume_id, new_title)
    }

    pub fn delete_volume(&self, project_id: &str, volume_id: &str) -> crate::error::Result<()> {
        crate::volume::delete_volume(&self.workspace_path, project_id, volume_id)
    }

    pub fn reorder_volumes(
        &self,
        project_id: &str,
        ordered_ids: &[String],
    ) -> crate::error::Result<()> {
        crate::volume::reorder_volumes(&self.workspace_path, project_id, ordered_ids)
    }

    pub fn rename_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        new_title: &str,
    ) -> crate::error::Result<()> {
        crate::chapter::rename_chapter(
            &self.workspace_path,
            project_id,
            volume_id,
            chapter_id,
            new_title,
        )
    }

    pub fn delete_chapter(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> crate::error::Result<()> {
        crate::chapter::delete_chapter(&self.workspace_path, project_id, volume_id, chapter_id)
    }

    pub fn reorder_chapters(
        &self,
        project_id: &str,
        volume_id: &str,
        ordered_ids: &[String],
    ) -> crate::error::Result<()> {
        crate::chapter::reorder_chapters(&self.workspace_path, project_id, volume_id, ordered_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_facade_basic_flow() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());

        // Create workspace
        assert!(core.create_workspace().is_ok());
        assert!(core.validate_workspace().unwrap());

        // Create project
        let project = core.create_project("My Project").unwrap();
        let projects = core.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, project.id);

        // Create volume
        let volume = core.create_volume(&project.id, "Vol 1").unwrap();
        let volumes = core.list_volumes(&project.id).unwrap();
        // Since we don't know which order they will be returned in (it reads from directory),
        // we sort them by title or id or just check if the new volume is present.
        assert_eq!(volumes.len(), 2);
        assert!(volumes.iter().any(|v| v.id == volume.id));

        // Create chapter
        let chapter = core
            .create_chapter(&project.id, &volume.id, "Ch 1")
            .unwrap();
        let chapters = core.list_chapters(&project.id, &volume.id).unwrap();
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].id, chapter.id);

        // Write and read chapter
        core.write_chapter(&project.id, &volume.id, &chapter.id, "Content here")
            .unwrap();
        let content = core
            .read_chapter(&project.id, &volume.id, &chapter.id)
            .unwrap();
        assert_eq!(content.content, "Content here");
    }

    #[test]
    fn test_facade_settings_flow() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let mut local_settings = core.load_local_settings().unwrap();
        local_settings.window_width = 1000.0;
        local_settings.theme_mode = Some("dark".to_string());
        local_settings.auto_save_enabled = true;
        local_settings.editor_font_size = 14.0;
        core.save_local_settings(&local_settings).unwrap();

        let loaded_local = core.load_local_settings().unwrap();
        assert_eq!(loaded_local.window_width, 1000.0);
        assert_eq!(loaded_local.theme_mode.unwrap(), "dark");
        assert!(loaded_local.auto_save_enabled);
        assert_eq!(loaded_local.editor_font_size, 14.0);

        let mut syncable_settings = core.load_syncable_settings().unwrap();
        syncable_settings.font_size = 18.0;
        syncable_settings.theme_mode = "system".to_string();
        core.save_syncable_settings(&syncable_settings).unwrap();

        let loaded_syncable = core.load_syncable_settings().unwrap();
        assert_eq!(loaded_syncable.font_size, 18.0);
        assert_eq!(loaded_syncable.theme_mode, "system");

        let get_result = core.execute_action("settings.editor.font_size.get", "", "").unwrap();
        assert!(get_result.success);
        let data = get_result.data.unwrap();
        assert_eq!(data.get("fontSize").unwrap().as_f64().unwrap(), 18.0);
        assert_eq!(data.get("source").unwrap().as_str().unwrap(), "syncable");
    }

    #[test]
    fn test_facade_not_implemented() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());

        assert!(core.backup_project("p1").is_err());
        assert!(core.move_chapter_to_trash("c1").is_err());
        assert!(core.update_index().is_err());
        assert!(core.sync_workspace().is_err());
    }

    #[test]
    fn test_facade_sync_config_flow() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        // Load non-existent should give default
        let config = core.load_sync_config().unwrap();
        assert!(!config.enabled);

        // Save new config
        let mut new_config = config.clone();
        new_config.enabled = true;
        new_config.remote_url = "https://example.com/repo.git".to_string();
        core.save_sync_config(&new_config).unwrap();

        let loaded = core.load_sync_config().unwrap();
        assert!(loaded.enabled);
        assert_eq!(loaded.remote_url, "https://example.com/repo.git");

        let mut secrets = core.load_sync_secrets().unwrap();
        secrets.token = Some("my_super_secret_token".to_string());
        core.save_sync_secrets(&secrets).unwrap();

        let loaded_secrets = core.load_sync_secrets().unwrap();
        assert_eq!(
            loaded_secrets.token.as_ref().unwrap(),
            "my_super_secret_token"
        );

        assert!(core.validate_sync_config(&loaded).unwrap());

        let mut bad_config = loaded.clone();
        bad_config.remote_url = "".to_string();
        assert!(!core.validate_sync_config(&bad_config).unwrap());
    }

    #[test]
    fn test_facade_perform_sync_dry_run() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let config = core.load_sync_config().unwrap();
        let plan = core.perform_sync_dry_run(&config).unwrap();
        // Since config is disabled by default, plan should be empty
        assert!(plan.files_to_upload.is_empty());
    }
}
