use crate::facade::ChapterOpenResult;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiagnosticsDto {
    pub has_workspace: bool,
    pub workspace_path: String,
    pub core_initialized: bool,
    pub path_exists: bool,
    pub is_dir: bool,
    pub manifest_path: String,
    pub manifest_exists: bool,
    pub projects_path: String,
    pub projects_dir_exists: bool,
    pub app_meta_exists: bool,
    pub writable: bool,
    pub writable_error: String,
    pub validate_workspace: bool,
    pub tree_count: u64,
    pub last_workspace_path: String,
    pub create_project_available: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct LocalSettingsDto {
    pub theme_mode: Option<String>,
    pub locale: Option<String>,
    pub auto_save_enabled: bool,
    pub editor_font_size: f32,
    pub editor_line_spacing_multiplier: f32,
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
    pub linux_sidebar_width: f64,
    pub linux_editor_width: f64,
}

impl From<crate::settings::LocalSettings> for LocalSettingsDto {
    fn from(s: crate::settings::LocalSettings) -> Self {
        Self {
            theme_mode: s.theme_mode,
            locale: s.locale,
            auto_save_enabled: s.auto_save_enabled,
            editor_font_size: s.editor_font_size,
            editor_line_spacing_multiplier: s.editor_line_spacing_multiplier,
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
            linux_sidebar_width: s.linux_sidebar_width,
            linux_editor_width: s.linux_editor_width,
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
            editor_line_spacing_multiplier: s.editor_line_spacing_multiplier,
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
            linux_sidebar_width: s.linux_sidebar_width,
            linux_editor_width: s.linux_editor_width,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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
    pub android_has_internet_permission: bool,
    pub android_has_access_network_state_permission: bool,
}

impl From<crate::sync_service::SyncConfig> for SyncConfigDto {
    fn from(c: crate::sync_service::SyncConfig) -> Self {
        Self {
            enabled: c.enabled,
            backend_type: match c.backend_type {
                crate::sync_service::BackendType::Git => "git".to_string(),
                crate::sync_service::BackendType::GithubApi => "github_api".to_string(),
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
            android_has_internet_permission: c.android_has_internet_permission,
            android_has_access_network_state_permission: c.android_has_access_network_state_permission,
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
            android_has_access_network_state_permission: c.android_has_access_network_state_permission,
            android_has_internet_permission: c.android_has_internet_permission,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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
            mode: p.mode.into(),
            success: p.success,
            status: p.status,
            message: p.message,
            raw_error: p.raw_error,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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
    pub error_category: Option<String>,
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
            error_category: r.error_category,
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


// MindMap DTOs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapSnapshotDto {
    pub project_id: String,
    pub layout_kind: String,
    pub nodes: Vec<MindMapSnapshotNodeDto>,
    pub edges: Vec<MindMapSnapshotEdgeDto>,
    pub bounds: MindMapBoundsDto,
    pub generated_at: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapSnapshotNodeDto {
    pub id: String,
    pub title: String,
    pub kind: MindMapNodeKindDto,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub collapsed: bool,
    pub anchor_count: u32,
    pub broken_link: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapSnapshotEdgeDto {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapBoundsDto {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl From<crate::mind_map::MindMapSnapshot> for MindMapSnapshotDto {
    fn from(s: crate::mind_map::MindMapSnapshot) -> Self {
        Self {
            project_id: s.project_id,
            layout_kind: s.layout_kind,
            nodes: s.nodes.into_iter().map(Into::into).collect(),
            edges: s.edges.into_iter().map(Into::into).collect(),
            bounds: s.bounds.into(),
            generated_at: s.generated_at,
        }
    }
}

impl From<crate::mind_map::MindMapSnapshotNode> for MindMapSnapshotNodeDto {
    fn from(n: crate::mind_map::MindMapSnapshotNode) -> Self {
        Self {
            id: n.id,
            title: n.title,
            kind: n.kind.into(),
            x: n.x,
            y: n.y,
            width: n.width,
            height: n.height,
            radius: n.radius,
            collapsed: n.collapsed,
            anchor_count: n.anchor_count as u32,
            broken_link: n.broken_link,
            tags: n.tags,
        }
    }
}

impl From<crate::mind_map::MindMapSnapshotEdge> for MindMapSnapshotEdgeDto {
    fn from(e: crate::mind_map::MindMapSnapshotEdge) -> Self {
        Self {
            id: e.id,
            from: e.from,
            to: e.to,
            kind: e.kind.into(),
            label: e.label,
        }
    }
}

impl From<crate::mind_map::MindMapBounds> for MindMapBoundsDto {
    fn from(b: crate::mind_map::MindMapBounds) -> Self {
        Self {
            min_x: b.min_x,
            min_y: b.min_y,
            max_x: b.max_x,
            max_y: b.max_y,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphDto {
    pub schema_version: u32,
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub nodes: Vec<MindMapGraphNodeDto>,
    pub edges: Vec<MindMapGraphEdgeDto>,
    pub anchors: Vec<MindMapAnchorDto>,
    pub links: Vec<MindMapLinkDto>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::mind_map::MindMapGraph> for MindMapGraphDto {
    fn from(g: crate::mind_map::MindMapGraph) -> Self {
        Self {
            schema_version: g.schema_version,
            id: g.id,
            project_id: g.project_id,
            title: g.title,
            nodes: g.nodes.into_iter().map(Into::into).collect(),
            edges: g.edges.into_iter().map(Into::into).collect(),
            anchors: g.anchors.into_iter().map(Into::into).collect(),
            links: g.links.into_iter().map(Into::into).collect(),
            created_at: g.created_at,
            updated_at: g.updated_at,
        }
    }
}

impl From<MindMapGraphDto> for crate::mind_map::MindMapGraph {
    fn from(d: MindMapGraphDto) -> Self {
        Self {
            schema_version: d.schema_version,
            id: d.id,
            project_id: d.project_id,
            title: d.title,
            nodes: d.nodes.into_iter().map(Into::into).collect(),
            edges: d.edges.into_iter().map(Into::into).collect(),
            anchors: d.anchors.into_iter().map(Into::into).collect(),
            links: d.links.into_iter().map(Into::into).collect(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphNodeDto {
    pub id: String,
    pub title: String,
    pub kind: MindMapNodeKindDto,
    pub payload: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::mind_map::MindMapGraphNode> for MindMapGraphNodeDto {
    fn from(n: crate::mind_map::MindMapGraphNode) -> Self {
        Self {
            id: n.id,
            title: n.title,
            kind: n.kind.into(),
            payload: n.payload.map(|v| serde_json::to_string(&v).unwrap_or_default()),
            tags: n.tags,
            created_at: n.created_at,
            updated_at: n.updated_at,
        }
    }
}

impl From<MindMapGraphNodeDto> for crate::mind_map::MindMapGraphNode {
    fn from(d: MindMapGraphNodeDto) -> Self {
        Self {
            id: d.id,
            title: d.title,
            kind: d.kind.into(),
            payload: d.payload.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            tags: d.tags,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphEdgeDto {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: MindMapEdgeKindDto,
    pub label: Option<String>,
    pub payload: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::mind_map::MindMapGraphEdge> for MindMapGraphEdgeDto {
    fn from(e: crate::mind_map::MindMapGraphEdge) -> Self {
        Self {
            id: e.id,
            from: e.from,
            to: e.to,
            kind: e.kind.into(),
            label: e.label,
            payload: e.payload.map(|v| serde_json::to_string(&v).unwrap_or_default()),
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

impl From<MindMapGraphEdgeDto> for crate::mind_map::MindMapGraphEdge {
    fn from(d: MindMapGraphEdgeDto) -> Self {
        Self {
            id: d.id,
            from: d.from,
            to: d.to,
            kind: d.kind.into(),
            label: d.label,
            payload: d.payload.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapAnchorDto {
    pub id: String,
    pub project_id: String,
    pub chapter_id: String,
    pub start_offset: u32,
    pub end_offset: u32,
    pub selected_text: String,
    pub prefix_text: String,
    pub suffix_text: String,
    pub checksum: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::mind_map::MindMapAnchor> for MindMapAnchorDto {
    fn from(a: crate::mind_map::MindMapAnchor) -> Self {
        Self {
            id: a.id,
            project_id: a.project_id,
            chapter_id: a.chapter_id,
            start_offset: a.start_offset as u32,
            end_offset: a.end_offset as u32,
            selected_text: a.selected_text,
            prefix_text: a.prefix_text,
            suffix_text: a.suffix_text,
            checksum: a.checksum,
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

impl From<MindMapAnchorDto> for crate::mind_map::MindMapAnchor {
    fn from(d: MindMapAnchorDto) -> Self {
        Self {
            id: d.id,
            project_id: d.project_id,
            chapter_id: d.chapter_id,
            start_offset: d.start_offset as usize,
            end_offset: d.end_offset as usize,
            selected_text: d.selected_text,
            prefix_text: d.prefix_text,
            suffix_text: d.suffix_text,
            checksum: d.checksum,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapLinkDto {
    pub id: String,
    pub node_id: String,
    pub anchor_id: String,
    pub kind: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::mind_map::MindMapLink> for MindMapLinkDto {
    fn from(l: crate::mind_map::MindMapLink) -> Self {
        Self {
            id: l.id,
            node_id: l.node_id,
            anchor_id: l.anchor_id,
            kind: l.kind.into(),
            created_at: l.created_at,
            updated_at: l.updated_at,
        }
    }
}

impl From<MindMapLinkDto> for crate::mind_map::MindMapLink {
    fn from(d: MindMapLinkDto) -> Self {
        Self {
            id: d.id,
            node_id: d.node_id,
            anchor_id: d.anchor_id,
            kind: d.kind,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapLayoutDto {
    pub kind: MindMapLayoutKindDto,
    pub nodes: Vec<MindMapLayoutNodeDto>,
}

impl From<crate::mind_map::MindMapLayout> for MindMapLayoutDto {
    fn from(l: crate::mind_map::MindMapLayout) -> Self {
        Self {
            kind: l.kind.into(),
            nodes: l.nodes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<MindMapLayoutDto> for crate::mind_map::MindMapLayout {
    fn from(d: MindMapLayoutDto) -> Self {
        Self {
            kind: d.kind.into(),
            nodes: d.nodes.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapLayoutNodeDto {
    pub node_id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub collapsed: bool,
    pub z_index: i32,
}

impl From<crate::mind_map::MindMapLayoutNode> for MindMapLayoutNodeDto {
    fn from(n: crate::mind_map::MindMapLayoutNode) -> Self {
        Self {
            node_id: n.node_id,
            x: n.x,
            y: n.y,
            width: n.width,
            height: n.height,
            radius: n.radius,
            collapsed: n.collapsed,
            z_index: n.z_index,
        }
    }
}

impl From<MindMapLayoutNodeDto> for crate::mind_map::MindMapLayoutNode {
    fn from(d: MindMapLayoutNodeDto) -> Self {
        Self {
            node_id: d.node_id,
            x: d.x,
            y: d.y,
            width: d.width,
            height: d.height,
            radius: d.radius,
            collapsed: d.collapsed,
            z_index: d.z_index,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphMetadataDto {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::mind_map::edit::MindMapGraphMetadata> for MindMapGraphMetadataDto {
    fn from(m: crate::mind_map::edit::MindMapGraphMetadata) -> Self {
        Self {
            id: m.id,
            title: m.title,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphsListDto {
    pub default_graph_id: Option<String>,
    pub graphs: Vec<MindMapGraphMetadataDto>,
}

impl From<crate::mind_map::edit::MindMapGraphsList> for MindMapGraphsListDto {
    fn from(l: crate::mind_map::edit::MindMapGraphsList) -> Self {
        Self {
            default_graph_id: l.default_graph_id,
            graphs: l.graphs.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapNodePatchDto {
    pub title: Option<String>,
    pub kind: Option<MindMapNodeKindDto>,
    pub payload: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
}

impl From<MindMapNodePatchDto> for crate::mind_map::edit::MindMapGraphNodePatch {
    fn from(d: MindMapNodePatchDto) -> Self {
        Self {
            title: d.title,
            kind: d.kind.map(Into::into),
            payload: d.payload.map(|opt| opt.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))),
            tags: d.tags,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapEdgePatchDto {
    pub kind: Option<MindMapEdgeKindDto>,
    pub label: Option<Option<String>>,
    pub payload: Option<Option<String>>,
}

impl From<MindMapEdgePatchDto> for crate::mind_map::edit::MindMapGraphEdgePatch {
    fn from(d: MindMapEdgePatchDto) -> Self {
        Self {
            kind: d.kind.map(Into::into),
            label: d.label,
            payload: d.payload.map(|opt| opt.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))),
        }
    }
}


// StarMap DTOs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapMetaDto {
    pub starmap_id: String,
    pub title: String,
    pub description: String,
    pub project_id: Option<String>,
    pub parent_starmap_id: Option<String>,
    pub is_main_for_project: bool,
    pub accent_color: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub node_count: u32,
    pub edge_count: u32,
    pub linked_chapter_count: u32,
    pub child_starmap_count: u32,
}

impl From<crate::starmap::StarMapMeta> for StarMapMetaDto {
    fn from(m: crate::starmap::StarMapMeta) -> Self {
        Self {
            starmap_id: m.starmap_id,
            title: m.title,
            description: m.description,
            project_id: m.project_id,
            parent_starmap_id: m.parent_starmap_id,
            is_main_for_project: m.is_main_for_project,
            accent_color: m.accent_color,
            created_at: m.created_at,
            updated_at: m.updated_at,
            node_count: m.node_count,
            edge_count: m.edge_count,
            linked_chapter_count: m.linked_chapter_count,
            child_starmap_count: m.child_starmap_count,
        }
    }
}

impl From<StarMapMetaDto> for crate::starmap::StarMapMeta {
    fn from(d: StarMapMetaDto) -> Self {
        Self {
            starmap_id: d.starmap_id,
            title: d.title,
            description: d.description,
            project_id: d.project_id,
            parent_starmap_id: d.parent_starmap_id,
            is_main_for_project: d.is_main_for_project,
            accent_color: d.accent_color,
            created_at: d.created_at,
            updated_at: d.updated_at,
            node_count: d.node_count,
            edge_count: d.edge_count,
            linked_chapter_count: d.linked_chapter_count,
            child_starmap_count: d.child_starmap_count,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapGraphDto {
    pub schema_version: u32,
    pub id: String,
    pub starmap_id: String,
    pub title: String,
    pub nodes: Vec<StarMapNodeDto>,
    pub edges: Vec<StarMapEdgeDto>,
    #[serde(default)]
    pub embeds: Vec<StarMapEmbedDto>,
    #[serde(default)]
    pub links: Vec<StarMapLinkDto>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapGraph> for StarMapGraphDto {
    fn from(g: crate::starmap::types::StarMapGraph) -> Self {
        Self {
            schema_version: g.schema_version,
            id: g.id,
            starmap_id: g.starmap_id,
            title: g.title,
            nodes: g.nodes.into_iter().map(Into::into).collect(),
            edges: g.edges.into_iter().map(Into::into).collect(),
            embeds: g.embeds.into_iter().map(Into::into).collect(),
            links: g.links.into_iter().map(Into::into).collect(),
            created_at: g.created_at,
            updated_at: g.updated_at,
        }
    }
}

impl From<StarMapGraphDto> for crate::starmap::types::StarMapGraph {
    fn from(d: StarMapGraphDto) -> Self {
        Self {
            schema_version: d.schema_version,
            id: d.id,
            starmap_id: d.starmap_id,
            title: d.title,
            nodes: d.nodes.into_iter().map(Into::into).collect(),
            edges: d.edges.into_iter().map(Into::into).collect(),
            embeds: d.embeds.into_iter().map(Into::into).collect(),
            links: d.links.into_iter().map(Into::into).collect(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodeDto {
    pub id: String,
    pub title: String,
    pub kind: StarMapNodeKindDto,
    pub payload: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub content: StarMapNodeContentDto,
    #[serde(default)]
    pub anchors: Vec<StarMapAnchorDto>,
    #[serde(default)]
    pub portal: Option<StarMapPortalDto>,
    #[serde(default)]
    pub display_policy: StarMapDisplayPolicyDto,
    #[serde(default)]
    pub open_behavior: StarMapOpenBehaviorDto,
    #[serde(default)]
    pub provenance: StarMapProvenanceDto,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapNode> for StarMapNodeDto {
    fn from(n: crate::starmap::types::StarMapNode) -> Self {
        Self {
            id: n.id,
            title: n.title,
            kind: n.kind.into(),
            payload: n.payload.map(|v| serde_json::to_string(&v).unwrap_or_default()),
            tags: n.tags,
            content: n.content.into(),
            anchors: n.anchors.into_iter().map(Into::into).collect(),
            portal: n.portal.map(Into::into),
            display_policy: n.display_policy.into(),
            open_behavior: n.open_behavior.into(),
            provenance: n.provenance.into(),
            created_at: n.created_at,
            updated_at: n.updated_at,
        }
    }
}

impl From<StarMapNodeDto> for crate::starmap::types::StarMapNode {
    fn from(d: StarMapNodeDto) -> Self {
        Self {
            id: d.id,
            title: d.title,
            kind: d.kind.into(),
            payload: d.payload.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            tags: d.tags,
            content: d.content.into(),
            anchors: d.anchors.into_iter().map(Into::into).collect(),
            portal: d.portal.map(Into::into),
            display_policy: d.display_policy.into(),
            open_behavior: d.open_behavior.into(),
            provenance: d.provenance.into(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

// Flattened struct (was tagged enum StarMapNodeContentDto)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodeContentDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub summary: Option<String>,
    pub body: Option<String>,
    pub project_id: Option<String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub range_start: Option<u32>,
    pub range_end: Option<u32>,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub uri: Option<String>,
    pub label: Option<String>,
}

impl From<crate::starmap::semantic::StarMapNodeContent> for StarMapNodeContentDto {
    fn from(c: crate::starmap::semantic::StarMapNodeContent) -> Self {
        match c {
            crate::starmap::semantic::StarMapNodeContent::Empty => Self {
                kind: "empty".to_string(),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapNodeContent::Inline { summary, body } => Self {
                kind: "inline".to_string(),
                summary,
                body,
                ..Default::default()
            },
            crate::starmap::semantic::StarMapNodeContent::ChapterRef { project_id, volume_id, chapter_id, range_start, range_end } => Self {
                kind: "chapterRef".to_string(),
                project_id: Some(project_id),
                volume_id,
                chapter_id: Some(chapter_id),
                range_start,
                range_end,
                ..Default::default()
            },
            crate::starmap::semantic::StarMapNodeContent::EntityRef { entity_type, entity_id } => Self {
                kind: "entityRef".to_string(),
                entity_type: Some(entity_type),
                entity_id: Some(entity_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapNodeContent::ExternalRef { uri, label } => Self {
                kind: "externalRef".to_string(),
                uri: Some(uri),
                label,
                ..Default::default()
            },
        }
    }
}

impl From<StarMapNodeContentDto> for crate::starmap::semantic::StarMapNodeContent {
    fn from(d: StarMapNodeContentDto) -> Self {
        match d.kind.as_str() {
            "inline" => Self::Inline {
                summary: d.summary,
                body: d.body,
            },
            "chapterRef" => Self::ChapterRef {
                project_id: d.project_id.unwrap_or_default(),
                volume_id: d.volume_id,
                chapter_id: d.chapter_id.unwrap_or_default(),
                range_start: d.range_start,
                range_end: d.range_end,
            },
            "entityRef" => Self::EntityRef {
                entity_type: d.entity_type.unwrap_or_default(),
                entity_id: d.entity_id.unwrap_or_default(),
            },
            "externalRef" => Self::ExternalRef {
                uri: d.uri.unwrap_or_default(),
                label: d.label,
            },
            _ => Self::Empty,
        }
    }
}

// Since StarMapAnchor depends on semantic enums, we copy its structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapAnchorDto {
    pub anchor_id: String,
    pub target: StarMapAnchorTargetDto,
    pub label: Option<String>,
    #[serde(default)]
    pub role: StarMapAnchorRoleDto,
}

impl From<crate::starmap::semantic::StarMapAnchor> for StarMapAnchorDto {
    fn from(a: crate::starmap::semantic::StarMapAnchor) -> Self {
        Self {
            anchor_id: a.anchor_id,
            target: a.target.into(),
            label: a.label,
            role: a.role.into(),
        }
    }
}

impl From<StarMapAnchorDto> for crate::starmap::semantic::StarMapAnchor {
    fn from(d: StarMapAnchorDto) -> Self {
        Self {
            anchor_id: d.anchor_id,
            target: d.target.into(),
            label: d.label,
            role: d.role.into(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPortalDto {
    pub target_starmap_id: String,
    #[serde(default)]
    pub deep_target: Option<StarMapDeepTargetDto>,
    #[serde(default)]
    pub mode: StarMapPortalModeDto,
    #[serde(default)]
    pub preview_policy: StarMapPortalPreviewPolicyDto,
}

impl From<crate::starmap::semantic::StarMapPortal> for StarMapPortalDto {
    fn from(p: crate::starmap::semantic::StarMapPortal) -> Self {
        Self {
            target_starmap_id: p.target_starmap_id,
            deep_target: p.deep_target.map(Into::into),
            mode: p.mode.into(),
            preview_policy: p.preview_policy.into(),
        }
    }
}

impl From<StarMapPortalDto> for crate::starmap::semantic::StarMapPortal {
    fn from(d: StarMapPortalDto) -> Self {
        Self {
            target_starmap_id: d.target_starmap_id,
            deep_target: d.deep_target.map(Into::into),
            mode: d.mode.into(),
            preview_policy: d.preview_policy.into(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapDisplayPolicyDto {
    pub importance: f32,
    pub min_visible_scale: f32,
    pub title_scale: f32,
    pub summary_scale: f32,
    pub detail_scale: f32,
    pub max_preview_chars: u32,
    pub min_readable_px: f32,
}

impl Default for StarMapDisplayPolicyDto {
    fn default() -> Self {
        crate::starmap::semantic::StarMapDisplayPolicy::default().into()
    }
}

impl From<crate::starmap::semantic::StarMapDisplayPolicy> for StarMapDisplayPolicyDto {
    fn from(p: crate::starmap::semantic::StarMapDisplayPolicy) -> Self {
        Self {
            importance: p.importance,
            min_visible_scale: p.min_visible_scale,
            title_scale: p.title_scale,
            summary_scale: p.summary_scale,
            detail_scale: p.detail_scale,
            max_preview_chars: p.max_preview_chars,
            min_readable_px: p.min_readable_px,
        }
    }
}

impl From<StarMapDisplayPolicyDto> for crate::starmap::semantic::StarMapDisplayPolicy {
    fn from(d: StarMapDisplayPolicyDto) -> Self {
        Self {
            importance: d.importance,
            min_visible_scale: d.min_visible_scale,
            title_scale: d.title_scale,
            summary_scale: d.summary_scale,
            detail_scale: d.detail_scale,
            max_preview_chars: d.max_preview_chars,
            min_readable_px: d.min_readable_px,
        }
    }
}

// We map OpenBehavior to string simply, or enum wrapper. Since it's enum in crate, we wrap it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapOpenBehaviorDto {
    #[default]
    Inspector,
    ExpandCard,
    WritingMode,
    JumpToAnchor,
    EnterPortal,
    Custom,
}

impl From<crate::starmap::semantic::StarMapOpenBehavior> for StarMapOpenBehaviorDto {
    fn from(b: crate::starmap::semantic::StarMapOpenBehavior) -> Self {
        match b {
            crate::starmap::semantic::StarMapOpenBehavior::Inspector => Self::Inspector,
            crate::starmap::semantic::StarMapOpenBehavior::ExpandCard => Self::ExpandCard,
            crate::starmap::semantic::StarMapOpenBehavior::WritingMode => Self::WritingMode,
            crate::starmap::semantic::StarMapOpenBehavior::JumpToAnchor => Self::JumpToAnchor,
            crate::starmap::semantic::StarMapOpenBehavior::EnterPortal => Self::EnterPortal,
            crate::starmap::semantic::StarMapOpenBehavior::Custom => Self::Custom,
        }
    }
}

impl From<StarMapOpenBehaviorDto> for crate::starmap::semantic::StarMapOpenBehavior {
    fn from(d: StarMapOpenBehaviorDto) -> Self {
        match d {
            StarMapOpenBehaviorDto::Inspector => Self::Inspector,
            StarMapOpenBehaviorDto::ExpandCard => Self::ExpandCard,
            StarMapOpenBehaviorDto::WritingMode => Self::WritingMode,
            StarMapOpenBehaviorDto::JumpToAnchor => Self::JumpToAnchor,
            StarMapOpenBehaviorDto::EnterPortal => Self::EnterPortal,
            StarMapOpenBehaviorDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapProvenanceDto {
    #[serde(default)]
    pub source: StarMapSourceKindDto,
    pub source_id: Option<String>,
    pub generated_by: Option<String>,
    pub prompt_id: Option<String>,
    #[serde(default)]
    pub review_status: StarMapReviewStatusDto,
    pub created_from_anchor: Option<String>,
}

impl From<crate::starmap::semantic::StarMapProvenance> for StarMapProvenanceDto {
    fn from(p: crate::starmap::semantic::StarMapProvenance) -> Self {
        Self {
            source: p.source.into(),
            source_id: p.source_id,
            generated_by: p.generated_by,
            prompt_id: p.prompt_id,
            review_status: p.review_status.into(),
            created_from_anchor: p.created_from_anchor,
        }
    }
}

impl From<StarMapProvenanceDto> for crate::starmap::semantic::StarMapProvenance {
    fn from(d: StarMapProvenanceDto) -> Self {
        Self {
            source: d.source.into(),
            source_id: d.source_id,
            generated_by: d.generated_by,
            prompt_id: d.prompt_id,
            review_status: d.review_status.into(),
            created_from_anchor: d.created_from_anchor,
        }
    }
}

impl Default for StarMapProvenanceDto {
    fn default() -> Self {
        crate::starmap::semantic::StarMapProvenance::default().into()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapDeepTargetDto {
    pub starmap_id: String,
    #[serde(default)]
    pub path: Vec<StarMapPathSegmentDto>,
    pub target: StarMapTargetDetailDto,
}

impl From<crate::starmap::semantic::StarMapDeepTarget> for StarMapDeepTargetDto {
    fn from(t: crate::starmap::semantic::StarMapDeepTarget) -> Self {
        Self {
            starmap_id: t.starmap_id,
            path: t.path.into_iter().map(Into::into).collect(),
            target: t.target.into(),
        }
    }
}

impl From<StarMapDeepTargetDto> for crate::starmap::semantic::StarMapDeepTarget {
    fn from(d: StarMapDeepTargetDto) -> Self {
        Self {
            starmap_id: d.starmap_id,
            path: d.path.into_iter().map(Into::into).collect(),
            target: d.target.into(),
        }
    }
}

// Flattened struct (was tagged enum StarMapPathSegmentDto)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPathSegmentDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub starmap_id: Option<String>,
    pub node_id: Option<String>,
}

impl From<crate::starmap::semantic::StarMapPathSegment> for StarMapPathSegmentDto {
    fn from(s: crate::starmap::semantic::StarMapPathSegment) -> Self {
        match s {
            crate::starmap::semantic::StarMapPathSegment::EnterChild { starmap_id } => Self {
                kind: "enterChild".to_string(),
                starmap_id: Some(starmap_id),
                node_id: None,
            },
            crate::starmap::semantic::StarMapPathSegment::EnterNode { node_id } => Self {
                kind: "enterNode".to_string(),
                starmap_id: None,
                node_id: Some(node_id),
            },
        }
    }
}

impl From<StarMapPathSegmentDto> for crate::starmap::semantic::StarMapPathSegment {
    fn from(d: StarMapPathSegmentDto) -> Self {
        match d.kind.as_str() {
            "enterChild" => Self::EnterChild {
                starmap_id: d.starmap_id.unwrap_or_default(),
            },
            _ => Self::EnterNode {
                node_id: d.node_id.unwrap_or_default(),
            },
        }
    }
}

// Flattened struct (was tagged enum StarMapEdgeEndpointDto)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgeEndpointDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub node_id: Option<String>,
    pub anchor_id: Option<String>,
    pub target: Option<StarMapDeepTargetDto>,
}

impl From<crate::starmap::types::StarMapEdgeEndpoint> for StarMapEdgeEndpointDto {
    fn from(e: crate::starmap::types::StarMapEdgeEndpoint) -> Self {
        match e {
            crate::starmap::types::StarMapEdgeEndpoint::Node { node_id } => Self {
                kind: "node".to_string(),
                node_id: Some(node_id),
                anchor_id: None,
                target: None,
            },
            crate::starmap::types::StarMapEdgeEndpoint::Anchor { node_id, anchor_id } => Self {
                kind: "anchor".to_string(),
                node_id: Some(node_id),
                anchor_id: Some(anchor_id),
                target: None,
            },
            crate::starmap::types::StarMapEdgeEndpoint::Starmap => Self {
                kind: "starmap".to_string(),
                node_id: None,
                anchor_id: None,
                target: None,
            },
            crate::starmap::types::StarMapEdgeEndpoint::DeepTarget { target } => Self {
                kind: "deepTarget".to_string(),
                node_id: None,
                anchor_id: None,
                target: Some(target.into()),
            },
        }
    }
}

impl From<StarMapEdgeEndpointDto> for crate::starmap::types::StarMapEdgeEndpoint {
    fn from(d: StarMapEdgeEndpointDto) -> Self {
        match d.kind.as_str() {
            "anchor" => Self::Anchor {
                node_id: d.node_id.unwrap_or_default(),
                anchor_id: d.anchor_id.unwrap_or_default(),
            },
            "starmap" => Self::Starmap,
            "deepTarget" => Self::DeepTarget {
                target: d.target.map(Into::into).unwrap_or_else(|| crate::starmap::semantic::StarMapDeepTarget {
                    starmap_id: String::new(),
                    path: vec![],
                    target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
                }),
            },
            _ => Self::Node {
                node_id: d.node_id.unwrap_or_default(),
            },
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgeDto {
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub kind: StarMapEdgeKindDto,
    pub label: Option<String>,
    pub payload: Option<String>,
    #[serde(default)]
    pub from_target: Option<StarMapDeepTargetDto>,
    #[serde(default)]
    pub to_target: Option<StarMapDeepTargetDto>,
    #[serde(default)]
    pub from_endpoint: Option<StarMapEdgeEndpointDto>,
    #[serde(default)]
    pub to_endpoint: Option<StarMapEdgeEndpointDto>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapEdge> for StarMapEdgeDto {
    fn from(e: crate::starmap::types::StarMapEdge) -> Self {
        Self {
            id: e.id,
            from: e.from,
            to: e.to,
            kind: e.kind.into(),
            label: e.label,
            payload: e.payload.map(|v| serde_json::to_string(&v).unwrap_or_default()),
            from_target: e.from_target.map(Into::into),
            to_target: e.to_target.map(Into::into),
            from_endpoint: e.from_endpoint.map(Into::into),
            to_endpoint: e.to_endpoint.map(Into::into),
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

impl From<StarMapEdgeDto> for crate::starmap::types::StarMapEdge {
    fn from(d: StarMapEdgeDto) -> Self {
        Self {
            id: d.id,
            from: d.from,
            to: d.to,
            kind: d.kind.into(),
            label: d.label,
            payload: d.payload.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            from_target: d.from_target.map(Into::into),
            to_target: d.to_target.map(Into::into),
            from_endpoint: d.from_endpoint.map(Into::into),
            to_endpoint: d.to_endpoint.map(Into::into),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPlacementDto {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub z_index: i32,
    pub collapsed: bool,
}

impl From<crate::starmap::types::StarMapEmbedPlacement> for StarMapEmbedPlacementDto {
    fn from(p: crate::starmap::types::StarMapEmbedPlacement) -> Self {
        Self {
            x: p.x,
            y: p.y,
            width: p.width,
            height: p.height,
            scale: p.scale,
            z_index: p.z_index,
            collapsed: p.collapsed,
        }
    }
}

impl From<StarMapEmbedPlacementDto> for crate::starmap::types::StarMapEmbedPlacement {
    fn from(d: StarMapEmbedPlacementDto) -> Self {
        Self {
            x: d.x,
            y: d.y,
            width: d.width,
            height: d.height,
            scale: d.scale,
            z_index: d.z_index,
            collapsed: d.collapsed,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedViewportDto {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl From<crate::starmap::types::StarMapEmbedViewport> for StarMapEmbedViewportDto {
    fn from(v: crate::starmap::types::StarMapEmbedViewport) -> Self {
        Self {
            scale: v.scale,
            offset_x: v.offset_x,
            offset_y: v.offset_y,
        }
    }
}

impl From<StarMapEmbedViewportDto> for crate::starmap::types::StarMapEmbedViewport {
    fn from(d: StarMapEmbedViewportDto) -> Self {
        Self {
            scale: d.scale,
            offset_x: d.offset_x,
            offset_y: d.offset_y,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedDto {
    pub instance_id: String,
    pub target_starmap_id: String,
    pub label: Option<String>,
    pub display_policy: StarMapDisplayPolicyDto,
    pub open_behavior: StarMapOpenBehaviorDto,
    pub placement: StarMapEmbedPlacementDto,
    pub target_viewport: StarMapEmbedViewportDto,
    pub source_node_id: Option<String>,
    pub host_endpoint: Option<StarMapEndpointDto>,
    pub provenance: StarMapProvenanceDto,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapEmbed> for StarMapEmbedDto {
    fn from(e: crate::starmap::types::StarMapEmbed) -> Self {
        Self {
            instance_id: e.instance_id,
            target_starmap_id: e.target_starmap_id,
            label: e.label,
            display_policy: e.display_policy.into(),
            open_behavior: e.open_behavior.into(),
            placement: e.placement.into(),
            target_viewport: e.target_viewport.into(),
            source_node_id: e.source_node_id,
            host_endpoint: e.host_endpoint.map(Into::into),
            provenance: e.provenance.into(),
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

impl From<StarMapEmbedDto> for crate::starmap::types::StarMapEmbed {
    fn from(d: StarMapEmbedDto) -> Self {
        Self {
            instance_id: d.instance_id,
            target_starmap_id: d.target_starmap_id,
            label: d.label,
            display_policy: d.display_policy.into(),
            open_behavior: d.open_behavior.into(),
            placement: d.placement.into(),
            target_viewport: d.target_viewport.into(),
            source_node_id: d.source_node_id,
            host_endpoint: d.host_endpoint.map(Into::into),
            provenance: d.provenance.into(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkDto {
    pub link_id: String,
    pub source: StarMapEndpointDto,
    pub target: StarMapDeepTargetDto,
    pub label: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapLink> for StarMapLinkDto {
    fn from(l: crate::starmap::types::StarMapLink) -> Self {
        Self {
            link_id: l.link_id,
            source: l.source.into(),
            target: l.target.into(),
            label: l.label,
            created_at: l.created_at,
            updated_at: l.updated_at,
        }
    }
}

impl From<StarMapLinkDto> for crate::starmap::types::StarMapLink {
    fn from(d: StarMapLinkDto) -> Self {
        Self {
            link_id: d.link_id,
            source: d.source.into(),
            target: d.target.into(),
            label: d.label,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLayoutDto {
    pub kind: StarMapLayoutKindDto,
    pub nodes: Vec<StarMapLayoutNodeDto>,
}

impl From<crate::starmap::types::StarMapLayout> for StarMapLayoutDto {
    fn from(l: crate::starmap::types::StarMapLayout) -> Self {
        Self {
            kind: l.kind.into(),
            nodes: l.nodes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<StarMapLayoutDto> for crate::starmap::types::StarMapLayout {
    fn from(d: StarMapLayoutDto) -> Self {
        Self {
            kind: d.kind.into(),
            nodes: d.nodes.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLayoutNodeDto {
    pub node_id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub collapsed: bool,
    pub z_index: i32,
    pub scale: f32,
    pub depth: f32,
    pub focus_weight: f32,
    pub orbit_group: Option<String>,
}

impl From<crate::starmap::types::StarMapLayoutNode> for StarMapLayoutNodeDto {
    fn from(n: crate::starmap::types::StarMapLayoutNode) -> Self {
        Self {
            node_id: n.node_id,
            x: n.x,
            y: n.y,
            width: n.width,
            height: n.height,
            radius: n.radius,
            collapsed: n.collapsed,
            z_index: n.z_index,
            scale: n.scale,
            depth: n.depth,
            focus_weight: n.focus_weight,
            orbit_group: n.orbit_group,
        }
    }
}

impl From<StarMapLayoutNodeDto> for crate::starmap::types::StarMapLayoutNode {
    fn from(d: StarMapLayoutNodeDto) -> Self {
        Self {
            node_id: d.node_id,
            x: d.x,
            y: d.y,
            width: d.width,
            height: d.height,
            radius: d.radius,
            collapsed: d.collapsed,
            z_index: d.z_index,
            scale: d.scale,
            depth: d.depth,
            focus_weight: d.focus_weight,
            orbit_group: d.orbit_group,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapReferenceDto {
    pub host_starmap_id: String,
    pub host_title: String,
    pub ref_type: String,
    pub ref_id: String,
    pub target_starmap_id: String,
}

impl From<crate::starmap::StarMapReference> for StarMapReferenceDto {
    fn from(r: crate::starmap::StarMapReference) -> Self {
        Self {
            host_starmap_id: r.host_starmap_id,
            host_title: r.host_title,
            ref_type: r.ref_type,
            ref_id: r.ref_id,
            target_starmap_id: r.target_starmap_id,
        }
    }
}

impl From<StarMapReferenceDto> for crate::starmap::StarMapReference {
    fn from(d: StarMapReferenceDto) -> Self {
        Self {
            host_starmap_id: d.host_starmap_id,
            host_title: d.host_title,
            ref_type: d.ref_type,
            ref_id: d.ref_id,
            target_starmap_id: d.target_starmap_id,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodePatchDto {
    pub title: Option<String>,
    pub kind: Option<StarMapNodeKindDto>,
    pub payload: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
    pub content: Option<StarMapNodeContentDto>,
    pub anchors: Option<Vec<StarMapAnchorDto>>,
    pub portal: Option<Option<StarMapPortalDto>>,
    pub display_policy: Option<StarMapDisplayPolicyDto>,
    pub open_behavior: Option<StarMapOpenBehaviorDto>,
    pub provenance: Option<StarMapProvenanceDto>,
}

impl From<StarMapNodePatchDto> for crate::starmap::types::StarMapNodePatch {
    fn from(d: StarMapNodePatchDto) -> Self {
        Self {
            title: d.title,
            kind: d.kind.map(Into::into),
            payload: d.payload.map(|opt| opt.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))),
            tags: d.tags,
            content: d.content.map(Into::into),
            anchors: d.anchors.map(|v| v.into_iter().map(Into::into).collect()),
            portal: d.portal.map(|p| p.map(Into::into)),
            display_policy: d.display_policy.map(Into::into),
            open_behavior: d.open_behavior.map(Into::into),
            provenance: d.provenance.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgePatchDto {
    pub kind: Option<StarMapEdgeKindDto>,
    pub label: Option<Option<String>>,
    pub payload: Option<Option<String>>,
    pub from_target: Option<Option<StarMapDeepTargetDto>>,
    pub to_target: Option<Option<StarMapDeepTargetDto>>,
    pub from_endpoint: Option<Option<StarMapEdgeEndpointDto>>,
    pub to_endpoint: Option<Option<StarMapEdgeEndpointDto>>,
}

impl From<StarMapEdgePatchDto> for crate::starmap::types::StarMapEdgePatch {
    fn from(d: StarMapEdgePatchDto) -> Self {
        Self {
            kind: d.kind.map(Into::into),
            label: d.label,
            payload: d.payload.map(|opt| opt.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))),
            from_target: d.from_target.map(|v| v.map(Into::into)),
            to_target: d.to_target.map(|v| v.map(Into::into)),
            from_endpoint: d.from_endpoint.map(|v| v.map(Into::into)),
            to_endpoint: d.to_endpoint.map(|v| v.map(Into::into)),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPatchDto {
    pub label: Option<Option<String>>,
    pub display_policy: Option<StarMapDisplayPolicyDto>,
    pub open_behavior: Option<StarMapOpenBehaviorDto>,
    pub viewport: Option<Option<StarMapViewportDto>>,
    pub placement: Option<Option<StarMapEmbedPlacementDto>>,
    pub target_viewport: Option<Option<StarMapEmbedViewportDto>>,
    pub source_node_id: Option<Option<String>>,
    pub host_anchor: Option<Option<String>>,
    pub host_endpoint: Option<Option<StarMapEndpointDto>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPatchInputDto {
    pub label: Option<String>,
    pub clear_label: bool,
    pub display_policy: Option<StarMapDisplayPolicyDto>,
    pub open_behavior: Option<StarMapOpenBehaviorDto>,
    pub viewport: Option<StarMapViewportDto>,
    pub clear_viewport: bool,
    pub placement: Option<StarMapEmbedPlacementDto>,
    pub clear_placement: bool,
    pub target_viewport: Option<StarMapEmbedViewportDto>,
    pub clear_target_viewport: bool,
    pub source_node_id: Option<String>,
    pub clear_source_node_id: bool,
    pub host_anchor: Option<String>,
    pub clear_host_anchor: bool,
    pub host_endpoint: Option<StarMapEndpointDto>,
    pub clear_host_endpoint: bool,
}

impl From<StarMapEmbedPatchInputDto> for StarMapEmbedPatchDto {
    fn from(d: StarMapEmbedPatchInputDto) -> Self {
        Self {
            label: if d.clear_label {
                Some(None)
            } else {
                d.label.map(Some)
            },
            display_policy: d.display_policy,
            open_behavior: d.open_behavior,
            viewport: if d.clear_viewport {
                Some(None)
            } else {
                d.viewport.map(Some)
            },
            placement: if d.clear_placement {
                Some(None)
            } else {
                d.placement.map(Some)
            },
            target_viewport: if d.clear_target_viewport {
                Some(None)
            } else {
                d.target_viewport.map(Some)
            },
            source_node_id: if d.clear_source_node_id {
                Some(None)
            } else {
                d.source_node_id.map(Some)
            },
            host_anchor: if d.clear_host_anchor {
                Some(None)
            } else {
                d.host_anchor.map(Some)
            },
            host_endpoint: if d.clear_host_endpoint {
                Some(None)
            } else {
                d.host_endpoint.map(Some)
            },
        }
    }
}

impl From<StarMapEmbedPatchDto> for crate::starmap::types::StarMapEmbedPatch {
    fn from(d: StarMapEmbedPatchDto) -> Self {
        Self {
            label: d.label,
            display_policy: d.display_policy.map(Into::into),
            open_behavior: d.open_behavior.map(Into::into),
            viewport: d.viewport.map(|v| v.map(Into::into)),
            placement: d.placement.map(|p| p.map(Into::into)),
            target_viewport: d.target_viewport.map(|v| v.map(Into::into)),
            source_node_id: d.source_node_id,
            host_anchor: d.host_anchor,
            host_endpoint: d.host_endpoint.map(|v| v.map(Into::into)),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkPatchDto {
    pub source: Option<StarMapEndpointDto>,
    pub target: Option<StarMapDeepTargetDto>,
    pub label: Option<Option<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkPatchInputDto {
    pub source: Option<StarMapEndpointDto>,
    pub target: Option<StarMapDeepTargetDto>,
    pub label: Option<String>,
    pub clear_label: bool,
}

impl From<StarMapLinkPatchInputDto> for StarMapLinkPatchDto {
    fn from(d: StarMapLinkPatchInputDto) -> Self {
        Self {
            source: d.source,
            target: d.target,
            label: if d.clear_label {
                Some(None)
            } else {
                d.label.map(Some)
            },
        }
    }
}

impl From<StarMapLinkPatchDto> for crate::starmap::types::StarMapLinkPatch {
    fn from(d: StarMapLinkPatchDto) -> Self {
        Self {
            source: d.source.map(Into::into),
            target: d.target.map(Into::into),
            label: d.label,
        }
    }
}


// WritingStats DTOs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DateRangeDto {
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingStatsSummaryDto {
    pub range: DateRangeDto,
    pub total_human_typed_chars: u64,
    pub total_pasted_chars: u64,
    pub total_deleted_chars: u64,
    pub total_ai_inserted_chars: u64,
    pub total_net_delta_chars: i64,
    pub total_active_seconds: u64,
    pub total_sessions: u32,
    pub days_count: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatsRecordDto {
    pub project_id: String,
    pub human_typed_chars: u64,
    pub pasted_chars: u64,
    pub deleted_chars: u64,
    pub ai_inserted_chars: u64,
    pub net_delta_chars: i64,
    pub active_seconds: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatsSummaryDto {
    pub range: DateRangeDto,
    pub projects: Vec<ProjectStatsRecordDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterStatsRecordDto {
    pub chapter_id: String,
    pub human_typed_chars: u64,
    pub pasted_chars: u64,
    pub deleted_chars: u64,
    pub ai_inserted_chars: u64,
    pub net_delta_chars: i64,
    pub active_seconds: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterStatsSummaryDto {
    pub range: DateRangeDto,
    pub chapters: Vec<ChapterStatsRecordDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatsRecordDto {
    pub device_id: String,
    pub platform: PlatformDto,
    pub human_typed_chars: u64,
    pub pasted_chars: u64,
    pub deleted_chars: u64,
    pub ai_inserted_chars: u64,
    pub net_delta_chars: i64,
    pub active_seconds: u64,
    pub sessions_count: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatsSummaryDto {
    pub range: DateRangeDto,
    pub devices: Vec<DeviceStatsRecordDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeedCurvePointDto {
    pub start_ms: i64,
    pub end_ms: i64,
    pub chars_typed: u32,
    pub chars_per_minute: f32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeedCurveSummaryDto {
    pub range: DateRangeDto,
    pub bucket_minutes: u32,
    pub buckets: Vec<SpeedCurvePointDto>,
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum MindMapNodeKindDto {
    Project, Volume, Chapter, TextAnchor, Character, Event, Location, Item, Concept, Theme, Note, Organization, Timeline, Plot, Foreshadowing, Custom,
}

impl From<crate::mind_map::MindMapNodeKind> for MindMapNodeKindDto {
    fn from(k: crate::mind_map::MindMapNodeKind) -> Self {
        match k {
            crate::mind_map::MindMapNodeKind::Project => Self::Project,
            crate::mind_map::MindMapNodeKind::Volume => Self::Volume,
            crate::mind_map::MindMapNodeKind::Chapter => Self::Chapter,
            crate::mind_map::MindMapNodeKind::TextAnchor => Self::TextAnchor,
            crate::mind_map::MindMapNodeKind::Character => Self::Character,
            crate::mind_map::MindMapNodeKind::Event => Self::Event,
            crate::mind_map::MindMapNodeKind::Location => Self::Location,
            crate::mind_map::MindMapNodeKind::Item => Self::Item,
            crate::mind_map::MindMapNodeKind::Concept => Self::Concept,
            crate::mind_map::MindMapNodeKind::Theme => Self::Theme,
            crate::mind_map::MindMapNodeKind::Note => Self::Note,
            crate::mind_map::MindMapNodeKind::Organization => Self::Organization,
            crate::mind_map::MindMapNodeKind::Timeline => Self::Timeline,
            crate::mind_map::MindMapNodeKind::Plot => Self::Plot,
            crate::mind_map::MindMapNodeKind::Foreshadowing => Self::Foreshadowing,
            crate::mind_map::MindMapNodeKind::Custom => Self::Custom,
        }
    }
}

impl From<MindMapNodeKindDto> for crate::mind_map::MindMapNodeKind {
    fn from(dto: MindMapNodeKindDto) -> Self {
        match dto {
            MindMapNodeKindDto::Project => Self::Project,
            MindMapNodeKindDto::Volume => Self::Volume,
            MindMapNodeKindDto::Chapter => Self::Chapter,
            MindMapNodeKindDto::TextAnchor => Self::TextAnchor,
            MindMapNodeKindDto::Character => Self::Character,
            MindMapNodeKindDto::Event => Self::Event,
            MindMapNodeKindDto::Location => Self::Location,
            MindMapNodeKindDto::Item => Self::Item,
            MindMapNodeKindDto::Concept => Self::Concept,
            MindMapNodeKindDto::Theme => Self::Theme,
            MindMapNodeKindDto::Note => Self::Note,
            MindMapNodeKindDto::Organization => Self::Organization,
            MindMapNodeKindDto::Timeline => Self::Timeline,
            MindMapNodeKindDto::Plot => Self::Plot,
            MindMapNodeKindDto::Foreshadowing => Self::Foreshadowing,
            MindMapNodeKindDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum MindMapEdgeKindDto {
    Contains, References, AppearsIn, Causes, RelatedTo, LocatedAt, CharacterRelation, Timeline, Foreshadows, Resolves, DependsOn, ConflictsWith, Custom,
}

impl From<crate::mind_map::MindMapEdgeKind> for MindMapEdgeKindDto {
    fn from(k: crate::mind_map::MindMapEdgeKind) -> Self {
        match k {
            crate::mind_map::MindMapEdgeKind::Contains => Self::Contains,
            crate::mind_map::MindMapEdgeKind::References => Self::References,
            crate::mind_map::MindMapEdgeKind::AppearsIn => Self::AppearsIn,
            crate::mind_map::MindMapEdgeKind::Causes => Self::Causes,
            crate::mind_map::MindMapEdgeKind::RelatedTo => Self::RelatedTo,
            crate::mind_map::MindMapEdgeKind::LocatedAt => Self::LocatedAt,
            crate::mind_map::MindMapEdgeKind::CharacterRelation => Self::CharacterRelation,
            crate::mind_map::MindMapEdgeKind::Timeline => Self::Timeline,
            crate::mind_map::MindMapEdgeKind::Foreshadows => Self::Foreshadows,
            crate::mind_map::MindMapEdgeKind::Resolves => Self::Resolves,
            crate::mind_map::MindMapEdgeKind::DependsOn => Self::DependsOn,
            crate::mind_map::MindMapEdgeKind::ConflictsWith => Self::ConflictsWith,
            crate::mind_map::MindMapEdgeKind::Custom => Self::Custom,
        }
    }
}

impl From<MindMapEdgeKindDto> for crate::mind_map::MindMapEdgeKind {
    fn from(dto: MindMapEdgeKindDto) -> Self {
        match dto {
            MindMapEdgeKindDto::Contains => Self::Contains,
            MindMapEdgeKindDto::References => Self::References,
            MindMapEdgeKindDto::AppearsIn => Self::AppearsIn,
            MindMapEdgeKindDto::Causes => Self::Causes,
            MindMapEdgeKindDto::RelatedTo => Self::RelatedTo,
            MindMapEdgeKindDto::LocatedAt => Self::LocatedAt,
            MindMapEdgeKindDto::CharacterRelation => Self::CharacterRelation,
            MindMapEdgeKindDto::Timeline => Self::Timeline,
            MindMapEdgeKindDto::Foreshadows => Self::Foreshadows,
            MindMapEdgeKindDto::Resolves => Self::Resolves,
            MindMapEdgeKindDto::DependsOn => Self::DependsOn,
            MindMapEdgeKindDto::ConflictsWith => Self::ConflictsWith,
            MindMapEdgeKindDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum MindMapLayoutKindDto {
    AutoRadial, HorizontalTree, Freeform, Timeline, Relationship, Custom,
}

impl From<crate::mind_map::LayoutKind> for MindMapLayoutKindDto {
    fn from(k: crate::mind_map::LayoutKind) -> Self {
        match k {
            crate::mind_map::LayoutKind::AutoRadial => Self::AutoRadial,
            crate::mind_map::LayoutKind::HorizontalTree => Self::HorizontalTree,
            crate::mind_map::LayoutKind::Freeform => Self::Freeform,
            crate::mind_map::LayoutKind::Timeline => Self::Timeline,
            crate::mind_map::LayoutKind::Relationship => Self::Relationship,
            crate::mind_map::LayoutKind::Custom => Self::Custom,
        }
    }
}

impl From<MindMapLayoutKindDto> for crate::mind_map::LayoutKind {
    fn from(dto: MindMapLayoutKindDto) -> Self {
        match dto {
            MindMapLayoutKindDto::AutoRadial => Self::AutoRadial,
            MindMapLayoutKindDto::HorizontalTree => Self::HorizontalTree,
            MindMapLayoutKindDto::Freeform => Self::Freeform,
            MindMapLayoutKindDto::Timeline => Self::Timeline,
            MindMapLayoutKindDto::Relationship => Self::Relationship,
            MindMapLayoutKindDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StarMapNodeKindDto {
    Character, Event, Location, Item, Concept, Theme, Note, Organization, Timeline, Plot, Foreshadowing, Chapter, Custom,
}

impl From<crate::starmap::types::StarMapNodeKind> for StarMapNodeKindDto {
    fn from(k: crate::starmap::types::StarMapNodeKind) -> Self {
        match k {
            crate::starmap::types::StarMapNodeKind::Character => Self::Character,
            crate::starmap::types::StarMapNodeKind::Event => Self::Event,
            crate::starmap::types::StarMapNodeKind::Location => Self::Location,
            crate::starmap::types::StarMapNodeKind::Item => Self::Item,
            crate::starmap::types::StarMapNodeKind::Concept => Self::Concept,
            crate::starmap::types::StarMapNodeKind::Theme => Self::Theme,
            crate::starmap::types::StarMapNodeKind::Note => Self::Note,
            crate::starmap::types::StarMapNodeKind::Organization => Self::Organization,
            crate::starmap::types::StarMapNodeKind::Timeline => Self::Timeline,
            crate::starmap::types::StarMapNodeKind::Plot => Self::Plot,
            crate::starmap::types::StarMapNodeKind::Foreshadowing => Self::Foreshadowing,
            crate::starmap::types::StarMapNodeKind::Chapter => Self::Chapter,
            crate::starmap::types::StarMapNodeKind::Custom => Self::Custom,
        }
    }
}

impl From<StarMapNodeKindDto> for crate::starmap::types::StarMapNodeKind {
    fn from(dto: StarMapNodeKindDto) -> Self {
        match dto {
            StarMapNodeKindDto::Character => Self::Character,
            StarMapNodeKindDto::Event => Self::Event,
            StarMapNodeKindDto::Location => Self::Location,
            StarMapNodeKindDto::Item => Self::Item,
            StarMapNodeKindDto::Concept => Self::Concept,
            StarMapNodeKindDto::Theme => Self::Theme,
            StarMapNodeKindDto::Note => Self::Note,
            StarMapNodeKindDto::Organization => Self::Organization,
            StarMapNodeKindDto::Timeline => Self::Timeline,
            StarMapNodeKindDto::Plot => Self::Plot,
            StarMapNodeKindDto::Foreshadowing => Self::Foreshadowing,
            StarMapNodeKindDto::Chapter => Self::Chapter,
            StarMapNodeKindDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StarMapEdgeKindDto {
    Contains, References, AppearsIn, Causes, RelatedTo, LocatedAt, CharacterRelation, Timeline, Foreshadows, Resolves, DependsOn, ConflictsWith, Custom,
}

impl From<crate::starmap::types::StarMapEdgeKind> for StarMapEdgeKindDto {
    fn from(k: crate::starmap::types::StarMapEdgeKind) -> Self {
        match k {
            crate::starmap::types::StarMapEdgeKind::Contains => Self::Contains,
            crate::starmap::types::StarMapEdgeKind::References => Self::References,
            crate::starmap::types::StarMapEdgeKind::AppearsIn => Self::AppearsIn,
            crate::starmap::types::StarMapEdgeKind::Causes => Self::Causes,
            crate::starmap::types::StarMapEdgeKind::RelatedTo => Self::RelatedTo,
            crate::starmap::types::StarMapEdgeKind::LocatedAt => Self::LocatedAt,
            crate::starmap::types::StarMapEdgeKind::CharacterRelation => Self::CharacterRelation,
            crate::starmap::types::StarMapEdgeKind::Timeline => Self::Timeline,
            crate::starmap::types::StarMapEdgeKind::Foreshadows => Self::Foreshadows,
            crate::starmap::types::StarMapEdgeKind::Resolves => Self::Resolves,
            crate::starmap::types::StarMapEdgeKind::DependsOn => Self::DependsOn,
            crate::starmap::types::StarMapEdgeKind::ConflictsWith => Self::ConflictsWith,
            crate::starmap::types::StarMapEdgeKind::Custom => Self::Custom,
        }
    }
}

impl From<StarMapEdgeKindDto> for crate::starmap::types::StarMapEdgeKind {
    fn from(dto: StarMapEdgeKindDto) -> Self {
        match dto {
            StarMapEdgeKindDto::Contains => Self::Contains,
            StarMapEdgeKindDto::References => Self::References,
            StarMapEdgeKindDto::AppearsIn => Self::AppearsIn,
            StarMapEdgeKindDto::Causes => Self::Causes,
            StarMapEdgeKindDto::RelatedTo => Self::RelatedTo,
            StarMapEdgeKindDto::LocatedAt => Self::LocatedAt,
            StarMapEdgeKindDto::CharacterRelation => Self::CharacterRelation,
            StarMapEdgeKindDto::Timeline => Self::Timeline,
            StarMapEdgeKindDto::Foreshadows => Self::Foreshadows,
            StarMapEdgeKindDto::Resolves => Self::Resolves,
            StarMapEdgeKindDto::DependsOn => Self::DependsOn,
            StarMapEdgeKindDto::ConflictsWith => Self::ConflictsWith,
            StarMapEdgeKindDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StarMapLayoutKindDto {
    Freeform, AutoRadial, Custom,
}

impl From<crate::starmap::types::StarMapLayoutKind> for StarMapLayoutKindDto {
    fn from(k: crate::starmap::types::StarMapLayoutKind) -> Self {
        match k {
            crate::starmap::types::StarMapLayoutKind::Freeform => Self::Freeform,
            crate::starmap::types::StarMapLayoutKind::AutoRadial => Self::AutoRadial,
            crate::starmap::types::StarMapLayoutKind::Custom => Self::Custom,
        }
    }
}

impl From<StarMapLayoutKindDto> for crate::starmap::types::StarMapLayoutKind {
    fn from(dto: StarMapLayoutKindDto) -> Self {
        match dto {
            StarMapLayoutKindDto::Freeform => Self::Freeform,
            StarMapLayoutKindDto::AutoRadial => Self::AutoRadial,
            StarMapLayoutKindDto::Custom => Self::Custom,
        }
    }
}

// Flattened struct (was tagged enum StarMapAnchorTargetDto)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapAnchorTargetDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub project_id: Option<String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub range_start: Option<u32>,
    pub range_end: Option<u32>,
    pub entity_id: Option<String>,
    pub entity_type: Option<String>,
    pub starmap_id: Option<String>,
    pub uri: Option<String>,
    pub payload: Option<String>,
}

impl From<crate::starmap::semantic::StarMapAnchorTarget> for StarMapAnchorTargetDto {
    fn from(t: crate::starmap::semantic::StarMapAnchorTarget) -> Self {
        match t {
            crate::starmap::semantic::StarMapAnchorTarget::ChapterRange { project_id, volume_id, chapter_id, range_start, range_end } => Self {
                kind: "chapterRange".to_string(),
                project_id,
                volume_id,
                chapter_id: Some(chapter_id),
                range_start,
                range_end,
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Project { project_id } => Self {
                kind: "project".to_string(),
                project_id: Some(project_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Volume { project_id, volume_id } => Self {
                kind: "volume".to_string(),
                project_id,
                volume_id: Some(volume_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Chapter { project_id, volume_id, chapter_id } => Self {
                kind: "chapter".to_string(),
                project_id,
                volume_id,
                chapter_id: Some(chapter_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Character { entity_id } => Self {
                kind: "character".to_string(),
                entity_id: Some(entity_id),
                entity_type: Some("character".to_string()),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Item { entity_id } => Self {
                kind: "item".to_string(),
                entity_id: Some(entity_id),
                entity_type: Some("item".to_string()),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Location { entity_id } => Self {
                kind: "location".to_string(),
                entity_id: Some(entity_id),
                entity_type: Some("location".to_string()),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Event { entity_id } => Self {
                kind: "event".to_string(),
                entity_id: Some(entity_id),
                entity_type: Some("event".to_string()),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Starmap { starmap_id } => Self {
                kind: "starmap".to_string(),
                starmap_id: Some(starmap_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::External { uri } => Self {
                kind: "external".to_string(),
                uri: Some(uri),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Custom { payload } => Self {
                kind: "custom".to_string(),
                payload: Some(serde_json::to_string(&payload).unwrap_or_default()),
                ..Default::default()
            },
        }
    }
}

impl From<StarMapAnchorTargetDto> for crate::starmap::semantic::StarMapAnchorTarget {
    fn from(d: StarMapAnchorTargetDto) -> Self {
        match d.kind.as_str() {
            "project" => Self::Project {
                project_id: d.project_id.unwrap_or_default(),
            },
            "volume" => Self::Volume {
                project_id: d.project_id,
                volume_id: d.volume_id.unwrap_or_default(),
            },
            "chapter" => Self::Chapter {
                project_id: d.project_id,
                volume_id: d.volume_id,
                chapter_id: d.chapter_id.unwrap_or_default(),
            },
            "character" => Self::Character {
                entity_id: d.entity_id.unwrap_or_default(),
            },
            "item" => Self::Item {
                entity_id: d.entity_id.unwrap_or_default(),
            },
            "location" => Self::Location {
                entity_id: d.entity_id.unwrap_or_default(),
            },
            "event" => Self::Event {
                entity_id: d.entity_id.unwrap_or_default(),
            },
            "starmap" => Self::Starmap {
                starmap_id: d.starmap_id.unwrap_or_default(),
            },
            "external" => Self::External {
                uri: d.uri.unwrap_or_default(),
            },
            "custom" => Self::Custom {
                payload: d.payload.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)).unwrap_or(serde_json::Value::Null),
            },
            _ => Self::ChapterRange {
                project_id: d.project_id,
                volume_id: d.volume_id,
                chapter_id: d.chapter_id.unwrap_or_default(),
                range_start: d.range_start,
                range_end: d.range_end,
            },
        }
    }
}

impl Default for StarMapAnchorTargetDto {
    fn default() -> Self {
        Self {
            kind: "chapterRange".to_string(),
            project_id: None,
            volume_id: None,
            chapter_id: None,
            range_start: None,
            range_end: None,
            entity_id: None,
            entity_type: None,
            starmap_id: None,
            uri: None,
            payload: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapAnchorRoleDto {
    #[default]
    Source, Destination, Reference, Custom,
}

impl From<crate::starmap::semantic::StarMapAnchorRole> for StarMapAnchorRoleDto {
    fn from(r: crate::starmap::semantic::StarMapAnchorRole) -> Self {
        match r {
            crate::starmap::semantic::StarMapAnchorRole::Source => Self::Source,
            crate::starmap::semantic::StarMapAnchorRole::Destination => Self::Destination,
            crate::starmap::semantic::StarMapAnchorRole::Reference => Self::Reference,
            crate::starmap::semantic::StarMapAnchorRole::Custom => Self::Custom,
        }
    }
}

impl From<StarMapAnchorRoleDto> for crate::starmap::semantic::StarMapAnchorRole {
    fn from(dto: StarMapAnchorRoleDto) -> Self {
        match dto {
            StarMapAnchorRoleDto::Source => Self::Source,
            StarMapAnchorRoleDto::Destination => Self::Destination,
            StarMapAnchorRoleDto::Reference => Self::Reference,
            StarMapAnchorRoleDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapPortalModeDto {
    #[default]
    EnterChild, PreviewInline, ReferenceOnly,
}

impl From<crate::starmap::semantic::StarMapPortalMode> for StarMapPortalModeDto {
    fn from(m: crate::starmap::semantic::StarMapPortalMode) -> Self {
        match m {
            crate::starmap::semantic::StarMapPortalMode::EnterChild => Self::EnterChild,
            crate::starmap::semantic::StarMapPortalMode::PreviewInline => Self::PreviewInline,
            crate::starmap::semantic::StarMapPortalMode::ReferenceOnly => Self::ReferenceOnly,
        }
    }
}

impl From<StarMapPortalModeDto> for crate::starmap::semantic::StarMapPortalMode {
    fn from(dto: StarMapPortalModeDto) -> Self {
        match dto {
            StarMapPortalModeDto::EnterChild => Self::EnterChild,
            StarMapPortalModeDto::PreviewInline => Self::PreviewInline,
            StarMapPortalModeDto::ReferenceOnly => Self::ReferenceOnly,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapPortalPreviewPolicyDto {
    #[default]
    Auto, Always, Never,
}

impl From<crate::starmap::semantic::StarMapPortalPreviewPolicy> for StarMapPortalPreviewPolicyDto {
    fn from(p: crate::starmap::semantic::StarMapPortalPreviewPolicy) -> Self {
        match p {
            crate::starmap::semantic::StarMapPortalPreviewPolicy::Auto => Self::Auto,
            crate::starmap::semantic::StarMapPortalPreviewPolicy::Always => Self::Always,
            crate::starmap::semantic::StarMapPortalPreviewPolicy::Never => Self::Never,
        }
    }
}

impl From<StarMapPortalPreviewPolicyDto> for crate::starmap::semantic::StarMapPortalPreviewPolicy {
    fn from(dto: StarMapPortalPreviewPolicyDto) -> Self {
        match dto {
            StarMapPortalPreviewPolicyDto::Auto => Self::Auto,
            StarMapPortalPreviewPolicyDto::Always => Self::Always,
            StarMapPortalPreviewPolicyDto::Never => Self::Never,
        }
    }
}

// Flattened struct (was tagged enum StarMapTargetDetailDto)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapTargetDetailDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub node_id: Option<String>,
    pub anchor_id: Option<String>,
    pub project_id: Option<String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub range_start: Option<u32>,
    pub range_end: Option<u32>,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub uri: Option<String>,
}

impl From<crate::starmap::semantic::StarMapTargetDetail> for StarMapTargetDetailDto {
    fn from(d: crate::starmap::semantic::StarMapTargetDetail) -> Self {
        match d {
            crate::starmap::semantic::StarMapTargetDetail::Starmap => Self {
                kind: "starmap".to_string(),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => Self {
                kind: "node".to_string(),
                node_id: Some(node_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, anchor_id } => Self {
                kind: "anchor".to_string(),
                node_id: Some(node_id),
                anchor_id: Some(anchor_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapTargetDetail::ChapterRange { project_id, volume_id, chapter_id, range_start, range_end } => Self {
                kind: "chapterRange".to_string(),
                project_id,
                volume_id,
                chapter_id: Some(chapter_id),
                range_start,
                range_end,
                ..Default::default()
            },
            crate::starmap::semantic::StarMapTargetDetail::Entity { entity_type, entity_id } => Self {
                kind: "entity".to_string(),
                entity_type: Some(entity_type),
                entity_id: Some(entity_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapTargetDetail::External { uri } => Self {
                kind: "external".to_string(),
                uri: Some(uri),
                ..Default::default()
            },
        }
    }
}

impl From<StarMapTargetDetailDto> for crate::starmap::semantic::StarMapTargetDetail {
    fn from(d: StarMapTargetDetailDto) -> Self {
        match d.kind.as_str() {
            "node" => Self::Node {
                node_id: d.node_id.unwrap_or_default(),
            },
            "anchor" => Self::Anchor {
                node_id: d.node_id.unwrap_or_default(),
                anchor_id: d.anchor_id.unwrap_or_default(),
            },
            "chapterRange" => Self::ChapterRange {
                project_id: d.project_id,
                volume_id: d.volume_id,
                chapter_id: d.chapter_id.unwrap_or_default(),
                range_start: d.range_start,
                range_end: d.range_end,
            },
            "entity" => Self::Entity {
                entity_type: d.entity_type.unwrap_or_default(),
                entity_id: d.entity_id.unwrap_or_default(),
            },
            "external" => Self::External {
                uri: d.uri.unwrap_or_default(),
            },
            _ => Self::Starmap,
        }
    }
}

impl Default for StarMapTargetDetailDto {
    fn default() -> Self {
        Self {
            kind: "starmap".to_string(),
            node_id: None,
            anchor_id: None,
            project_id: None,
            volume_id: None,
            chapter_id: None,
            range_start: None,
            range_end: None,
            entity_type: None,
            entity_id: None,
            uri: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StarMapSourceKindDto {
    Human, Import, Plugin, Ai, System, Unknown,
}

impl Default for StarMapSourceKindDto {
    fn default() -> Self {
        Self::Unknown
    }
}

impl From<crate::starmap::semantic::StarMapSourceKind> for StarMapSourceKindDto {
    fn from(k: crate::starmap::semantic::StarMapSourceKind) -> Self {
        match k {
            crate::starmap::semantic::StarMapSourceKind::Human => Self::Human,
            crate::starmap::semantic::StarMapSourceKind::Import => Self::Import,
            crate::starmap::semantic::StarMapSourceKind::Plugin => Self::Plugin,
            crate::starmap::semantic::StarMapSourceKind::Ai => Self::Ai,
            crate::starmap::semantic::StarMapSourceKind::System => Self::System,
            crate::starmap::semantic::StarMapSourceKind::Unknown => Self::Unknown,
        }
    }
}

impl From<StarMapSourceKindDto> for crate::starmap::semantic::StarMapSourceKind {
    fn from(dto: StarMapSourceKindDto) -> Self {
        match dto {
            StarMapSourceKindDto::Human => Self::Human,
            StarMapSourceKindDto::Import => Self::Import,
            StarMapSourceKindDto::Plugin => Self::Plugin,
            StarMapSourceKindDto::Ai => Self::Ai,
            StarMapSourceKindDto::System => Self::System,
            StarMapSourceKindDto::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StarMapReviewStatusDto {
    Accepted, Draft, NeedsReview, Rejected, Unknown,
}

impl Default for StarMapReviewStatusDto {
    fn default() -> Self {
        Self::Unknown
    }
}

impl From<crate::starmap::semantic::StarMapReviewStatus> for StarMapReviewStatusDto {
    fn from(s: crate::starmap::semantic::StarMapReviewStatus) -> Self {
        match s {
            crate::starmap::semantic::StarMapReviewStatus::Accepted => Self::Accepted,
            crate::starmap::semantic::StarMapReviewStatus::Draft => Self::Draft,
            crate::starmap::semantic::StarMapReviewStatus::NeedsReview => Self::NeedsReview,
            crate::starmap::semantic::StarMapReviewStatus::Rejected => Self::Rejected,
            crate::starmap::semantic::StarMapReviewStatus::Unknown => Self::Unknown,
        }
    }
}

impl From<StarMapReviewStatusDto> for crate::starmap::semantic::StarMapReviewStatus {
    fn from(dto: StarMapReviewStatusDto) -> Self {
        match dto {
            StarMapReviewStatusDto::Accepted => Self::Accepted,
            StarMapReviewStatusDto::Draft => Self::Draft,
            StarMapReviewStatusDto::NeedsReview => Self::NeedsReview,
            StarMapReviewStatusDto::Rejected => Self::Rejected,
            StarMapReviewStatusDto::Unknown => Self::Unknown,
        }
    }
}

// Flattened struct (was tagged enum StarMapEndpointDto)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEndpointDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub node_id: Option<String>,
    pub anchor_id: Option<String>,
}

impl From<crate::starmap::types::StarMapEndpoint> for StarMapEndpointDto {
    fn from(e: crate::starmap::types::StarMapEndpoint) -> Self {
        match e {
            crate::starmap::types::StarMapEndpoint::Node { node_id } => Self {
                kind: "node".to_string(),
                node_id: Some(node_id),
                anchor_id: None,
            },
            crate::starmap::types::StarMapEndpoint::Anchor { node_id, anchor_id } => Self {
                kind: "anchor".to_string(),
                node_id: Some(node_id),
                anchor_id: Some(anchor_id),
            },
            crate::starmap::types::StarMapEndpoint::Starmap => Self {
                kind: "starmap".to_string(),
                node_id: None,
                anchor_id: None,
            },
        }
    }
}

impl From<StarMapEndpointDto> for crate::starmap::types::StarMapEndpoint {
    fn from(dto: StarMapEndpointDto) -> Self {
        match dto.kind.as_str() {
            "anchor" => Self::Anchor {
                node_id: dto.node_id.unwrap_or_default(),
                anchor_id: dto.anchor_id.unwrap_or_default(),
            },
            "starmap" => Self::Starmap,
            _ => Self::Node {
                node_id: dto.node_id.unwrap_or_default(),
            },
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapViewportDto {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<crate::starmap::types::StarMapViewport> for StarMapViewportDto {
    fn from(v: crate::starmap::types::StarMapViewport) -> Self {
        Self {
            scale: v.scale,
            offset_x: v.offset_x,
            offset_y: v.offset_y,
            width: v.width,
            height: v.height,
        }
    }
}

impl From<StarMapViewportDto> for crate::starmap::types::StarMapViewport {
    fn from(dto: StarMapViewportDto) -> Self {
        Self {
            scale: dto.scale,
            offset_x: dto.offset_x,
            offset_y: dto.offset_y,
            width: dto.width,
            height: dto.height,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ActionKindDto {
    Query, Preview, Mutation,
}

impl From<crate::action_registry::ActionKind> for ActionKindDto {
    fn from(k: crate::action_registry::ActionKind) -> Self {
        match k {
            crate::action_registry::ActionKind::Query => Self::Query,
            crate::action_registry::ActionKind::Preview => Self::Preview,
            crate::action_registry::ActionKind::Mutation => Self::Mutation,
        }
    }
}

impl From<ActionKindDto> for crate::action_registry::ActionKind {
    fn from(dto: ActionKindDto) -> Self {
        match dto {
            ActionKindDto::Query => Self::Query,
            ActionKindDto::Preview => Self::Preview,
            ActionKindDto::Mutation => Self::Mutation,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ActionRiskLevelDto {
    SafeRead, SafeWrite, ContentWrite, Dangerous,
}

impl From<crate::action_registry::ActionRiskLevel> for ActionRiskLevelDto {
    fn from(r: crate::action_registry::ActionRiskLevel) -> Self {
        match r {
            crate::action_registry::ActionRiskLevel::SafeRead => Self::SafeRead,
            crate::action_registry::ActionRiskLevel::SafeWrite => Self::SafeWrite,
            crate::action_registry::ActionRiskLevel::ContentWrite => Self::ContentWrite,
            crate::action_registry::ActionRiskLevel::Dangerous => Self::Dangerous,
        }
    }
}

impl From<ActionRiskLevelDto> for crate::action_registry::ActionRiskLevel {
    fn from(dto: ActionRiskLevelDto) -> Self {
        match dto {
            ActionRiskLevelDto::SafeRead => Self::SafeRead,
            ActionRiskLevelDto::SafeWrite => Self::SafeWrite,
            ActionRiskLevelDto::ContentWrite => Self::ContentWrite,
            ActionRiskLevelDto::Dangerous => Self::Dangerous,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionDescriptorDto {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub kind: ActionKindDto,
    pub risk_level: ActionRiskLevelDto,
    pub confirm_required: bool,
    pub undoable: bool,
    pub platforms: Vec<String>,
    pub input_schema: Option<String>,
    pub ui_schema: Option<String>,
}

impl From<crate::action_registry::ActionDescriptor> for ActionDescriptorDto {
    fn from(d: crate::action_registry::ActionDescriptor) -> Self {
        Self {
            id: d.id,
            title: d.title,
            description: d.description,
            category: d.category,
            kind: d.kind.into(),
            risk_level: d.risk_level.into(),
            confirm_required: d.confirm_required,
            undoable: d.undoable,
            platforms: d.platforms,
            input_schema: d.input_schema.map(|v| serde_json::to_string(&v).unwrap_or_default()),
            ui_schema: d.ui_schema.map(|v| serde_json::to_string(&v).unwrap_or_default()),
        }
    }
}

impl From<ActionDescriptorDto> for crate::action_registry::ActionDescriptor {
    fn from(dto: ActionDescriptorDto) -> Self {
        Self {
            id: dto.id,
            title: dto.title,
            description: dto.description,
            category: dto.category,
            kind: dto.kind.into(),
            risk_level: dto.risk_level.into(),
            confirm_required: dto.confirm_required,
            undoable: dto.undoable,
            platforms: dto.platforms,
            input_schema: dto.input_schema.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            ui_schema: dto.ui_schema.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionResultDto {
    pub success: bool,
    pub message: Option<String>,
    pub data: Option<String>,
    pub proposed_ui: Option<String>,
    pub requires_confirmation: Option<bool>,
}

impl From<crate::action_registry::ActionResult> for ActionResultDto {
    fn from(r: crate::action_registry::ActionResult) -> Self {
        Self {
            success: r.success,
            message: r.message,
            data: r.data.map(|v| serde_json::to_string(&v).unwrap_or_default()),
            proposed_ui: r.proposed_ui.map(|v| serde_json::to_string(&v).unwrap_or_default()),
            requires_confirmation: r.requires_confirmation,
        }
    }
}

impl From<ActionResultDto> for crate::action_registry::ActionResult {
    fn from(dto: ActionResultDto) -> Self {
        Self {
            success: dto.success,
            message: dto.message,
            data: dto.data.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            proposed_ui: dto.proposed_ui.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            requires_confirmation: dto.requires_confirmation,
        }
    }
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PlatformDto {
    #[default]
    Linux,
    Android,
}

impl From<crate::writing_stats::Platform> for PlatformDto {
    fn from(p: crate::writing_stats::Platform) -> Self {
        match p {
            crate::writing_stats::Platform::Linux => Self::Linux,
            crate::writing_stats::Platform::Android => Self::Android,
        }
    }
}

impl From<PlatformDto> for crate::writing_stats::Platform {
    fn from(dto: PlatformDto) -> Self {
        match dto {
            PlatformDto::Linux => Self::Linux,
            PlatformDto::Android => Self::Android,
        }
    }
}
