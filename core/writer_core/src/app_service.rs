use std::path::Path;
use std::sync::Arc;
use crate::facade::WriterCore;
use crate::error::Error;

#[derive(Debug, Clone)]
pub struct ProjectDto {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<crate::project::Project> for ProjectDto {
    fn from(p: crate::project::Project) -> Self {
        Self {
            id: p.id,
            title: p.title,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VolumeDto {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub order: i32,
}

impl From<crate::volume::Volume> for VolumeDto {
    fn from(v: crate::volume::Volume) -> Self {
        Self {
            id: v.id,
            title: v.title,
            created_at: v.created_at,
            updated_at: v.updated_at,
            order: v.order,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChapterMetaDto {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub order: i32,
    pub word_count: u32,
    pub hash: String,
    pub note: Option<String>,
}

impl From<crate::chapter::Chapter> for ChapterMetaDto {
    fn from(c: crate::chapter::Chapter) -> Self {
        Self {
            id: c.id,
            title: c.title,
            created_at: c.created_at,
            updated_at: c.updated_at,
            order: c.order,
            word_count: c.word_count,
            hash: c.hash,
            note: c.note,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChapterContentDto {
    pub meta: ChapterMetaDto,
    pub content: String,
}

impl From<crate::chapter::ChapterContent> for ChapterContentDto {
    fn from(c: crate::chapter::ChapterContent) -> Self {
        Self {
            meta: c.meta.into(),
            content: c.content,
        }
    }
}

use crate::facade::ChapterOpenResult;

impl From<ChapterOpenResult> for ChapterContentDto {
    fn from(c: ChapterOpenResult) -> Self {
        Self {
            meta: c.meta.into(),
            content: c.content,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChapterSaveReceiptDto {
    pub chapter_relative_path: String,
    pub content_len: u32,
    pub content_hash: String,
    pub meta_hash: String,
    pub updated_at: String,
    pub word_count: u32,
}

impl From<crate::chapter::ChapterSaveReceipt> for ChapterSaveReceiptDto {
    fn from(r: crate::chapter::ChapterSaveReceipt) -> Self {
        Self {
            chapter_relative_path: r.chapter_relative_path,
            content_len: r.content_len as u32,
            content_hash: r.content_hash,
            meta_hash: r.meta_hash,
            updated_at: r.updated_at,
            word_count: r.word_count,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecentEditDto {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub timestamp: String,
}

impl From<crate::workspace::RecentEdit> for RecentEditDto {
    fn from(r: crate::workspace::RecentEdit) -> Self {
        Self {
            project_id: r.project_id,
            volume_id: r.volume_id,
            chapter_id: r.chapter_id,
            timestamp: r.timestamp,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalSettingsDto {
    pub theme_mode: Option<String>,
    pub locale: Option<String>,
    pub auto_save_enabled: bool,
    pub editor_font_size: f32,
    pub window_width: f32,
    pub window_height: f32,
    pub auto_save_delay_ms: u64,
    pub auto_indent_enabled: bool,
    pub auto_indent_width: f32,
    pub editor_typing_animation_enabled: bool,
    pub editor_smooth_cursor_enabled: bool,
    pub editor_typing_animation_duration_ms: u64,
    pub editor_smooth_cursor_duration_ms: u64,
    pub ai_enabled: bool,
    pub stats_device_id: Option<String>,
}

impl From<crate::settings::LocalSettings> for LocalSettingsDto {
    fn from(s: crate::settings::LocalSettings) -> Self {
        Self {
            theme_mode: s.theme_mode,
            locale: s.locale,
            auto_save_enabled: s.auto_save_enabled,
            editor_font_size: s.editor_font_size,
            window_width: s.window_width as f32,
            window_height: s.window_height as f32,
            auto_save_delay_ms: s.auto_save_delay_ms,
            auto_indent_enabled: s.auto_indent_enabled,
            auto_indent_width: s.auto_indent_width,
            editor_typing_animation_enabled: s.editor_typing_animation_enabled,
            editor_smooth_cursor_enabled: s.editor_smooth_cursor_enabled,
            editor_typing_animation_duration_ms: s.editor_typing_animation_duration_ms,
            editor_smooth_cursor_duration_ms: s.editor_smooth_cursor_duration_ms,
            ai_enabled: s.ai_enabled,
            stats_device_id: s.stats_device_id,
        }
    }
}

impl Into<crate::settings::LocalSettings> for LocalSettingsDto {
    fn into(self) -> crate::settings::LocalSettings {
        crate::settings::LocalSettings {
            theme_mode: self.theme_mode,
            locale: self.locale,
            auto_save_enabled: self.auto_save_enabled,
            editor_font_size: self.editor_font_size,
            window_width: self.window_width as f64,
            window_height: self.window_height as f64,
            auto_save_delay_ms: self.auto_save_delay_ms,
            auto_indent_enabled: self.auto_indent_enabled,
            auto_indent_width: self.auto_indent_width,
            editor_typing_animation_enabled: self.editor_typing_animation_enabled,
            editor_smooth_cursor_enabled: self.editor_smooth_cursor_enabled,
            editor_typing_animation_duration_ms: self.editor_typing_animation_duration_ms,
            editor_smooth_cursor_duration_ms: self.editor_smooth_cursor_duration_ms,
            ai_enabled: self.ai_enabled,
            stats_device_id: self.stats_device_id,
            editor_line_spacing_multiplier: 1.5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncableSettingsDto {
    pub font_size: f64,
    pub theme_mode: String,
    pub monet_color: String,
}

impl From<crate::settings::SyncableSettings> for SyncableSettingsDto {
    fn from(s: crate::settings::SyncableSettings) -> Self {
        Self {
            font_size: s.font_size,
            theme_mode: s.theme_mode,
            monet_color: s.monet_color,
        }
    }
}

impl Into<crate::settings::SyncableSettings> for SyncableSettingsDto {
    fn into(self) -> crate::settings::SyncableSettings {
        crate::settings::SyncableSettings {
            font_size: self.font_size,
            theme_mode: self.theme_mode,
            monet_color: self.monet_color,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncConfigDto {
    pub enabled: bool,
    pub backend_type: String,
    pub remote_url: String,
    pub transport: String,
    pub branch: String,
    pub auto_sync: bool,
    pub sync_interval_seconds: u32,
    pub proxy_enabled: bool,
    pub proxy_type: String,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub username: String,
}

impl From<crate::sync_service::SyncConfig> for SyncConfigDto {
    fn from(c: crate::sync_service::SyncConfig) -> Self {
        Self {
            enabled: c.enabled,
            backend_type: match c.backend_type {
                crate::sync_service::BackendType::Git => "git".to_string(),
                crate::sync_service::BackendType::GithubApi => "github_api".to_string(),
                crate::sync_service::BackendType::WebDav => "webdav".to_string(),
                crate::sync_service::BackendType::S3 => "s3".to_string(),
                crate::sync_service::BackendType::LocalFolder => "local_folder".to_string(),
            },
            remote_url: c.remote_url,
            transport: match c.transport {
                crate::sync_service::SyncTransport::HttpsToken => "https_token".to_string(),
                crate::sync_service::SyncTransport::SshDeployKey => "ssh".to_string(),
            },
            branch: c.branch,
            auto_sync: c.auto_sync,
            sync_interval_seconds: c.sync_interval_seconds,
            proxy_enabled: c.proxy_enabled,
            proxy_type: "auto".to_string(),
            proxy_host: c.proxy_host,
            proxy_port: c.proxy_port,
            username: c.username,
        }
    }
}

impl Into<crate::sync_service::SyncConfig> for SyncConfigDto {
    fn into(self) -> crate::sync_service::SyncConfig {
        crate::sync_service::SyncConfig {
            enabled: self.enabled,
            backend_type: match self.backend_type.as_str() {
                "git" => crate::sync_service::BackendType::Git,
                "github_api" => crate::sync_service::BackendType::GithubApi,
                "webdav" => crate::sync_service::BackendType::WebDav,
                "s3" => crate::sync_service::BackendType::S3,
                _ => crate::sync_service::BackendType::GithubApi,
            },
            remote_url: self.remote_url,
            transport: match self.transport.as_str() {
                "https_token" => crate::sync_service::SyncTransport::HttpsToken,
                "ssh" => crate::sync_service::SyncTransport::SshDeployKey,
                _ => crate::sync_service::SyncTransport::HttpsToken,
            },
            branch: self.branch,
            auto_sync: self.auto_sync,
            sync_interval_seconds: self.sync_interval_seconds,
            proxy_enabled: self.proxy_enabled,
            proxy_type: Default::default(),
            proxy_host: self.proxy_host,
            proxy_port: self.proxy_port,
            username: self.username,
            android_has_access_network_state_permission: false,
            android_has_internet_permission: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncSecretsDto {
    pub token: Option<String>,
}

impl From<crate::sync_service::SyncSecrets> for SyncSecretsDto {
    fn from(s: crate::sync_service::SyncSecrets) -> Self {
        Self {
            token: s.token,
        }
    }
}

impl Into<crate::sync_service::SyncSecrets> for SyncSecretsDto {
    fn into(self) -> crate::sync_service::SyncSecrets {
        crate::sync_service::SyncSecrets {
            token: self.token,
            ssh_private_key: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncStateDto {
    pub status: String,
    pub backend_type: Option<String>,
    pub transport: Option<String>,
    pub last_synced_commit: Option<String>,
    pub last_sync_time: Option<i64>,
    pub last_error: Option<String>,
    pub last_successful_network_mode: Option<String>,
    pub conflicts: Option<Vec<SyncConflictDto>>,
}

#[derive(Debug, Clone)]
pub struct SyncConflictDto {
    pub local_path: String,
    pub remote_path: String,
    pub local_hash: String,
    pub remote_hash: String,
    pub base_hash: String,
    pub created_at: i64,
    pub description: String,
}

impl From<crate::sync_service::SyncConflict> for SyncConflictDto {
    fn from(c: crate::sync_service::SyncConflict) -> Self {
        Self {
            local_path: c.local_path,
            remote_path: c.remote_path,
            local_hash: c.local_hash,
            remote_hash: c.remote_hash,
            base_hash: c.base_hash,
            created_at: c.created_at,
            description: c.description,
        }
    }
}

impl From<crate::sync_service::SyncState> for SyncStateDto {
    fn from(s: crate::sync_service::SyncState) -> Self {
        Self {
            status: "idle".to_string(), // Simplified, enum serialization handled in Kotlin wrapper later
            backend_type: None,
            transport: None,
            last_synced_commit: s.last_synced_commit,
            last_sync_time: s.last_sync_time,
            last_error: s.last_error,
            last_successful_network_mode: s.last_successful_network_mode,
            conflicts: Some(s.conflicts.into_iter().map(Into::into).collect()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetworkProbeResultDto {
    pub mode: String,
    pub success: bool,
    pub status: String,
    pub message: String,
    pub raw_error: Option<String>,
}

impl From<crate::sync_service::NetworkProbeResult> for NetworkProbeResultDto {
    fn from(p: crate::sync_service::NetworkProbeResult) -> Self {
        Self {
            mode: p.mode,
            success: p.success,
            status: p.status,
            message: p.message,
            raw_error: p.raw_error,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncDiagnosticsResultDto {
    pub success: bool,
    pub backend_type: String,
    pub android_has_internet_permission: bool,
    pub android_has_access_network_state_permission: bool,
    pub android_network_state: String,
    pub tcp_probe_ok: bool,
    pub tcp_probe_status: String,
    pub http_connect_probe_ok: bool,
    pub http_connect_probe_status: String,
    pub libgit2_probe_ok: bool,
    pub libgit2_probe_status: String,
    pub network_ok: bool,
    pub auth_ok: bool,
    pub repo_ok: bool,
    pub branch_ok: bool,
    pub network_status: String,
    pub auth_status: String,
    pub repo_status: String,
    pub branch_status: String,
    pub remote_url_sanitized: String,
    pub transport: String,
    pub error_category: String,
    pub user_message: String,
    pub raw_error: Option<String>,
    pub chosen_network_mode: Option<String>,
    pub network_probe_summary: Option<Vec<NetworkProbeResultDto>>,
}

impl From<crate::sync_service::SyncDiagnosticsResult> for SyncDiagnosticsResultDto {
    fn from(d: crate::sync_service::SyncDiagnosticsResult) -> Self {
        Self {
            success: d.success,
            backend_type: "git".to_string(), // Simplified
            android_has_internet_permission: d.android_has_internet_permission,
            android_has_access_network_state_permission: d.android_has_access_network_state_permission,
            android_network_state: d.android_network_state,
            tcp_probe_ok: false,
            tcp_probe_status: "".to_string(),
            http_connect_probe_ok: false,
            http_connect_probe_status: "".to_string(),
            libgit2_probe_ok: false,
            libgit2_probe_status: "".to_string(),
            network_ok: d.network_ok,
            auth_ok: d.auth_ok,
            repo_ok: d.repo_ok,
            branch_ok: d.branch_ok,
            network_status: d.network_status,
            auth_status: d.auth_status,
            repo_status: d.repo_status,
            branch_status: d.branch_status,
            remote_url_sanitized: d.remote_url_sanitized,
            transport: "https_token".to_string(), // Simplified
            error_category: "unknown".to_string(),
            user_message: d.user_message,
            raw_error: d.raw_error,
            chosen_network_mode: d.chosen_network_mode,
            network_probe_summary: Some(d.network_probe_summary.into_iter().map(Into::into).collect()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncPlanDto {
    pub files_to_upload: Vec<String>,
    pub files_to_download: Vec<String>,
    pub files_to_delete_local: Vec<String>,
    pub files_to_delete_remote: Vec<String>,
    pub ignored_files: Vec<String>,
    pub conflicts: Vec<String>,
}

impl From<crate::sync_service::SyncPlan> for SyncPlanDto {
    fn from(p: crate::sync_service::SyncPlan) -> Self {
        Self {
            files_to_upload: p.files_to_upload,
            files_to_download: p.files_to_download,
            files_to_delete_local: p.files_to_delete_local,
            files_to_delete_remote: p.files_to_delete_remote,
            ignored_files: p.ignored_files,
            conflicts: p.conflicts,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncResultDto {
    pub status: String,
    pub uploaded_files: Vec<String>,
    pub downloaded_files: Vec<String>,
    pub local_deletes: Vec<String>,
    pub remote_deletes: Vec<String>,
    pub overwritten_files: Vec<String>,
    pub ignored_files: Vec<String>,
    pub conflicts: Vec<SyncConflictDto>,
    pub commit_hash: Option<String>,
    pub error: Option<String>,
    pub first_sync_mode: String,
    pub user_message: Option<String>,
    pub chosen_network_mode: Option<String>,
    pub network_probe_summary: Option<Vec<NetworkProbeResultDto>>,
}

impl From<crate::sync_service::SyncResult> for SyncResultDto {
    fn from(r: crate::sync_service::SyncResult) -> Self {
        Self {
            status: "idle".to_string(),
            uploaded_files: r.uploaded_files,
            downloaded_files: r.downloaded_files,
            local_deletes: r.local_deletes,
            remote_deletes: r.remote_deletes,
            overwritten_files: r.overwritten_files,
            ignored_files: r.ignored_files,
            conflicts: r.conflicts.into_iter().map(Into::into).collect(),
            commit_hash: r.commit_hash,
            error: r.error,
            first_sync_mode: "none".to_string(),
            user_message: r.user_message,
            chosen_network_mode: r.chosen_network_mode,
            network_probe_summary: Some(r.network_probe_summary.into_iter().map(Into::into).collect()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WriterError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("JSON parsing error: {0}")]
    Json(String),
    #[error("Workspace not found or invalid")]
    InvalidWorkspace,
    #[error("Project not found")]
    ProjectNotFound,
    #[error("Volume not found")]
    VolumeNotFound,
    #[error("Chapter not found")]
    ChapterNotFound,
    #[error("blocked_empty_overwrite: chapter_id={chapter_id}, old_len={old_len}, new_len={new_len}, reason={reason}")]
    EmptyOverwriteBlocked {
        chapter_id: String,
        old_len: u32,
        new_len: u32,
        reason: String,
    },
    #[error("Not implemented")]
    NotImplemented,
    #[error("Refuse to delete workspace root")]
    RefuseToDeleteWorkspaceRoot,
    #[error("Invalid delete target: {0}")]
    InvalidDeleteTarget(String),
    #[error("Other error: {0}")]
    Other(String),
}

impl From<crate::error::Error> for WriterError {
    fn from(e: crate::error::Error) -> Self {
        match e {
            Error::Io(e) => WriterError::Io(e.to_string()),
            Error::Json(e) => WriterError::Json(e.to_string()),
            Error::InvalidWorkspace => WriterError::InvalidWorkspace,
            Error::ProjectNotFound => WriterError::ProjectNotFound,
            Error::VolumeNotFound => WriterError::VolumeNotFound,
            Error::ChapterNotFound => WriterError::ChapterNotFound,
            Error::EmptyOverwriteBlocked { chapter_id, old_len, new_len, reason } => WriterError::EmptyOverwriteBlocked {
                chapter_id,
                old_len: old_len as u32,
                new_len: new_len as u32,
                reason,
            },
            Error::NotImplemented => WriterError::NotImplemented,
            Error::RefuseToDeleteWorkspaceRoot => WriterError::RefuseToDeleteWorkspaceRoot,
            Error::InvalidDeleteTarget(s) => WriterError::InvalidDeleteTarget(s),
            Error::Other(s) => WriterError::Other(s),
        }
    }
}


pub struct WriterAppService {
    workspace_path: String,
}


impl WriterAppService {

    pub fn new(workspace_path: String) -> Self {
        Self { workspace_path }
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectDto>, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.list_projects().map(|v| v.into_iter().map(Into::into).collect()).map_err(Into::into)
    }

    pub fn create_workspace_if_needed(&self) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.create_workspace().map(|_| true).map_err(Into::into)
    }

    pub fn validate_workspace(&self) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.validate_workspace().map_err(Into::into)
    }

    pub fn get_recent_edits(&self) -> Result<Vec<RecentEditDto>, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.get_recent_edits().map(|v| v.into_iter().map(Into::into).collect()).map_err(Into::into)
    }

    pub fn record_recent_edit(&self, project_id: String, volume_id: String, chapter_id: String) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.record_recent_edit(&project_id, &volume_id, &chapter_id).map(|_| true).map_err(Into::into)
    }

    pub fn create_project(&self, title: String) -> Result<ProjectDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.create_project(&title).map(Into::into).map_err(Into::into)
    }

    pub fn rename_project(&self, project_id: String, new_title: String) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.rename_project(&project_id, &new_title).map(|_| true).map_err(Into::into)
    }

    pub fn delete_project(&self, project_id: String) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.delete_project(&project_id).map(|_| true).map_err(Into::into)
    }

    pub fn reorder_projects(&self, ordered_project_ids: Vec<String>) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.reorder_projects(&ordered_project_ids).map(|_| true).map_err(Into::into)
    }

    pub fn list_volumes(&self, project_id: String) -> Result<Vec<VolumeDto>, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.list_volumes(&project_id).map(|v| v.into_iter().map(Into::into).collect()).map_err(Into::into)
    }

    pub fn create_volume(&self, project_id: String, title: String) -> Result<VolumeDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.create_volume(&project_id, &title).map(Into::into).map_err(Into::into)
    }

    pub fn rename_volume(&self, project_id: String, volume_id: String, new_title: String) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.rename_volume(&project_id, &volume_id, &new_title).map(|_| true).map_err(Into::into)
    }

    pub fn delete_volume(&self, project_id: String, volume_id: String) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.delete_volume(&project_id, &volume_id).map(|_| true).map_err(Into::into)
    }

    pub fn reorder_volumes(&self, project_id: String, ordered_volume_ids: Vec<String>) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.reorder_volumes(&project_id, &ordered_volume_ids).map(|_| true).map_err(Into::into)
    }

    pub fn list_chapters(&self, project_id: String, volume_id: String) -> Result<Vec<ChapterMetaDto>, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.list_chapters(&project_id, &volume_id).map(|v| v.into_iter().map(Into::into).collect()).map_err(Into::into)
    }

    pub fn create_chapter(&self, project_id: String, volume_id: String, title: String) -> Result<ChapterMetaDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.create_chapter(&project_id, &volume_id, &title).map(Into::into).map_err(Into::into)
    }

    pub fn rename_chapter(&self, project_id: String, volume_id: String, chapter_id: String, new_title: String) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.rename_chapter(&project_id, &volume_id, &chapter_id, &new_title).map(|_| true).map_err(Into::into)
    }

    pub fn delete_chapter(&self, project_id: String, volume_id: String, chapter_id: String) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.delete_chapter(&project_id, &volume_id, &chapter_id).map(|_| true).map_err(Into::into)
    }

    pub fn reorder_chapters(&self, project_id: String, volume_id: String, ordered_chapter_ids: Vec<String>) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.reorder_chapters(&project_id, &volume_id, &ordered_chapter_ids).map(|_| true).map_err(Into::into)
    }

    pub fn open_chapter(&self, project_id: String, volume_id: String, chapter_id: String) -> Result<ChapterContentDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.open_chapter(&project_id, &volume_id, &chapter_id).map(Into::into).map_err(Into::into)
    }

    pub fn save_chapter_content(&self, project_id: String, volume_id: String, chapter_id: String, content: String) -> Result<ChapterSaveReceiptDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.write_chapter_verified(&project_id, &volume_id, &chapter_id, &content).map(Into::into).map_err(Into::into)
    }

    pub fn clear_chapter_content(&self, project_id: String, volume_id: String, chapter_id: String) -> Result<ChapterSaveReceiptDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.clear_chapter_content_verified(&project_id, &volume_id, &chapter_id).map(Into::into).map_err(Into::into)
    }

    pub fn update_chapter_note(&self, project_id: String, volume_id: String, chapter_id: String, note: String) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.update_chapter_note(&project_id, &volume_id, &chapter_id, &note).map(|_| true).map_err(Into::into)
    }

    pub fn load_local_settings(&self) -> Result<LocalSettingsDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.load_local_settings().map(Into::into).map_err(Into::into)
    }

    pub fn save_local_settings(&self, settings: LocalSettingsDto) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.save_local_settings(&settings.into()).map(|_| true).map_err(Into::into)
    }

    pub fn load_syncable_settings(&self) -> Result<SyncableSettingsDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.load_syncable_settings().map(Into::into).map_err(Into::into)
    }

    pub fn save_syncable_settings(&self, settings: SyncableSettingsDto) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.save_syncable_settings(&settings.into()).map(|_| true).map_err(Into::into)
    }

    pub fn load_sync_config(&self) -> Result<SyncConfigDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.load_sync_config().map(Into::into).map_err(Into::into)
    }

    pub fn save_sync_config(&self, config: SyncConfigDto) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.save_sync_config(&config.into()).map(|_| true).map_err(Into::into)
    }

    pub fn load_sync_secrets(&self) -> Result<SyncSecretsDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.load_sync_secrets().map(Into::into).map_err(Into::into)
    }

    pub fn save_sync_secrets(&self, secrets: SyncSecretsDto) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.save_sync_secrets(&secrets.into()).map(|_| true).map_err(Into::into)
    }

    pub fn load_sync_state(&self) -> Result<SyncStateDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.load_sync_state().map(Into::into).map_err(Into::into)
    }

    pub fn perform_sync_diagnostics(&self, config: SyncConfigDto) -> Result<SyncDiagnosticsResultDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.perform_sync_diagnostics(&config.into()).map(Into::into).map_err(Into::into)
    }

    pub fn perform_sync_dry_run(&self, config: SyncConfigDto) -> Result<SyncPlanDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.perform_sync_dry_run(&config.into()).map(Into::into).map_err(Into::into)
    }

    pub fn perform_sync(&self, config: SyncConfigDto) -> Result<SyncResultDto, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.perform_sync(&config.into()).map(Into::into).map_err(Into::into)
    }

    // Statistics
    pub fn get_writing_stats_summary(&self, start_date: String, end_date: String) -> Result<String, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        let val = core.get_writing_stats_summary(&start_date, &end_date).map_err(WriterError::from)?;
        Ok(serde_json::to_string(&val).unwrap_or_default())
    }

    pub fn get_writing_stats_by_project(&self, start_date: String, end_date: String) -> Result<String, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        let val = core.get_writing_stats_by_project(&start_date, &end_date).map_err(WriterError::from)?;
        Ok(serde_json::to_string(&val).unwrap_or_default())
    }

    pub fn get_writing_stats_by_chapter(&self, start_date: String, end_date: String) -> Result<String, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        let val = core.get_writing_stats_by_chapter(&start_date, &end_date).map_err(WriterError::from)?;
        Ok(serde_json::to_string(&val).unwrap_or_default())
    }

    pub fn get_writing_stats_by_device(&self, start_date: String, end_date: String) -> Result<String, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        let val = core.get_writing_stats_by_device(&start_date, &end_date).map_err(WriterError::from)?;
        Ok(serde_json::to_string(&val).unwrap_or_default())
    }

    pub fn get_writing_speed_curve(&self, start_date: String, end_date: String, bucket_minutes: u32) -> Result<String, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        let val = core.get_writing_speed_curve(&start_date, &end_date, bucket_minutes).map_err(WriterError::from)?;
        Ok(serde_json::to_string(&val).unwrap_or_default())
    }

    pub fn calculate_word_count(&self, text: String) -> u32 {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.calculate_word_count(&text) as u32
    }

    pub fn process_writing_event(&self, device_id: String, platform: String, project_id: String, volume_id: String, chapter_id: String, old_text: String, new_text: String, session_id: String) -> bool {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.process_writing_event(&device_id, &platform, &project_id, &volume_id, &chapter_id, &old_text, &new_text, &session_id).is_ok()
    }

    pub fn record_writing_event(&self, device_id: String, project_id: String, volume_id: String, chapter_id: String, source: String, inserted_chars: i32, deleted_chars: i32, pasted_chars: i32, ai_inserted_chars: i32, session_id: String) -> bool {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.record_writing_event(&device_id, "android", &project_id, &volume_id, &chapter_id, &source, inserted_chars as u32, deleted_chars as u32, pasted_chars as u32, ai_inserted_chars as u32, &session_id).is_ok()
    }

    // MindMap & StarMap (using JSON mapping to satisfy UI backwards compat while fully migrating NativeCoreBridge to UniFFI wrapper without breaking UI)
    pub fn get_mindmap_snapshot_json(&self, project_id: String) -> Result<String, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        let val = core.get_mind_map_snapshot(&project_id).map_err(WriterError::from)?;
        Ok(serde_json::to_string(&val).unwrap_or_default())
    }

    pub fn save_mindmap_graph_json(&self, _project_id: String, graph_json: String) -> Result<bool, WriterError> {
        let _core = WriterCore::new(Path::new(&self.workspace_path));
        let _val: serde_json::Value = serde_json::from_str(&graph_json).map_err(|e| WriterError::Json(e.to_string()))?;
        Ok(false)
    }

    pub fn list_starmaps(&self) -> Result<String, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        let val = core.list_starmaps().map_err(WriterError::from)?;
        Ok(serde_json::to_string(&val).unwrap_or_default())
    }

    pub fn create_starmap(&self, title: String, desc: String) -> Result<String, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        let val = core.create_starmap(&title, &desc, None).map_err(WriterError::from)?;
        Ok(serde_json::to_string(&val).unwrap_or_default())
    }

    pub fn get_starmap_graph(&self, starmap_id: String) -> Result<String, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        let val = core.get_starmap_graph(&starmap_id).map_err(WriterError::from)?;
        Ok(serde_json::to_string(&val).unwrap_or_default())
    }

    pub fn add_starmap_node(&self, starmap_id: String, node_json: String) -> Result<String, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        let res = core.execute_action("starmap.node.add", &starmap_id, &node_json).map_err(WriterError::from)?;
        Ok(serde_json::to_string(&res).unwrap_or_default())
    }

    pub fn save_starmap_layout(&self, starmap_id: String, layout_json: String) -> Result<bool, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.execute_action("starmap.layout.save", &starmap_id, &layout_json).map_err(WriterError::from)?;
        Ok(true)
    }
}
