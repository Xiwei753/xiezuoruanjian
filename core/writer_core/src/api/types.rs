use crate::facade::ChapterOpenResult;

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
pub struct ProjectStatsDto {
    pub total_word_count: u32,
    pub volume_count: u32,
    pub chapter_count: u32,
}

impl From<crate::project::ProjectStats> for ProjectStatsDto {
    fn from(s: crate::project::ProjectStats) -> Self {
        Self {
            total_word_count: s.total_word_count,
            volume_count: s.volume_count,
            chapter_count: s.chapter_count,
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

impl From<LocalSettingsDto> for crate::settings::LocalSettings {
    fn from(s: LocalSettingsDto) -> Self {
        crate::settings::LocalSettings {
            theme_mode: s.theme_mode,
            locale: s.locale,
            auto_save_enabled: s.auto_save_enabled,
            editor_font_size: s.editor_font_size,
            window_width: s.window_width as f64,
            window_height: s.window_height as f64,
            auto_save_delay_ms: s.auto_save_delay_ms,
            auto_indent_enabled: s.auto_indent_enabled,
            auto_indent_width: s.auto_indent_width,
            editor_typing_animation_enabled: s.editor_typing_animation_enabled,
            editor_smooth_cursor_enabled: s.editor_smooth_cursor_enabled,
            editor_typing_animation_duration_ms: s.editor_typing_animation_duration_ms,
            editor_smooth_cursor_duration_ms: s.editor_smooth_cursor_duration_ms,
            ai_enabled: s.ai_enabled,
            stats_device_id: s.stats_device_id,
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

impl From<SyncableSettingsDto> for crate::settings::SyncableSettings {
    fn from(s: SyncableSettingsDto) -> Self {
        crate::settings::SyncableSettings {
            font_size: s.font_size,
            theme_mode: s.theme_mode,
            monet_color: s.monet_color,
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
                crate::sync_service::SyncTransport::SshDeployKey => "ssh_deploy_key".to_string(),
            },
            branch: c.branch,
            auto_sync: c.auto_sync,
            sync_interval_seconds: c.sync_interval_seconds,
            proxy_enabled: c.proxy_enabled,
            proxy_type: c.proxy_type,
            proxy_host: c.proxy_host,
            proxy_port: c.proxy_port,
            username: c.username,
        }
    }
}

impl From<SyncConfigDto> for crate::sync_service::SyncConfig {
    fn from(c: SyncConfigDto) -> Self {
        crate::sync_service::SyncConfig {
            enabled: c.enabled,
            backend_type: match c.backend_type.as_str() {
                "git" => crate::sync_service::BackendType::Git,
                "github_api" => crate::sync_service::BackendType::GithubApi,
                "webdav" => crate::sync_service::BackendType::WebDav,
                "s3" => crate::sync_service::BackendType::S3,
                _ => crate::sync_service::BackendType::GithubApi,
            },
            remote_url: c.remote_url,
            transport: match c.transport.as_str() {
                "https_token" => crate::sync_service::SyncTransport::HttpsToken,
                "ssh" | "ssh_deploy_key" => crate::sync_service::SyncTransport::SshDeployKey,
                _ => crate::sync_service::SyncTransport::HttpsToken,
            },
            branch: c.branch,
            auto_sync: c.auto_sync,
            sync_interval_seconds: c.sync_interval_seconds,
            proxy_enabled: c.proxy_enabled,
            proxy_type: c.proxy_type,
            proxy_host: c.proxy_host,
            proxy_port: c.proxy_port,
            username: c.username,
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
        Self { token: s.token }
    }
}

impl From<SyncSecretsDto> for crate::sync_service::SyncSecrets {
    fn from(s: SyncSecretsDto) -> Self {
        crate::sync_service::SyncSecrets {
            token: s.token,
            ssh_private_key: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncStateDto {
    pub status: String,
    pub remote_url: Option<String>,
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
            status: "idle".to_string(),
            remote_url: s.remote_url,
            backend_type: None,
            transport: s.transport.map(sync_transport_to_wire),
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
            backend_type: d.backend_type,
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
            transport: d.transport,
            error_category: d.error_category,
            user_message: d.user_message,
            raw_error: d.raw_error,
            chosen_network_mode: d.chosen_network_mode,
            network_probe_summary: Some(
                d.network_probe_summary.into_iter().map(Into::into).collect(),
            ),
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
            status: sync_status_to_wire(&r.status),
            uploaded_files: r.uploaded_files,
            downloaded_files: r.downloaded_files,
            local_deletes: r.local_deletes,
            remote_deletes: r.remote_deletes,
            overwritten_files: r.overwritten_files,
            ignored_files: r.ignored_files,
            conflicts: r.conflicts.into_iter().map(Into::into).collect(),
            commit_hash: r.commit_hash,
            error: r.error,
            first_sync_mode: first_sync_mode_to_wire(&r.first_sync_mode),
            user_message: r.user_message,
            chosen_network_mode: r.chosen_network_mode,
            network_probe_summary: Some(
                r.network_probe_summary.into_iter().map(Into::into).collect(),
            ),
        }
    }
}

fn sync_transport_to_wire(transport: crate::sync_service::SyncTransport) -> String {
    match transport {
        crate::sync_service::SyncTransport::HttpsToken => "https_token".to_string(),
        crate::sync_service::SyncTransport::SshDeployKey => "ssh_deploy_key".to_string(),
    }
}

fn sync_status_to_wire(status: &crate::sync_service::SyncStatus) -> String {
    match status {
        crate::sync_service::SyncStatus::Idle => "idle",
        crate::sync_service::SyncStatus::Syncing => "syncing",
        crate::sync_service::SyncStatus::Success => "success",
        crate::sync_service::SyncStatus::ConfiguredUntested => "configured_untested",
        crate::sync_service::SyncStatus::Conflict => "conflict",
        crate::sync_service::SyncStatus::RecoverableError(_) => "recoverable_error",
        crate::sync_service::SyncStatus::FatalError(_) => "fatal_error",
        crate::sync_service::SyncStatus::DirtyRepoBlocked => "dirty_repo_blocked",
        crate::sync_service::SyncStatus::BranchMissingRecovered => "branch_missing_recovered",
        crate::sync_service::SyncStatus::Error(_) => "error",
        crate::sync_service::SyncStatus::NoChanges => "no_changes",
        crate::sync_service::SyncStatus::LatestWinsApplied => "latest_wins_applied",
    }
    .to_string()
}

fn first_sync_mode_to_wire(mode: &crate::sync_service::FirstSyncMode) -> String {
    match mode {
        crate::sync_service::FirstSyncMode::NotAttempted => "not_attempted",
        crate::sync_service::FirstSyncMode::CloneIntoEmptyWorkspace => {
            "clone_into_empty_workspace"
        }
        crate::sync_service::FirstSyncMode::InitExistingWorkspace => "init_existing_workspace",
        crate::sync_service::FirstSyncMode::AlreadyGitRepo => "already_git_repo",
        crate::sync_service::FirstSyncMode::BlockedNonEmptyRemote => "blocked_non_empty_remote",
        crate::sync_service::FirstSyncMode::UnrelatedHistories => "unrelated_histories",
    }
    .to_string()
}
