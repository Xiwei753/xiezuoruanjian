use super::service::{ApiResult, WriterCoreApi};
use super::types::*;
use crate::sync::cancellation_token::{SyncCancellationToken, SyncProgressSink};
use sha2::{Digest, Sha256};
use std::time::Instant;

fn record_sync_diagnostic(
    event: &str,
    target: &str,
    fields: std::collections::BTreeMap<String, serde_json::Value>,
) {
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: if event.starts_with("sync.object.")
            || event.starts_with("sync.target.")
            || (target == "sync.target" && event.starts_with("sync.phase."))
        {
            writer_diagnostics::DiagnosticLevel::Debug
        } else {
            writer_diagnostics::DiagnosticLevel::Info
        },
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: event.to_string(),
        target: target.to_string(),
        message: None,
        fields,
    });
}

fn sync_diagnostic_fields(
    operation_id: &str,
    phase: &str,
    target_digest: Option<&str>,
) -> std::collections::BTreeMap<String, serde_json::Value> {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(
        "operation_id".to_string(),
        serde_json::Value::String(operation_id.to_string()),
    );
    fields.insert(
        "phase".to_string(),
        serde_json::Value::String(phase.to_string()),
    );
    if let Some(target_digest) = target_digest {
        fields.insert(
            "target_digest".to_string(),
            serde_json::Value::String(target_digest.to_string()),
        );
    }
    fields
}

fn sync_error_category(error: &crate::api::error::WriterError) -> &str {
    match error {
        crate::api::error::WriterError::Io(_) | crate::api::error::WriterError::RetryableIo(_) => {
            "local_io"
        }
        crate::api::error::WriterError::Json(_) => "local_decode",
        crate::api::error::WriterError::RetryableNetwork(_) => "network",
        crate::api::error::WriterError::Authentication(_) => "authentication",
        crate::api::error::WriterError::SyncConflict(_)
        | crate::api::error::WriterError::Conflict(_) => "conflict",
        crate::api::error::WriterError::NotImplemented => "unsupported",
        crate::api::error::WriterError::SyncFailure { category, .. } => category,
        _ => "sync_error",
    }
}

fn core_sync_category(error: &crate::Error) -> String {
    let category = error.sync_category();
    if category.is_empty() {
        "other".to_string()
    } else {
        category.to_string()
    }
}

/// Provider 初始化错误在保留真实阶段/分类后，仍需保持网络类错误原有的
/// REST 后备重试语义；证书、CA、本地缓存等错误不进入此分支。
fn writer_error_is_retryable_network(error: &crate::api::error::WriterError) -> bool {
    use crate::api::error::WriterError;
    match error {
        WriterError::RetryableNetwork(_) => true,
        WriterError::SyncFailure { category, .. } => matches!(
            category.as_str(),
            "network" | "rate_limited" | "temporary_unavailable"
        ),
        _ => false,
    }
}

fn record_sync_phase_failure(
    operation_id: &str,
    phase: &str,
    error: &crate::api::error::WriterError,
) {
    let mut fields = sync_diagnostic_fields(operation_id, phase, None);
    fields.insert(
        "error_code".to_string(),
        serde_json::Value::String(error.code().to_string()),
    );
    fields.insert(
        "error_category".to_string(),
        serde_json::Value::String(sync_error_category(error).to_string()),
    );
    record_sync_diagnostic("sync.phase.failure", "sync.run", fields);
}

fn sync_target_digest(target: &str) -> String {
    format!("{:x}", Sha256::digest(target.as_bytes()))
}

fn record_sync_object_result(
    operation_id: &str,
    target_digest: &str,
    object_path: &str,
    action: &str,
) {
    let mut fields = sync_diagnostic_fields(operation_id, "object_result", Some(target_digest));
    fields.insert(
        "object_path_digest".to_string(),
        serde_json::Value::String(sync_target_digest(object_path)),
    );
    fields.insert(
        "action".to_string(),
        serde_json::Value::String(action.to_string()),
    );
    record_sync_diagnostic("sync.object.result", "sync.object", fields);
}

struct SyncDiagnosticPhase {
    operation_id: String,
    phase: String,
    target_digest: Option<String>,
    started_at: Instant,
    network_metrics_start: Option<crate::sync::provider::ProviderNetworkMetrics>,
    network_metrics_delta: Option<crate::sync::provider::ProviderNetworkMetrics>,
    finished: bool,
}

impl SyncDiagnosticPhase {
    fn start(operation_id: &str, phase: &str, target_digest: Option<String>) -> Self {
        let guard = Self {
            operation_id: operation_id.to_string(),
            phase: phase.to_string(),
            target_digest,
            started_at: Instant::now(),
            network_metrics_start: None,
            network_metrics_delta: None,
            finished: false,
        };
        guard.record("sync.phase.start", None);
        guard
    }

    fn start_with_provider(
        operation_id: &str,
        phase: &str,
        target_digest: Option<String>,
        provider: &dyn crate::sync::provider::SyncProvider,
    ) -> Self {
        let mut guard = Self::start(operation_id, phase, target_digest);
        guard.network_metrics_start = Some(provider.network_metrics());
        guard
    }

    fn finish(mut self, outcome: &str) {
        self.record("sync.phase.end", Some(outcome));
        self.finished = true;
    }

    fn finish_with_provider(
        mut self,
        outcome: &str,
        provider: &dyn crate::sync::provider::SyncProvider,
    ) {
        let end = provider.network_metrics();
        let start = self.network_metrics_start.take().unwrap_or_default();
        self.network_metrics_delta = Some(crate::sync::provider::ProviderNetworkMetrics {
            request_count: end.request_count.saturating_sub(start.request_count),
            bytes_received: end.bytes_received.saturating_sub(start.bytes_received),
            bytes_sent: end.bytes_sent.saturating_sub(start.bytes_sent),
            operation_id: end.operation_id,
            metric_scope: end.metric_scope,
        });
        self.record("sync.phase.end", Some(outcome));
        self.finished = true;
    }

    fn record(&self, event: &str, outcome: Option<&str>) {
        let mut fields = sync_diagnostic_fields(
            &self.operation_id,
            &self.phase,
            self.target_digest.as_deref(),
        );
        if let Some(outcome) = outcome {
            fields.insert(
                "outcome".to_string(),
                serde_json::Value::String(outcome.to_string()),
            );
        }
        if event.ends_with(".end") {
            fields.insert(
                "duration_ms".to_string(),
                serde_json::Value::Number(serde_json::Number::from(
                    u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
                )),
            );
            if let Some(metrics) = &self.network_metrics_delta {
                fields.insert(
                    "request_count".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(metrics.request_count)),
                );
                fields.insert(
                    "bytes_received".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(metrics.bytes_received)),
                );
                fields.insert(
                    "bytes_sent".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(metrics.bytes_sent)),
                );
                fields.insert(
                    "metric_scope".to_string(),
                    serde_json::Value::String(metrics.metric_scope.clone()),
                );
            }
        }
        let target = if self.target_digest.is_some() {
            "sync.target"
        } else {
            "sync.run"
        };
        record_sync_diagnostic(event, target, fields);
    }
}

impl Drop for SyncDiagnosticPhase {
    fn drop(&mut self) {
        if !self.finished {
            self.record("sync.phase.end", Some("interrupted"));
            self.finished = true;
        }
    }
}

struct LocalSyncVersionSnapshot {
    fingerprint: String,
    target_fingerprints: std::collections::BTreeMap<String, String>,
    no_change_targets: Vec<crate::sync::types::TargetSyncResult>,
}

fn add_fingerprint_field(hasher: &mut Sha256, key: &str, value: &[u8]) {
    hasher.update((key.len() as u64).to_le_bytes());
    hasher.update(key.as_bytes());
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn add_local_objects_to_fingerprint(
    hasher: &mut Sha256,
    root_key: &str,
    entries: Vec<crate::sync::types::SyncFileEntry>,
) {
    let mut objects: Vec<_> = entries
        .into_iter()
        .filter(|entry| matches!(&entry.sync_kind, crate::sync::types::SyncKind::Upload))
        .collect();
    objects.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    for entry in objects {
        add_fingerprint_field(
            hasher,
            &format!("{root_key}:{}", entry.relative_path),
            entry.file_hash.as_bytes(),
        );
    }
}

fn add_optional_file_to_fingerprint(
    hasher: &mut Sha256,
    key: &str,
    path: &std::path::Path,
) -> crate::Result<()> {
    match std::fs::read(path) {
        Ok(bytes) => add_fingerprint_field(hasher, key, &bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            add_fingerprint_field(hasher, key, b"<absent>");
        }
        Err(error) => return Err(crate::Error::Io(error)),
    }
    Ok(())
}

/// Snapshot the current syncable objects and group state. `None` means a known
/// pending operation or migration makes the fast no-change path ineligible.
fn local_sync_version_snapshot(
    app_data_root: &std::path::Path,
    projects_root: &std::path::Path,
) -> crate::Result<Option<LocalSyncVersionSnapshot>> {
    if crate::writing_stats::migration::legacy_events_present(app_data_root)
        || app_data_root
            .join("app-meta/sync/manifest.sync.json")
            .exists()
        || app_data_root.join("app-meta/sync/sync_state.json").exists()
    {
        return Ok(None);
    }

    match crate::sync::pending_deleted::load_pending_deleted_targets(app_data_root) {
        Ok(pending) if pending.is_empty() => {}
        Ok(_) | Err(_) => return Ok(None),
    }
    match crate::sync::pending_remote_cleanup::load_pending_remote_cleanups(app_data_root) {
        Ok(pending) if pending.is_empty() => {}
        Ok(_) | Err(_) => return Ok(None),
    }

    let mut target_fingerprints = std::collections::BTreeMap::new();
    let app_groups =
        crate::sync::app_data_group_catalog::discover_local_app_data_groups(app_data_root)?;
    let mut no_change_targets = Vec::new();
    for group in app_groups {
        let state = crate::sync::SyncService::load_sync_state_read_only_for_group(
            app_data_root,
            &group,
            None,
        )?;
        if state.last_error.is_some()
            || !state.tombstones.is_empty()
            || !state.conflicted_files.is_empty()
            || !state.pending_take_remote.is_empty()
        {
            return Ok(None);
        }
        let mut target_hasher = Sha256::new();
        let app_entries = crate::sync::scanner::scan_for_sync_group(
            app_data_root,
            crate::sync::types::SyncScope::App,
            Some(&group),
        )?;
        add_local_objects_to_fingerprint(&mut target_hasher, "objects", app_entries);
        add_optional_file_to_fingerprint(
            &mut target_hasher,
            "state",
            &app_data_root.join(group.local_state_path()),
        )?;
        add_optional_file_to_fingerprint(
            &mut target_hasher,
            "manifest",
            &app_data_root.join(group.local_manifest_path()),
        )?;
        add_optional_file_to_fingerprint(
            &mut target_hasher,
            "conflicts",
            &app_data_root.join(group.local_conflicts_path()),
        )?;
        let group_id = group.id();
        target_fingerprints.insert(
            format!("app:{group_id}"),
            format!("{:x}", target_hasher.finalize()),
        );
        let target = crate::sync::types::SyncTarget::app_data_group(group);
        no_change_targets.push(crate::sync::types::TargetSyncResult {
            target_kind: "app".to_string(),
            project_id: None,
            remote_prefix: target.remote_prefix,
            group_id: target.app_group.as_ref().map(|group| group.id()),
            failure_path: None,
            failure_phase: None,
            result: crate::sync::types::SyncResult::no_changes(),
            deleted_resolution: None,
            local_lifecycle_action: crate::sync::types::LocalLifecycleCommitAction::None,
        });
    }

    let projects = crate::project::list_projects(projects_root)?;
    for project in projects {
        let project_root = projects_root.join(&project.id);
        if project_root.join("app-meta/sync/sync_state.json").exists() {
            return Ok(None);
        }
        let entries = crate::sync::scanner::scan_for_sync_group(
            &project_root,
            crate::sync::types::SyncScope::Project,
            None,
        )?;
        let mut target_hasher = Sha256::new();
        add_local_objects_to_fingerprint(&mut target_hasher, "objects", entries);
        let state = crate::sync::SyncService::load_sync_state_read_only(&project_root, None)?;
        if state.last_error.is_some()
            || !state.tombstones.is_empty()
            || !state.conflicted_files.is_empty()
            || !state.pending_take_remote.is_empty()
        {
            return Ok(None);
        }
        for (name, relative_path) in [
            ("state", "app-meta/sync/state.local.json"),
            ("manifest", "app-meta/sync/manifest.sync.json"),
            ("conflicts", "app-meta/sync/conflicts.json"),
        ] {
            add_optional_file_to_fingerprint(
                &mut target_hasher,
                name,
                &project_root.join(relative_path),
            )?;
        }
        target_fingerprints.insert(
            format!("project:{}", project.id),
            format!("{:x}", target_hasher.finalize()),
        );
        no_change_targets.push(crate::sync::types::TargetSyncResult {
            target_kind: "project".to_string(),
            project_id: Some(project.id.clone()),
            remote_prefix: format!("projects/{}", project.id),
            group_id: None,
            failure_path: None,
            failure_phase: None,
            result: crate::sync::types::SyncResult::no_changes(),
            deleted_resolution: None,
            local_lifecycle_action: crate::sync::types::LocalLifecycleCommitAction::None,
        });
    }

    let mut hasher = Sha256::new();
    for (target, fingerprint) in &target_fingerprints {
        add_fingerprint_field(&mut hasher, target, fingerprint.as_bytes());
    }
    Ok(Some(LocalSyncVersionSnapshot {
        fingerprint: format!("{:x}", hasher.finalize()),
        target_fingerprints,
        no_change_targets,
    }))
}

fn sync_target_checkpoint_key(target: &crate::sync::full_sync::PlannedTarget) -> Option<String> {
    if let Some(group) = target.target.app_group.as_ref() {
        return Some(format!("app:{}", group.id()));
    }
    target
        .project_id
        .as_ref()
        .map(|project_id| format!("project:{project_id}"))
}

fn repository_identity(config: &crate::sync::SyncConfig) -> Option<String> {
    match serde_json::to_vec(&(&config.active_provider, &config.provider_config)) {
        Ok(identity) => Some(format!("{:x}", Sha256::digest(identity))),
        Err(error) => {
            log::warn!("[sync] failed to serialize repository identity for checkpoint: {error}");
            None
        }
    }
}

/// GitHub 仓库是否还值得在完整同步失败后改用 API 重试。
///
/// 仅当配置指向官方 `github.com` HTTPS 仓库、且本轮尚未判定 Git 不可达时成立。
/// 认证/权限/证书/non-fast-forward 不会走到 `RetryableNetwork`，因此不会被重试。
fn full_sync_has_network_failure(result: &FullSyncResultDto) -> bool {
    result.error_category.as_deref() == Some("network")
        || result
            .targets
            .iter()
            .any(|target| target.result.error_category.as_deref() == Some("network"))
}

/// 远端版本在规划后变化，旧 batch 已不能安全提交。
///
/// `PreconditionFailed` 在完整同步结果中稳定映射为 `precondition_failed`。
/// 必须重新执行整轮同步（新 provider、新远端快照、重新 LWW），不能在
/// generation 层重放旧 mutations。
fn full_sync_has_precondition_failure(result: &FullSyncResultDto) -> bool {
    result.error_category.as_deref() == Some("precondition_failed")
        || result
            .targets
            .iter()
            .any(|target| target.result.error_category.as_deref() == Some("precondition_failed"))
}

fn should_retry_full_sync_via_github_api(config: &crate::sync::SyncConfig) -> bool {
    use crate::sync::provider::{router, ProviderConfig};

    if router::is_git_known_unreachable() {
        return false;
    }
    match config.provider_config.as_ref() {
        Some(ProviderConfig::GitHttps(git)) => router::is_github_https_remote(&git.remote_url),
        #[cfg(feature = "github-api")]
        Some(ProviderConfig::GitHub(github)) => router::is_github_https_remote(&github.remote_url),
        _ => false,
    }
}

/// 同步 API — 全量同步统一入口。
///
/// 一个全局 `SyncConfig` + 一份全局凭据，`perform_full_sync` 内部按 `SyncTarget`
/// 把不同本地根映射到同一个远端仓库的不同前缀。
/// 旧的"作品同步 + 应用数据同步"两套用户配置 API 已删除。
impl WriterCoreApi {
    /// 旧→新同步 profile 一次性迁移（  / D）。
    ///
    /// 详见 `crate::storage::migration`。失败时返回 `WriterError`；
    /// 冲突时返回 `NeedsReconfigure`（非 Err），由 UI 引导用户重选全局仓库。
    pub fn migrate_legacy_sync_profile(&self) -> ApiResult<LegacyMigrationOutcomeDto> {
        self.core_write()
            .migrate_legacy_sync_profile()
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 旧→新同步 profile 一次性迁移，接受精确 generation metadata。
    ///
    /// 详见 `crate::storage::migration::LegacySyncProfileMigrator::migrate_with_metadata`。
    /// 当 metadata 中某 source 有 `active_generation = Some(n)` 时，精确读取
    /// `sync_token_<base>_g{n}`；当 `active_generation = None` 时回退 base key / 文件。
    pub fn migrate_legacy_sync_profile_with_metadata(
        &self,
        metadata: Vec<LegacyProfileMetadataDto>,
    ) -> ApiResult<LegacyMigrationOutcomeDto> {
        self.core_write()
            .migrate_legacy_sync_profile_with_metadata(
                &metadata.into_iter().map(Into::into).collect::<Vec<_>>(),
            )
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 加载全局同步配置。
    pub fn load_sync_config(&self) -> ApiResult<SyncConfigDto> {
        self.core_read()
            .load_sync_config()
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 保存全局同步配置。成功返回 true。
    pub fn save_sync_config(&self, config: SyncConfigDto) -> ApiResult<bool> {
        self.core_write()
            .save_sync_config(&config.into())
            .map(|_| true)
            .map_err(crate::api::error::WriterError::from)?;
        //   sync config 是同步引擎运行状态
        // （app-meta/sync/config.local.json），不进入本地用户版本历史。
        // is_workspace_history_path 已把 SyncEngineState 排除，这里不再调
        // record_workspace_history。
        Ok(true)
    }

    /// 加载全局同步密钥（token 等）。
    /// 先查 API 层 override snapshot，
    /// 没有再短暂 core_read 从 secure storage/file 读取。
    pub fn load_sync_secrets(&self) -> ApiResult<SyncSecretsDto> {
        if let Some(secrets) = self.secrets_override_snapshot() {
            return Ok(secrets.into());
        }
        self.core_read()
            .load_sync_secrets()
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 保存全局同步密钥。成功返回 true。
    ///
    /// 凭据写入根本不是历史内容，
    /// 不调用 `record_workspace_history`。凭据路径由
    /// [`crate::storage::workspace_paths::is_workspace_secret_path`]
    /// 在底层统一排除，永不进入 history change set。
    pub fn save_sync_secrets(&self, secrets: SyncSecretsDto) -> ApiResult<bool> {
        self.core_write()
            .save_sync_secrets(&secrets.into())
            .map(|_| true)
            .map_err(crate::api::error::WriterError::from)
    }

    ///  五：设置进程级 secrets override。
    /// 直接写 API 层 Mutex，不再透传到 facade::WriterCore。
    pub fn set_sync_secrets_override(&self, secrets: SyncSecretsDto) -> ApiResult<()> {
        self.set_secrets_override(Some(secrets.into()));
        Ok(())
    }

    ///  十：清除进程级 secrets override。
    pub fn clear_sync_secrets_override(&self) -> ApiResult<()> {
        self.set_secrets_override(None);
        Ok(())
    }

    ///  五：按 generation 保存凭据到安全存储。
    pub fn save_sync_secrets_for_generation(
        &self,
        generation: u64,
        secrets: SyncSecretsDto,
    ) -> ApiResult<bool> {
        self.core_write()
            .save_sync_secrets_for_generation(generation, &secrets.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    ///  五：读取指定 generation 的安全存储凭据；缺失返回 None。
    pub fn load_sync_secrets_for_generation(
        &self,
        generation: u64,
    ) -> ApiResult<Option<SyncSecretsDto>> {
        self.core_read()
            .load_sync_secrets_for_generation(generation)
            .map(|opt| opt.map(Into::into))
            .map_err(Into::into)
    }

    ///  五：删除指定 generation 的安全存储凭据。
    pub fn delete_sync_secrets_for_generation(&self, generation: u64) -> ApiResult<()> {
        self.core_write()
            .delete_sync_secrets_for_generation(generation)
            .map_err(Into::into)
    }

    /// Project target 同步状态。
    pub fn load_sync_state(&self, project_id: &str) -> ApiResult<SyncStateDto> {
        self.core_read()
            .load_sync_state(project_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    /// App target 同步状态。
    pub fn load_app_sync_state(&self) -> ApiResult<SyncStateDto> {
        self.core_read()
            .load_app_sync_state()
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 保存 App target 同步状态。
    pub fn save_app_sync_state(&self, state: SyncStateDto) -> ApiResult<()> {
        self.core_write()
            .save_app_sync_state(&state.into())
            .map_err(crate::api::error::WriterError::from)?;
        //   App target 同步状态是同步引擎运行状态
        // （app-meta/sync/state.local.json），不进入本地用户版本历史。
        // is_workspace_history_path 已把 SyncEngineState 排除，这里不再调
        // record_workspace_history。
        Ok(())
    }

    /// 全量同步持久状态。
    ///
    /// 读取 `<app_data_root>/app-meta/sync/full_state.local.json`。
    /// 文件不存在或 JSON 损坏时返回 None，不报错。
    pub fn load_full_sync_state(&self) -> ApiResult<Option<FullSyncStateDto>> {
        self.core_read()
            .load_full_sync_state()
            .map(|opt| opt.map(Into::into))
            .map_err(Into::into)
    }

    /// Last successful generation GC time stored in this workspace's app-meta sync state.
    pub fn load_generation_gc_maintenance_last_success_at(&self) -> ApiResult<Option<i64>> {
        let core = self.core_read();
        crate::sync::generation_gc::load_maintenance_last_success_at(&core.app_data_root)
            .map_err(Into::into)
    }

    /// 最近一次持久化的完整同步报告，供各平台页面和诊断包读取。
    pub fn load_latest_full_sync_report(&self) -> ApiResult<Option<FullSyncReportDto>> {
        self.core_read()
            .load_latest_full_sync_report()
            .map(|opt| opt.map(Into::into))
            .map_err(Into::into)
    }

    /// 冷启动恢复中断的 Syncing 状态。
    ///
    /// 读取 `full_state.local.json`，只有旧状态是 `Syncing` 才原子改成
    /// `RecoverableError("previous_full_sync_interrupted")`；其它终态不动。
    /// 只能在新 Core/WriterAppService 实例启动时执行一次。
    pub fn recover_interrupted_full_sync_state(&self) -> ApiResult<bool> {
        self.core_write()
            .recover_interrupted_full_sync_state()
            .map_err(Into::into)
    }

    /// 平台预处理失败写同一份 Core FullSyncState 的窄接口。
    ///
    /// 只负责更新 `<app_data_root>/app-meta/sync/full_state.local.json`（与
    /// `perform_full_sync` 同一份），不新建平台第二份状态。覆盖 Android 正文 flush /
    /// app data barrier / credentials override 等 Core 根本没进入 full sync 的失败路径。
    ///
    /// - `status`：线格式状态码（`"fatal_error"` / `"recoverable_error"` / ...，与
    ///   `FullSyncStateDto.overall_status` 同一映射）；未知 code 视为 `FatalError`；
    /// - `failed_target`：传 `"preflight"`，不要伪造某个 project id。
    ///
    /// 保留旧 `last_success_time`，保证重启后顶部不会出现旧绿灯。
    pub fn record_full_sync_preflight_failure(
        &self,
        status: String,
        failed_target: String,
    ) -> ApiResult<()> {
        let parsed = super::types::sync_status_from_wire(&status);
        self.core_write()
            .record_full_sync_preflight_failure(parsed, &failed_target)
            .map_err(Into::into)
    }

    /// 平台预处理/Provider 初始化失败时保留 Core 原始分类与阶段。
    pub fn record_full_sync_preflight_failure_typed(
        &self,
        status: String,
        failed_target: String,
        error_category: String,
        phase: String,
        detail: String,
    ) -> ApiResult<()> {
        let parsed = super::types::sync_status_from_wire(&status);
        self.core_write()
            .record_full_sync_preflight_failure_typed(
                parsed,
                &failed_target,
                &error_category,
                &phase,
                &detail,
            )
            .map_err(Into::into)
    }

    /// 全量同步诊断 — 只测一次仓库、分支、token。
    pub fn perform_full_sync_diagnostics(
        &self,
        config: SyncConfigDto,
    ) -> ApiResult<FullSyncDiagnosticsResultDto> {
        let secrets = self.secrets_override_snapshot().unwrap_or_default();
        self.core_write()
            .perform_full_sync_diagnostics(&config.into(), &secrets)
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 全量同步 dry-run — 枚举 App target + 所有 Project target。
    ///
    ///   dry-run 网络 IO（读远端 catalog）在 core_write
    /// 锁外执行，拆三段短锁，避免阻塞正文/作品读取。
    ///   dry-run 用 core_read read-only state loader，
    /// 绝不写本地文件，绝不写远端（discover_legacy_remote_catalog 只读）。
    pub fn perform_full_sync_dry_run(
        &self,
        config: SyncConfigDto,
    ) -> ApiResult<FullSyncDryRunResultDto> {
        let sync_config: crate::sync::SyncConfig = config.into();
        let secrets = self.secrets_override_snapshot().unwrap_or_default();

        //   三段短锁 — 网络 IO 不持 core 写锁。
        // 1a. 短锁 A：创建 provider（transport 初始化可能涉及本地 IO）。
        let provider = if sync_config.enabled {
            let core = self.core_write();
            match core.create_sync_provider_for_plan(&sync_config, &secrets) {
                Ok(p) => p,
                Err(e) => {
                    //   provider 创建失败 → 返回错误，
                    // 不降级为空 catalog（dry-run 是预览，但不能返回假的远端事实）。
                    log::warn!("[sync] dry-run: create_sync_provider_for_plan failed: {e}");
                    return Err(crate::api::error::WriterError::from(e));
                }
            }
        } else {
            // sync disabled → 不做网络 IO，用空 catalog。
            //   回退问题：短锁 snapshot paths，锁外扫描。
            let (app_data_root, projects_root) = {
                let core = self.core_read();
                (core.app_data_root.clone(), core.projects_root.clone())
            };
            return crate::facade::WriterCore::build_full_sync_dry_run_unlocked(
                &app_data_root,
                &projects_root,
                &sync_config,
                &crate::sync::types::TargetLifecycleCatalog::default(),
                &[],
            )
            .map(Into::into)
            .map_err(Into::into);
        };
        // 写锁已释放。
        // 1b. 无锁：读 remote catalog（网络 IO，只读一个文件）。
        //   用 discover_legacy_remote_catalog（只读，不写远端）。
        // dry-run 绝不在远端创建 targets.sync.json。
        let remote_catalog_snapshot =
            crate::sync::target_lifecycle::discover_legacy_remote_catalog(provider.as_ref())
                .map_err(|e| {
                    log::warn!("[sync] dry-run: discover_legacy_remote_catalog failed: {e}");
                    crate::api::error::WriterError::from(e)
                })?;

        let remote_app_data_groups =
            crate::sync::app_data_group_catalog::discover_remote_app_data_groups(provider.as_ref())
                .map_err(|e| {
                    log::warn!("[sync] dry-run: discover_remote_app_data_groups failed: {e}");
                    crate::api::error::WriterError::from(e)
                })?;

        // 1c. 短锁 B（read）：snapshot paths，锁外扫描。
        //   回退问题：恢复短锁+锁外扫描。
        let (app_data_root, projects_root) = {
            let core = self.core_read();
            (core.app_data_root.clone(), core.projects_root.clone())
        };
        crate::facade::WriterCore::build_full_sync_dry_run_unlocked(
            &app_data_root,
            &projects_root,
            &sync_config,
            &remote_catalog_snapshot.catalog,
            &remote_app_data_groups,
        )
        .map(Into::into)
        .map_err(Into::into)
    }

    /// 全量同步 — Prepare（短写锁）→ Seed staging（不持锁）→ Per-target Transfer+Commit → 聚合收口。
    ///
    /// 网络阶段完全不持 Core 锁，
    /// 避免全量同步期间阻塞所有读操作。
    ///
    /// staging seed（磁盘扫描/复制）也移出写锁，
    /// 避免冷启动读取卷章被同步 Prepare 卡住。
    ///
    /// 通用 full-sync 入口不再 `#[cfg(feature = "github-api")]`
    /// 门控。具体 Provider 能否创建由 [`crate::facade::WriterCore::create_sync_provider_for_plan`]
    /// 决定（未启用 github-api feature 时 `github_api` 分支返回 `NotImplemented`）。
    pub fn perform_full_sync(
        &self,
        config: SyncConfigDto,
        force_sync: bool,
        cancellation_token: Option<SyncCancellationToken>,
        progress_sink: Option<SyncProgressSink>,
        target_progress: Option<&crate::sync::full_sync::SyncProgressCallback>,
    ) -> ApiResult<FullSyncResultDto> {
        self.perform_full_sync_internal(
            config,
            force_sync,
            cancellation_token,
            progress_sink,
            target_progress,
            None,
            None,
        )
    }

    /// Harmony C ABI entry that has already recorded local preflight stages.
    /// Reuse its operation id so Core's detailed phases join the FFI diagnostics.
    pub(crate) fn perform_full_sync_for_operation_id(
        &self,
        config: SyncConfigDto,
        force_sync: bool,
        operation_id: String,
        trigger: &str,
    ) -> ApiResult<FullSyncResultDto> {
        self.perform_full_sync_internal(
            config,
            force_sync,
            None,
            None,
            None,
            Some(operation_id),
            Some(trigger.to_string()),
        )
    }

    fn perform_full_sync_internal(
        &self,
        config: SyncConfigDto,
        force_sync: bool,
        cancellation_token: Option<SyncCancellationToken>,
        progress_sink: Option<SyncProgressSink>,
        target_progress: Option<&crate::sync::full_sync::SyncProgressCallback>,
        operation_id_override: Option<String>,
        trigger_override: Option<String>,
    ) -> ApiResult<FullSyncResultDto> {
        let operation_id = operation_id_override
            .or_else(|| {
                progress_sink
                    .as_ref()
                    .and_then(SyncProgressSink::operation_id)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let started_at = Instant::now();
        record_sync_diagnostic("sync.run.start", "sync.run", {
            let mut fields = sync_diagnostic_fields(&operation_id, "prepare", None);
            fields.insert(
                "trigger".to_string(),
                serde_json::Value::String(
                    progress_sink
                        .as_ref()
                        .and_then(SyncProgressSink::trigger)
                        .or(trigger_override.as_deref())
                        .unwrap_or(if force_sync { "manual" } else { "auto" })
                        .to_string(),
                ),
            );
            fields
        });
        let mut result = self.perform_full_sync_once(
            config.clone(),
            force_sync,
            cancellation_token.clone(),
            progress_sink.clone(),
            target_progress,
            &operation_id,
        );
        let sync_config: crate::sync::SyncConfig = config.clone().into();
        let mut retried_via_github_api = false;
        let mut replanned_after_precondition = false;

        loop {
            // Issue #990：GitHub 仓库在传输中途才暴露网络不可达时，不能沿用
            // 旧的前置条件和半途结果。标记 Git 不可达后从 Prepare 阶段重新
            // 开始完整同步，由 router 改走 api.github.com。每轮最多一次。
            let retry_via_api = !retried_via_github_api
                && match &result {
                    Err(error) => writer_error_is_retryable_network(error),
                    Ok(output) => full_sync_has_network_failure(output),
                }
                && should_retry_full_sync_via_github_api(&sync_config);
            if retry_via_api {
                crate::sync::provider::router::mark_git_unreachable();
                retried_via_github_api = true;
                result = self.perform_full_sync_once(
                    config.clone(),
                    force_sync,
                    cancellation_token.clone(),
                    progress_sink.clone(),
                    target_progress,
                    &operation_id,
                );
                continue;
            }

            // 远端 HEAD 在规划后变化：旧 batch 即使重试也不能提交。重新执行
            // 整轮 Prepare → 读取 catalog → LWW → 新 batch，每轮使用新 provider
            // 和新远端快照；不能靠 generation 层重放旧 mutations。
            let cancelled = cancellation_token
                .as_ref()
                .is_some_and(SyncCancellationToken::is_cancelled);
            let retry_after_precondition = !replanned_after_precondition
                && !cancelled
                && match &result {
                    Ok(output) => full_sync_has_precondition_failure(output),
                    Err(crate::api::error::WriterError::PreconditionFailed(_)) => true,
                    Err(_) => false,
                };
            if retry_after_precondition {
                log::info!(
                    "[sync] perform_full_sync: remote precondition changed — replanning the full sync"
                );
                replanned_after_precondition = true;
                result = self.perform_full_sync_once(
                    config.clone(),
                    force_sync,
                    cancellation_token.clone(),
                    progress_sink.clone(),
                    target_progress,
                    &operation_id,
                );
                continue;
            }

            break;
        }
        let mut fields = sync_diagnostic_fields(&operation_id, "complete", None);
        let cancelled = cancellation_token
            .as_ref()
            .is_some_and(SyncCancellationToken::is_cancelled);
        fields.insert(
            "trigger".to_string(),
            serde_json::Value::String(
                progress_sink
                    .as_ref()
                    .and_then(SyncProgressSink::trigger)
                    .or(trigger_override.as_deref())
                    .unwrap_or(if force_sync { "manual" } else { "auto" })
                    .to_string(),
            ),
        );
        fields.insert(
            "duration_ms".to_string(),
            serde_json::Value::Number(serde_json::Number::from(
                u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
            )),
        );
        fields.insert(
            "outcome".to_string(),
            serde_json::Value::String(
                if cancelled {
                    "cancelled"
                } else if result.is_ok() {
                    "returned"
                } else {
                    "error"
                }
                .to_string(),
            ),
        );
        fields.insert(
            "termination_reason".to_string(),
            serde_json::Value::String(if cancelled {
                "cancelled_by_platform".to_string()
            } else if let Ok(output) = &result {
                output.overall_status.clone()
            } else {
                "error".to_string()
            }),
        );
        fields.insert(
            "cancel_requested".to_string(),
            serde_json::Value::Bool(cancelled),
        );
        if let Ok(output) = &result {
            fields.insert(
                "status_code".to_string(),
                serde_json::Value::String(output.overall_status.clone()),
            );
        } else if let Err(error) = &result {
            fields.insert(
                "error_code".to_string(),
                serde_json::Value::String(error.code().to_string()),
            );
            fields.insert(
                "error_category".to_string(),
                serde_json::Value::String(sync_error_category(error).to_string()),
            );
        }
        record_sync_diagnostic("sync.run.end", "sync.run", fields);
        result
    }

    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn perform_full_sync_once(
        &self,
        config: SyncConfigDto,
        force_sync: bool,
        cancellation_token: Option<SyncCancellationToken>,
        progress_sink: Option<SyncProgressSink>,
        target_progress: Option<&crate::sync::full_sync::SyncProgressCallback>,
        diagnostic_operation_id: &str,
    ) -> ApiResult<FullSyncResultDto> {
        let sync_config: crate::sync::SyncConfig = config.into();

        // 与 sync disabled 相同的 no-op FullSyncResult。
        // Issue #729：各阶段边界检查取消令牌后复用此闭包返回，避免重复构造。
        let make_noop_result =
            || -> ApiResult<FullSyncResultDto> { Ok(Self::full_sync_noop_result()) };

        // sync disabled → 直接返回 no-op，
        // 不创建 provider、不读 catalog、不建 plan、不进入 run_transfer。
        // 防止 disabled 状态下仍写远端（LiveProject 会发布只有 generation.meta.json
        // 没有 正文/manifest 的空 active generation）。
        if !sync_config.enabled {
            log::debug!("[sync] perform_full_sync: sync disabled — returning no-op");
            return make_noop_result();
        }

        // Issue #729：取消令牌已标记取消时，直接返回 no-op，与 sync disabled 相同逻辑。
        // 平台层切工作区时调用 token.cancel()，此处感知后提前终止，不进入网络阶段。
        if let Some(ref token) = cancellation_token {
            if token.is_cancelled() {
                log::debug!(
                    "[sync] perform_full_sync: cancellation token already cancelled — returning no-op"
                );
                return make_noop_result();
            }
        }

        // Snapshot secrets before acquiring core_write（避免持锁期间回调 override）。
        let secrets = self.secrets_override_snapshot().unwrap_or_default();

        // Phase 1: Prepare — 拆成三段短锁，网络 IO 不持 core 写锁。
        //   load_remote_catalog 是网络 IO，必须在
        // core_write 作用域外执行，不阻塞正文/作品读取（ 拆锁路线）。
        //
        // 1a. 短锁 A：创建 provider（transport 初始化可能涉及本地 IO）。
        let provider_phase =
            SyncDiagnosticPhase::start(diagnostic_operation_id, "provider_init", None);
        let provider_result = {
            let core = self.core_write();
            core.create_sync_provider_for_plan(&sync_config, &secrets)
        };
        let provider = match provider_result {
            Ok(provider) => provider,
            Err(error) => {
                provider_phase.finish("error");
                let category = {
                    let category = error.sync_category();
                    if category.is_empty() {
                        "other".to_string()
                    } else {
                        category.to_string()
                    }
                };
                let detail = error.to_string();
                let status = crate::sync::full_sync::error_to_persist_status(&error);
                let status_code = sync_status_to_wire(&status);
                if let Err(state_error) = self.record_full_sync_preflight_failure_typed(
                    status_code,
                    "provider_init".to_string(),
                    category.clone(),
                    "provider_init".to_string(),
                    detail.clone(),
                ) {
                    log::warn!(
                        "[sync] provider_init failed-state write failed: error_code={}",
                        state_error.code()
                    );
                }
                let writer_error = crate::api::error::WriterError::SyncFailure {
                    category,
                    stage: "provider_init".to_string(),
                    detail,
                };
                record_sync_phase_failure(diagnostic_operation_id, "provider_init", &writer_error);
                return Err(writer_error);
            }
        };
        provider.set_operation_id(Some(diagnostic_operation_id));
        provider_phase.finish("success");

        // A checkpoint snapshot must observe StarMap Store dirty state on disk.
        // The later staging-seed flush remains necessary for edits made while the
        // local snapshot and plan are being prepared.
        let starmap_flush_phase = SyncDiagnosticPhase::start_with_provider(
            diagnostic_operation_id,
            "starmap_store_flush",
            None,
            provider.as_ref(),
        );
        if let Err(error) = self.flush_all_starmap_stores() {
            starmap_flush_phase.finish_with_provider("error", provider.as_ref());
            let error_category = sync_error_category(&error).to_string();
            let writer_error = error;
            record_sync_phase_failure(
                diagnostic_operation_id,
                "starmap_store_flush",
                &writer_error,
            );
            log::warn!(
                "[sync] starmap_store_flush failed: error_code={}",
                writer_error.code()
            );
            let _ = self.record_full_sync_preflight_failure_typed(
                "recoverable_error".to_string(),
                "starmap_store_flush".to_string(),
                error_category,
                "starmap_store_flush".to_string(),
                writer_error.to_string(),
            );
            return Err(writer_error);
        }
        starmap_flush_phase.finish_with_provider("success", provider.as_ref());

        let (app_data_root, projects_root, previous_full_sync_state) = {
            let core = self.core_write();
            (
                core.app_data_root.clone(),
                core.projects_root.clone(),
                core.load_full_sync_state().ok().flatten(),
            )
        };
        let repo_identity = repository_identity(&sync_config);
        let revision_phase = SyncDiagnosticPhase::start_with_provider(
            diagnostic_operation_id,
            "remote_revision",
            None,
            provider.as_ref(),
        );
        let remote_revision_before = match provider.remote_revision() {
            Ok(revision) => revision,
            Err(error) => {
                revision_phase.finish_with_provider("error", provider.as_ref());
                let core_error = crate::error::Error::from(error);
                let status = crate::sync::full_sync::error_to_persist_status(&core_error);
                let error_category = core_sync_category(&core_error);
                let writer_error = crate::api::error::WriterError::from(core_error);
                record_sync_phase_failure(
                    diagnostic_operation_id,
                    "remote_revision",
                    &writer_error,
                );
                log::warn!(
                    "[sync] remote_revision failed: error_code={}",
                    writer_error.code()
                );
                let _ = self.record_full_sync_preflight_failure_typed(
                    sync_status_to_wire(&status),
                    "remote_revision".to_string(),
                    error_category,
                    "remote_revision".to_string(),
                    writer_error.to_string(),
                );
                return Err(writer_error);
            }
        };
        revision_phase.finish_with_provider("success", provider.as_ref());
        let checkpoint = crate::sync::full_sync_state::FullSyncCheckpoint::load(&app_data_root)
            .ok()
            .flatten();
        let previous_was_converged = previous_full_sync_state.as_ref().is_some_and(|state| {
            crate::sync::full_sync_state::FullSyncState::is_overall_success(&state.overall_status)
                && state.failed_targets.is_empty()
        });
        let mut checkpoint_remote_unchanged = !force_sync
            && previous_was_converged
            && checkpoint.as_ref().is_some_and(|checkpoint| {
                repo_identity
                    .as_ref()
                    .is_some_and(|identity| checkpoint.repository_identity == *identity)
                    && remote_revision_before.as_deref()
                        == Some(checkpoint.remote_revision.as_str())
            });
        let preflight_snapshot = if checkpoint_remote_unchanged {
            let local_snapshot_phase = SyncDiagnosticPhase::start_with_provider(
                diagnostic_operation_id,
                "local_snapshot",
                None,
                provider.as_ref(),
            );
            let snapshot = local_sync_version_snapshot(&app_data_root, &projects_root);
            match snapshot {
                Ok(snapshot) => {
                    local_snapshot_phase.finish_with_provider(
                        if snapshot.is_some() {
                            "success"
                        } else {
                            "ineligible"
                        },
                        provider.as_ref(),
                    );
                    snapshot
                }
                Err(error) => {
                    local_snapshot_phase.finish_with_provider("error", provider.as_ref());
                    log::warn!(
                        "[sync] local checkpoint snapshot failed; continuing with LWW: {error}"
                    );
                    None
                }
            }
        } else {
            None
        };

        // Fast path is allowed only after a successful, fully converged run and
        // only when both the local object versions and remote branch revision
        // match that run's checkpoint. Any uncertainty falls through to LWW.
        if let (Some(checkpoint), Some(snapshot)) =
            (checkpoint.as_ref(), preflight_snapshot.as_ref())
        {
            if checkpoint.local_object_fingerprint == snapshot.fingerprint {
                log::info!(
                    "[sync] full-sync checkpoint matched; returning no_changes without target transfers"
                );
                let mut fields =
                    sync_diagnostic_fields(diagnostic_operation_id, "checkpoint_match", None);
                fields.insert(
                    "skipped_targets".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(
                        u64::try_from(snapshot.target_fingerprints.len()).unwrap_or(u64::MAX),
                    )),
                );
                record_sync_diagnostic("sync.checkpoint.skip", "sync.run", fields);
                let (mut result, _, _) = self
                    .core_write()
                    .finalize_full_sync(snapshot.no_change_targets.clone(), None);
                // The target results were persisted by finalize_full_sync. Returning
                // an empty list lets the platform distinguish this checkpoint no-op
                // and avoid launching generation GC on every polling interval.
                result.targets.clear();
                return Ok(result.into());
            }
        }

        // 写锁已释放。
        // 1b. 无锁：读 remote catalog（网络 IO，只读一个文件）。
        //   catalog 读取失败直接结束本次 full sync，
        // 返回 RecoverableError，不构造空 catalog 继续 plan（空 catalog 会让
        // planner 误判"远端无记录"做破坏性删除/复活决策）。
        //   用 discover_legacy_remote_catalog（真做 legacy 枚举），
        // 不再用 load_remote_catalog（只读 catalog 文件，不做 legacy 发现）。
        let remote_catalog_phase = SyncDiagnosticPhase::start_with_provider(
            diagnostic_operation_id,
            "remote_catalog",
            None,
            provider.as_ref(),
        );
        let remote_catalog_snapshot =
            match crate::sync::target_lifecycle::discover_legacy_remote_catalog(provider.as_ref()) {
                Ok(snapshot) => snapshot,
                Err(e) => {
                    remote_catalog_phase.finish_with_provider("error", provider.as_ref());
                    let msg = format!("discover_legacy_remote_catalog failed: {e}");
                    log::warn!("[sync] prepare: {msg}");
                    let status = crate::sync::full_sync::error_to_persist_status(&e);
                    let error_category = core_sync_category(&e);
                    let _ = self.record_full_sync_preflight_failure_typed(
                        sync_status_to_wire(&status),
                        "target_catalog".to_string(),
                        error_category,
                        "target_catalog".to_string(),
                        msg,
                    );
                    return Err(crate::api::error::WriterError::from(e));
                }
            };
        remote_catalog_phase.finish_with_provider("success", provider.as_ref());

        // Issue #729：discover_legacy_remote_catalog 返回后检查取消令牌。
        // 取消则返回 no-op，不进入 persist_bootstrap_catalog / plan / transfer。
        if let Some(ref token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] perform_full_sync: cancellation requested after discover_legacy_remote_catalog — returning no-op"
                );
                return make_noop_result();
            }
        }

        //   catalog 文件不存在于远端（version == __nonexistent__）
        // → 正式 sync 需要把 discover 合成的 bootstrap catalog 落盘，后续 CAS 写入才有 base version。
        // dry-run 不走本路径（dry-run 用 perform_full_sync_dry_run_with_catalog，不 persist）。
        // Issue #729：persist_bootstrap_catalog 前检查取消令牌。
        if let Some(ref token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] perform_full_sync: cancellation requested before persist_bootstrap_catalog — returning no-op"
                );
                return make_noop_result();
            }
        }
        let remote_catalog_snapshot =
            if remote_catalog_snapshot.version.as_str() == "__nonexistent__" {
                match crate::sync::target_lifecycle::persist_bootstrap_catalog(
                    provider.as_ref(),
                    &remote_catalog_snapshot.catalog,
                    &remote_catalog_snapshot.version,
                ) {
                    Ok(persisted) => {
                        log::info!(
                            "[sync] prepare: persisted bootstrap catalog ({} records) to remote",
                            persisted.catalog.records.len()
                        );
                        persisted
                    }
                    Err(e) => {
                        let msg = format!("persist_bootstrap_catalog failed: {e}");
                        log::warn!("[sync] prepare: {msg}");
                        let status = crate::sync::full_sync::error_to_persist_status(&e);
                        let error_category = core_sync_category(&e);
                        let _ = self.record_full_sync_preflight_failure_typed(
                            sync_status_to_wire(&status),
                            "target_catalog".to_string(),
                            error_category,
                            "target_catalog".to_string(),
                            msg,
                        );
                        return Err(crate::api::error::WriterError::from(e));
                    }
                }
            } else {
                remote_catalog_snapshot
            };

        // App data-group catalog is independent from the project lifecycle catalog.
        // A malformed/unreadable group catalog fails Prepare instead of guessing a
        // flat App layout and risking a mixed or destructive merge.
        let app_group_catalog_phase = SyncDiagnosticPhase::start_with_provider(
            diagnostic_operation_id,
            "remote_app_group_catalog",
            None,
            provider.as_ref(),
        );
        let remote_app_data_groups =
            match crate::sync::app_data_group_catalog::discover_remote_app_data_groups(
                provider.as_ref(),
            ) {
                Ok(groups) => groups,
                Err(e) => {
                    app_group_catalog_phase.finish_with_provider("error", provider.as_ref());
                    let msg = format!("discover_remote_app_data_groups failed: {e}");
                    log::warn!("[sync] prepare: {msg}");
                    let status = crate::sync::full_sync::error_to_persist_status(&e);
                    let error_category = core_sync_category(&e);
                    let _ = self.record_full_sync_preflight_failure_typed(
                        sync_status_to_wire(&status),
                        "app_group_catalog".to_string(),
                        error_category,
                        "app_group_catalog".to_string(),
                        msg,
                    );
                    return Err(crate::api::error::WriterError::from(e));
                }
            };
        app_group_catalog_phase.finish_with_provider("success", provider.as_ref());
        // Issue #729：persist_bootstrap_catalog 返回后检查取消令牌。
        if let Some(ref token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] perform_full_sync: cancellation requested after persist_bootstrap_catalog — returning no-op"
                );
                return make_noop_result();
            }
        }
        // 1c. 短锁 B：只 persist Syncing + snapshot app_data_root/projects_root。
        //   回退问题：恢复短锁+锁外扫描。
        // 短锁只拿 app_data_root/projects_root/sync_policy/remote snapshot + persist Syncing，
        // 释放锁后 list_projects/pending/device/planner/scan 全部锁外执行，
        // 避免阻塞正文/作品读取。
        self.core_write().persist_full_sync_started();
        // 写锁已释放。锁外构建 plan（list_projects / pending / device / planner / scan）。
        let plan_phase = SyncDiagnosticPhase::start_with_provider(
            diagnostic_operation_id,
            "plan",
            None,
            provider.as_ref(),
        );
        let mut plan = match crate::facade::WriterCore::build_full_sync_plan_unlocked(
            &app_data_root,
            &projects_root,
            &sync_config,
            force_sync,
            &remote_catalog_snapshot.catalog,
            &remote_app_data_groups,
            remote_catalog_snapshot.clone(),
        ) {
            Ok(plan) => plan,
            Err(err) => {
                plan_phase.finish_with_provider("error", provider.as_ref());
                let msg = err.to_string();
                log::warn!("[sync] prepare: build_full_sync_plan_unlocked failed: {msg}");
                let status = crate::sync::full_sync::error_to_persist_status(&err);
                let error_category = core_sync_category(&err);
                let _ = self.record_full_sync_preflight_failure_typed(
                    sync_status_to_wire(&status),
                    "global".to_string(),
                    error_category,
                    "plan".to_string(),
                    msg,
                );
                return Err(crate::api::error::WriterError::from(err));
            }
        };
        plan_phase.finish_with_provider("success", provider.as_ref());

        if checkpoint_remote_unchanged {
            let revision_phase = SyncDiagnosticPhase::start_with_provider(
                diagnostic_operation_id,
                "remote_revision_before_group_filter",
                None,
                provider.as_ref(),
            );
            match provider.remote_revision() {
                Ok(revision) if revision == remote_revision_before => {
                    revision_phase.finish_with_provider("success", provider.as_ref());
                }
                Ok(_) => {
                    revision_phase.finish_with_provider("changed", provider.as_ref());
                    checkpoint_remote_unchanged = false;
                    log::info!(
                        "[sync] remote branch changed during prepare; keeping every target in the LWW plan"
                    );
                }
                Err(error) => {
                    revision_phase.finish_with_provider("error", provider.as_ref());
                    return Err(crate::api::error::WriterError::from(
                        crate::error::Error::from(error),
                    ));
                }
            }
        }

        // Issue #729：build_full_sync_plan_unlocked 返回后检查取消令牌。
        if let Some(ref token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] perform_full_sync: cancellation requested after build_full_sync_plan_unlocked — returning no-op"
                );
                // Issue #729 评论 5765306162 问题6：persist_full_sync_started 已写
                // Syncing，取消前持久化取消终态，避免 full_state.local.json 停在 Syncing。
                self.core_write().persist_full_sync_cancelled();
                return make_noop_result();
            }
        }

        // If the remote branch is unchanged, only locally changed groups need a
        // plan, staging seed, and transfer. New or lifecycle targets have no prior
        // fingerprint and remain in the plan so the normal LWW path handles them.
        if checkpoint_remote_unchanged {
            if let (Some(checkpoint), Some(snapshot)) =
                (checkpoint.as_ref(), preflight_snapshot.as_ref())
            {
                let previous_target_count = plan.targets.len();
                plan.targets.retain(|planned| {
                    let Some(key) = sync_target_checkpoint_key(planned) else {
                        return true;
                    };
                    match (
                        checkpoint.target_fingerprints.get(&key),
                        snapshot.target_fingerprints.get(&key),
                    ) {
                        (Some(previous), Some(current)) => previous != current,
                        _ => true,
                    }
                });
                let skipped_target_count = previous_target_count.saturating_sub(plan.targets.len());
                if skipped_target_count > 0 {
                    log::info!(
                        "[sync] unchanged local targets skipped from transfer plan: count={skipped_target_count}"
                    );
                }
                if plan.targets.is_empty() {
                    let mut fields =
                        sync_diagnostic_fields(diagnostic_operation_id, "unchanged_targets", None);
                    fields.insert(
                        "skipped_targets".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(
                            u64::try_from(skipped_target_count).unwrap_or(u64::MAX),
                        )),
                    );
                    record_sync_diagnostic("sync.checkpoint.skip", "sync.run", fields);
                    let (mut result, _, _) = self
                        .core_write()
                        .finalize_full_sync(snapshot.no_change_targets.clone(), None);
                    result.targets.clear();
                    return Ok(result.into());
                }
            }
        }

        // Phase 2: Seed staging（不持锁）— 磁盘扫描/复制，创建隔离 staging 目录。
        // seed 失败直接终止本次同步，不继续拿半成品。
        // prepare_staging_runs 是纯函数，不依赖 WriterCore，无需持锁。
        //
        // seed 失败时必须把 FullSyncState 从 Syncing
        // 改为失败终态，否则下次启动/同步会永久看到上一次遗留的 Syncing。
        //
        //   staging 不再按 active_provider 分 Git/GithubApi
        // backend 走不同 seed 路径；统一调 `seed_from_live`（文件级复制）。
        // workspace 级别的 Git layout 迁移仍由 `prepare_staging_runs` 内部完成，
        // 但不作为某个 remote provider 的 staging 模式。
        // Issue #729：prepare_staging_runs 前检查取消令牌。
        if let Some(ref token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] perform_full_sync: cancellation requested before prepare_staging_runs — returning no-op"
                );
                // Issue #729 评论 5765306162 问题6：persist_full_sync_started 已写
                // Syncing，取消前持久化取消终态。
                self.core_write().persist_full_sync_cancelled();
                return make_noop_result();
            }
        }
        // staging seed 前 flush 所有 Store 的 dirty/删除状态，
        // 保证 staging seed 能看到本地最新修改（问题1）。
        // flush 失败则终止同步——不能让 staging seed 基于过时数据做三方合并。
        if let Err(e) = self.core_write().flush_all_starmap_stores() {
            let msg = format!("flush_all_starmap_stores before staging seed failed: {e}");
            log::warn!("[sync] prepare: {msg}");
            let status = crate::sync::full_sync::error_to_persist_status(&e);
            let error_category = core_sync_category(&e);
            let _ = self.record_full_sync_preflight_failure_typed(
                sync_status_to_wire(&status),
                "staging_seed".to_string(),
                error_category,
                "staging_seed".to_string(),
                msg,
            );
            return Err(crate::api::error::WriterError::from(e));
        }
        // Issue #1005: 在 staging seed 前一次性迁移本地旧 settings.sync.json。
        // 这样 base/live/staging 快照从同一份规范内容开始，compute_commit_plan
        // 只读 live，不会在失败/取消时提前改写用户文件。
        for planned_target in &plan.targets {
            if planned_target.target.app_group.as_ref()
                != Some(&crate::sync::types::AppDataGroup::Global)
            {
                continue;
            }
            let settings_path = planned_target.target_live_root.join("settings.sync.json");
            if let Err(e) = crate::settings::normalize_syncable_settings_file(&settings_path) {
                let msg = format!(
                    "normalize settings.sync.json before staging seed failed at {}: {e}",
                    settings_path.display()
                );
                log::warn!("[sync] prepare: {msg}");
                let status = crate::sync::full_sync::error_to_persist_status(&e);
                let error_category = core_sync_category(&e);
                let _ = self.record_full_sync_preflight_failure_typed(
                    sync_status_to_wire(&status),
                    "staging_seed".to_string(),
                    error_category,
                    "staging_seed".to_string(),
                    msg,
                );
                return Err(crate::api::error::WriterError::from(e));
            }
        }
        let staging_phase = SyncDiagnosticPhase::start_with_provider(
            diagnostic_operation_id,
            "staging",
            None,
            provider.as_ref(),
        );
        let staging_runs = match crate::sync::staging::prepare_staging_runs(&mut plan) {
            Ok(runs) => runs,
            Err(err) => {
                staging_phase.finish_with_provider("error", provider.as_ref());
                let status = crate::sync::full_sync::error_to_persist_status(&err);
                let error_category = core_sync_category(&err);
                let _ = self.record_full_sync_preflight_failure_typed(
                    sync_status_to_wire(&status),
                    "staging_seed".to_string(),
                    error_category,
                    "staging_seed".to_string(),
                    err.to_string(),
                );
                return Err(err.into());
            }
        };
        staging_phase.finish_with_provider("success", provider.as_ref());

        if checkpoint_remote_unchanged {
            let revision_phase = SyncDiagnosticPhase::start_with_provider(
                diagnostic_operation_id,
                "remote_revision_before_transfer",
                None,
                provider.as_ref(),
            );
            match provider.remote_revision() {
                Ok(revision) if revision == remote_revision_before => {
                    revision_phase.finish_with_provider("success", provider.as_ref());
                }
                Ok(_) => {
                    revision_phase.finish_with_provider("changed", provider.as_ref());
                    return Err(crate::api::error::WriterError::PreconditionFailed(
                        "remote branch changed before a filtered target sync".to_string(),
                    ));
                }
                Err(error) => {
                    revision_phase.finish_with_provider("error", provider.as_ref());
                    return Err(crate::api::error::WriterError::from(
                        crate::error::Error::from(error),
                    ));
                }
            }
        }

        // Issue #729：prepare_staging_runs 返回后检查取消令牌。
        if let Some(ref token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] perform_full_sync: cancellation requested after prepare_staging_runs — returning no-op"
                );
                // Issue #729 评论 5765306162 问题6：persist_full_sync_started 已写
                // Syncing，取消前持久化取消终态。
                self.core_write().persist_full_sync_cancelled();
                return make_noop_result();
            }
        }

        // Phase 3+4+5：逐 target Transfer → Commit → progress，整轮结束聚合收口。
        // Issue #779 评论 5854082763：perform_full_sync_with_provider 拿到终态后立即返回。
        // generation GC maintenance 不再在用户同步路径内 spawn，改由平台层通过
        // perform_generation_gc_maintenance 异步调用（独立 maintenance token，
        // 不复用用户同步 token），避免 GC detached thread 和下一轮用户同步同时写
        // 同一 branch 导致 PreconditionFailed。
        let result = self.perform_full_sync_with_provider_for_run(
            provider.as_ref(),
            &plan,
            staging_runs,
            cancellation_token,
            progress_sink,
            target_progress,
            diagnostic_operation_id,
            remote_revision_before.as_deref(),
        )?;

        let fully_converged = matches!(
            result.overall_status.as_str(),
            "success" | "no_changes" | "latest_wins_applied"
        ) && result.targets.iter().all(|target| {
            matches!(
                target.result.status.as_str(),
                "success" | "no_changes" | "latest_wins_applied"
            )
        });
        if fully_converged {
            if let (
                Some(repository_identity),
                Some(revision_before),
                Ok(Some(revision_after)),
                Ok(Some(snapshot)),
            ) = (
                repo_identity.as_ref(),
                remote_revision_before.as_deref(),
                provider.remote_revision(),
                local_sync_version_snapshot(&app_data_root, &projects_root),
            ) {
                // A changed branch revision means another commit (or our own
                // upload) occurred during this pass. Let a later stable pass
                // establish the checkpoint instead of claiming that unseen
                // remote state was already consumed.
                if revision_before == revision_after {
                    let checkpoint = crate::sync::full_sync_state::FullSyncCheckpoint {
                        repository_identity: repository_identity.clone(),
                        remote_revision: revision_after,
                        local_object_fingerprint: snapshot.fingerprint,
                        target_fingerprints: snapshot.target_fingerprints,
                    };
                    if let Err(error) = checkpoint.save(&app_data_root) {
                        log::warn!("[sync] failed to persist full-sync checkpoint: {error}");
                    }
                }
            }
        }

        // 同步可能修改磁盘上的星图数据（staging → commit），失效路径依赖索引
        // 确保下次查询从真实数据重建，不持有过期 entry（问题1）。
        // perform_full_sync_with_provider 内部已对每个 target evict 旧 Store 并在循环
        // 结束后统一失效索引（问题2）；此处再失效一次是冗余但安全的兜底，
        // 覆盖 staging seed 等不经过 per-target 循环的磁盘变更路径。
        self.core_write().invalidate_path_dependency_index();

        Ok(result)
    }

    /// 全量同步编排尾部 — 逐 target `Transfer → Commit → progress`，整轮结束聚合收口。
    ///
    /// Issue #762 评论 5828791004：不能同时存在"Transfer 提前写 live"和
    /// "整轮结束后统一 Commit staging"两套权威。每个 target 的顺序固定为
    /// `Transfer（不持锁）→ Commit（短写锁，写 live 终态）→ progress`：
    /// progress 发出时该 target 的 `state.local.json / conflicts.json` 已经是 live
    /// 最终状态，用户在同步进行中立即 resolve，后面其他 target 继续同步也不会再碰
    /// 这个 target 的 staging（commit 后立即 cleanup）。
    ///
    /// 整轮结束只做全局收口：generation GC、aggregate、`FullSyncState`、
    /// search index、lifecycle deletes、workspace history；
    /// **不再第二次提交任何已完成 target 的 staging**。
    ///
    /// 生产路径由 [`Self::perform_full_sync`] 在 Prepare / Seed 之后调用；
    /// 集成测试用 `MemoryProvider` + 手工 `FullSyncPlan` + `prepare_staging_runs`
    /// 驱动同一条编排。
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    pub fn perform_full_sync_with_provider(
        &self,
        provider: &dyn crate::sync::provider::SyncProvider,
        plan: &crate::sync::full_sync::FullSyncPlan,
        staging_runs: Vec<crate::sync::staging::StagingRun>,
        cancellation_token: Option<SyncCancellationToken>,
        progress_sink: Option<SyncProgressSink>,
        target_progress: Option<&crate::sync::full_sync::SyncProgressCallback>,
    ) -> ApiResult<FullSyncResultDto> {
        let operation_id = progress_sink
            .as_ref()
            .and_then(SyncProgressSink::operation_id)
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        self.perform_full_sync_with_provider_for_run(
            provider,
            plan,
            staging_runs,
            cancellation_token,
            progress_sink,
            target_progress,
            &operation_id,
            None,
        )
    }

    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn perform_full_sync_with_provider_for_run(
        &self,
        provider: &dyn crate::sync::provider::SyncProvider,
        plan: &crate::sync::full_sync::FullSyncPlan,
        staging_runs: Vec<crate::sync::staging::StagingRun>,
        cancellation_token: Option<SyncCancellationToken>,
        progress_sink: Option<SyncProgressSink>,
        target_progress: Option<&crate::sync::full_sync::SyncProgressCallback>,
        diagnostic_operation_id: &str,
        remote_revision: Option<&str>,
    ) -> ApiResult<FullSyncResultDto> {
        let mut all_targets: Vec<crate::sync::types::TargetSyncResult> = Vec::new();
        let mut all_committed_paths: Vec<std::path::PathBuf> = Vec::new();
        let mut catalog_snapshot = plan.remote_catalog_snapshot.clone();

        let total_targets = u32::try_from(plan.targets.len()).unwrap_or(u32::MAX);

        for target_index in 0..plan.targets.len() {
            // Issue #729：每个 target 前检查取消令牌。
            if Self::sync_cancelled(cancellation_token.as_ref()) {
                log::info!(
                    "[sync] perform_full_sync: cancellation requested — breaking after {} targets",
                    all_targets.len()
                );
                break;
            }

            let planned = &plan.targets[target_index];
            let target_digest = sync_target_digest(&planned.target.remote_prefix);
            let target_kind = planned.target_kind.as_target_kind_str().to_string();
            let target_id = planned
                .project_id
                .clone()
                .or_else(|| planned.target.app_group.as_ref().map(|group| group.id()));
            let target_id_digest = target_id.as_deref().map(sync_target_digest);
            let active_generation = planned.project_id.as_ref().and_then(|project_id| {
                catalog_snapshot
                    .catalog
                    .records
                    .iter()
                    .find(|record| {
                        record.remote_prefix == planned.target.remote_prefix
                            || record.target_id == format!("projects/{project_id}")
                    })
                    .and_then(|record| record.active_generation.clone())
            });
            let target_started_at = Instant::now();
            let mut target_fields =
                sync_diagnostic_fields(diagnostic_operation_id, "transfer", Some(&target_digest));
            target_fields.insert(
                "target_kind".to_string(),
                serde_json::Value::String(target_kind.clone()),
            );
            target_fields.insert(
                "target_index".to_string(),
                serde_json::Value::Number(serde_json::Number::from(
                    u64::try_from(target_index).unwrap_or(u64::MAX),
                )),
            );
            target_fields.insert(
                "total_targets".to_string(),
                serde_json::Value::Number(serde_json::Number::from(total_targets)),
            );
            if let Some(target_id_digest) = target_id_digest.as_ref() {
                target_fields.insert(
                    "target_id_digest".to_string(),
                    serde_json::Value::String(target_id_digest.clone()),
                );
            }
            if let Some(remote_revision) = remote_revision {
                target_fields.insert(
                    "remote_revision".to_string(),
                    serde_json::Value::String(remote_revision.to_string()),
                );
            }
            if let Some(active_generation) = active_generation.as_deref() {
                target_fields.insert(
                    "remote_generation_digest".to_string(),
                    serde_json::Value::String(sync_target_digest(active_generation)),
                );
            }
            record_sync_diagnostic("sync.target.start", "sync.target", target_fields);

            // 1. sink.update_target_start — 标记开始处理该 target（Transfer 阶段）。
            if let Some(ref sink) = progress_sink {
                let finished = u32::try_from(target_index).unwrap_or(u32::MAX);
                sink.update_target_start(
                    &planned.target.remote_prefix,
                    planned.project_id.as_deref(),
                    "transfer",
                    finished,
                    total_targets,
                );
            }

            // 2. Transfer（不持锁）。
            // #955：Prepare 阶段失败的 target（例如本机统计事件库迁移失败）只让
            // 该组失败，其余独立组继续同步；不进入 Transfer / Commit。
            let prepared_failure = staging_runs
                .get(target_index)
                .and_then(|run| run.prepare_error());
            let transfer_phase = SyncDiagnosticPhase::start_with_provider(
                diagnostic_operation_id,
                "transfer",
                Some(target_digest.clone()),
                provider,
            );
            let transfer_result = if prepared_failure.is_some() {
                Some(crate::sync::full_sync::prepare_failed_target_result(
                    plan,
                    target_index,
                    prepared_failure.unwrap_or_default(),
                ))
            } else {
                crate::sync::full_sync::run_single_target_transfer(
                    provider,
                    plan,
                    target_index,
                    &mut catalog_snapshot,
                    cancellation_token.as_ref(),
                    &all_targets,
                    Some(diagnostic_operation_id),
                )
                .map(|(result, _resolution, _action)| result)
            };
            let Some(target_sync_result) = transfer_result else {
                transfer_phase.finish_with_provider("cancelled", provider);
                let mut fields = sync_diagnostic_fields(
                    diagnostic_operation_id,
                    "cancelled",
                    Some(&target_digest),
                );
                fields.insert(
                    "duration_ms".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(
                        u64::try_from(target_started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
                    )),
                );
                record_sync_diagnostic("sync.target.end", "sync.target", fields);
                // target 未执行（取消或索引越界），跳过。
                continue;
            };
            let transfer_outcome = if prepared_failure.is_some() {
                "prepare_failed".to_string()
            } else {
                crate::api::types::sync_status_to_wire(&target_sync_result.result.status)
            };
            transfer_phase.finish_with_provider(&transfer_outcome, provider);
            for path in &target_sync_result.result.downloaded_files {
                record_sync_object_result(
                    diagnostic_operation_id,
                    &target_digest,
                    path,
                    "downloaded",
                );
            }
            for path in &target_sync_result.result.local_deletes {
                record_sync_object_result(
                    diagnostic_operation_id,
                    &target_digest,
                    path,
                    "deleted_locally",
                );
            }
            for path in &target_sync_result.result.remote_deletes {
                record_sync_object_result(
                    diagnostic_operation_id,
                    &target_digest,
                    path,
                    "deleted_remotely",
                );
            }

            // 3. sink.set_target_phase("commit") — 标记该 target 进入 Commit 阶段。
            if let Some(ref sink) = progress_sink {
                sink.set_target_phase(
                    &planned.target.remote_prefix,
                    planned.project_id.as_deref(),
                    "commit",
                );
            }

            // 4. Commit（短写锁）— 该 target 的 state/conflicts 成为 live 终态。
            let commit_phase = SyncDiagnosticPhase::start_with_provider(
                diagnostic_operation_id,
                "commit",
                Some(target_digest.clone()),
                provider,
            );
            let (target_with_conflicts, target_committed_paths) = self.commit_transferred_target(
                target_index,
                prepared_failure.is_some(),
                &staging_runs,
                &target_sync_result,
            );
            let commit_outcome =
                crate::api::types::sync_status_to_wire(&target_with_conflicts.result.status);
            commit_phase.finish_with_provider(&commit_outcome, provider);

            // Store 协调与索引失效已在 commit_target_after_transfer 内的
            // 同一写锁持有期完成（问题1+2），不再在循环体中单独执行。

            // 5. sink.update_target_finish — 标记该 target 已完成（phase 清空）。
            if let Some(ref sink) = progress_sink {
                let finished = u32::try_from(target_index + 1).unwrap_or(u32::MAX);
                sink.update_target_finish(
                    &planned.target.remote_prefix,
                    planned.project_id.as_deref(),
                    finished,
                    total_targets,
                );
            }

            // cleanup staging run — 之后整轮收口不再碰这个 target 的 staging。
            if let Some(run) = staging_runs.get(target_index) {
                run.cleanup();
            }

            // target_progress callback — 此时该 target 的 state/conflicts 已经成为
            // live 最终状态。平台层收到回调时冲突已是 live 最终值，用户立即 resolve
            // 不会被后续 Commit 覆盖。
            if let Some(cb) = target_progress {
                let progress_status =
                    crate::api::types::sync_status_to_wire(&target_with_conflicts.result.status);
                let progress_conflict_count =
                    u32::try_from(target_with_conflicts.result.conflicts.len()).unwrap_or(u32::MAX);
                let progress_target_kind =
                    plan.targets[target_index].target_kind.as_target_kind_str();
                cb(crate::sync::full_sync::SyncTargetProgress {
                    project_id: plan.targets[target_index].project_id.clone(),
                    target_kind: progress_target_kind.to_string(),
                    status: progress_status,
                    conflict_count: progress_conflict_count,
                });
            }

            let target_status =
                crate::api::types::sync_status_to_wire(&target_with_conflicts.result.status);
            let mut target_end_fields =
                sync_diagnostic_fields(diagnostic_operation_id, "complete", Some(&target_digest));
            target_end_fields.insert(
                "target_kind".to_string(),
                serde_json::Value::String(target_kind),
            );
            target_end_fields.insert(
                "target_index".to_string(),
                serde_json::Value::Number(serde_json::Number::from(
                    u64::try_from(target_index).unwrap_or(u64::MAX),
                )),
            );
            target_end_fields.insert(
                "total_targets".to_string(),
                serde_json::Value::Number(serde_json::Number::from(total_targets)),
            );
            target_end_fields.insert(
                "status_code".to_string(),
                serde_json::Value::String(target_status),
            );
            target_end_fields.insert(
                "duration_ms".to_string(),
                serde_json::Value::Number(serde_json::Number::from(
                    u64::try_from(target_started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
                )),
            );
            if let Some(target_id_digest) = target_id_digest {
                target_end_fields.insert(
                    "target_id_digest".to_string(),
                    serde_json::Value::String(target_id_digest),
                );
            }
            if let Some(remote_revision) = remote_revision {
                target_end_fields.insert(
                    "remote_revision".to_string(),
                    serde_json::Value::String(remote_revision.to_string()),
                );
            }
            if let Some(active_generation) = active_generation.as_deref() {
                target_end_fields.insert(
                    "remote_generation_digest".to_string(),
                    serde_json::Value::String(sync_target_digest(active_generation)),
                );
            }
            let sync_result = &target_with_conflicts.result;
            let uploaded = sync_result.uploaded_files.len();
            let downloaded = sync_result.downloaded_files.len();
            let local_deleted = sync_result.local_deletes.len();
            let remote_deleted = sync_result.remote_deletes.len();
            let overwritten = sync_result.overwritten_files.len();
            let ignored = sync_result.ignored_files.len();
            let conflicts = sync_result.conflicts.len();
            for (name, value) in [
                ("uploaded", uploaded),
                ("downloaded", downloaded),
                ("local_deleted", local_deleted),
                ("remote_deleted", remote_deleted),
                ("overwritten", overwritten),
                ("ignored", ignored),
                ("conflicts", conflicts),
            ] {
                target_end_fields.insert(
                    name.to_string(),
                    serde_json::Value::Number(serde_json::Number::from(
                        u64::try_from(value).unwrap_or(u64::MAX),
                    )),
                );
            }
            let decision_summary = if conflicts > 0 {
                "conflict"
            } else if downloaded > 0 || remote_deleted > 0 {
                "remote_change_applied"
            } else if uploaded > 0 || local_deleted > 0 {
                "local_change_published"
            } else if overwritten > 0 {
                "overwritten"
            } else if ignored > 0 {
                "ignored_or_reused"
            } else {
                "unchanged"
            };
            target_end_fields.insert(
                "lww_decision".to_string(),
                serde_json::Value::String(decision_summary.to_string()),
            );
            target_end_fields.insert(
                "state_source".to_string(),
                serde_json::Value::String("live_after_commit".to_string()),
            );
            target_end_fields.insert(
                "commit_modified_live".to_string(),
                serde_json::Value::Bool(!target_committed_paths.is_empty()),
            );
            record_sync_diagnostic("sync.target.end", "sync.target", target_end_fields);
            all_targets.push(target_with_conflicts);
            all_committed_paths.extend(target_committed_paths.into_iter().map(|path| path.path));
        }

        // 每个 target 的 commit 已在同一写锁内完成 Store 协调与索引失效（问题2）。
        // 此处再失效一次是冗余但安全的兜底，覆盖部分 target 成功、后续取消的场景。
        self.core_write().invalidate_path_dependency_index();

        // Issue #779 评论 5853718466：generation GC 不再在用户同步阻塞路径内执行。
        // 原实现在此处同步调 generation GC，GC 失败会被 finalize_full_sync 升级为
        // RecoverableError，且 GC 阻塞 perform_full_sync 返回，导致 Qt 后台线程无法
        // 及时回调 outcome，UI 一直显示 syncing。
        // 现在改为：perform_full_sync 在拿到终态后 spawn 后台线程执行 GC maintenance
        // （fire-and-forget），GC 失败只 log warn，不影响同步终态。

        // Issue #729：整轮结束后检查取消令牌。
        if Self::sync_cancelled(cancellation_token.as_ref()) {
            log::info!(
                "[sync] perform_full_sync: cancellation requested after per-target loop — persisting cancelled state"
            );
            self.core_write().persist_full_sync_cancelled();
            return Ok(Self::full_sync_noop_result());
        }

        // Issue #763 评论 5831610228：finalize 前用 set_global_phase("commit") 清掉
        // 单一 current target，表示整轮进入全局 Commit 收口，没有单一 current target。
        if let Some(ref sink) = progress_sink {
            sink.set_global_phase("commit");
        }

        // Phase 5: 聚合 + lifecycle 处理 + FullSyncState 持久化（短写锁）。
        // Issue #779 评论 5853718466：传 None — GC 失败不影响同步终态。
        let (result, committed_paths, lifecycle_receipts) = {
            let core = self.core_write();
            core.finalize_full_sync(all_targets, None)
        };

        // 合并 per-target committed_paths 与 finalize 阶段返回的 committed_paths。
        let mut all_paths = all_committed_paths;
        all_paths.extend(committed_paths);

        // 用 commit 阶段返回的 committed_paths
        // 精确 stage，替代全量 &[] 扫描。committed_paths 是 workspace-relative paths。
        // 空 committed_paths 不触发全量扫描（record_workspace_paths_history
        // 空 paths 直接返回空结果）。
        let local_content_changed = !all_paths.is_empty();
        self.record_workspace_paths_history(&all_paths, "full_sync_commit");

        // 处理 RemoteLifecycle 删除事务的 receipts。
        // 恢复单一 durable 路线 —
        // 对每个 receipt：用 change_set 调 record_workspace_change_set_history 记本地 history，
        // 成功后才调 ack_project_delete_history 推进 journal。
        // history 失败 → 不 ack → journal 保留 StarMapsUnbound → bootstrap/recover 下次补记。
        for receipt in &lifecycle_receipts {
            self.process_lifecycle_receipt(receipt);
        }

        let mut result: FullSyncResultDto = result.into();
        result.local_content_changed = local_content_changed;
        Ok(result)
    }

    /// 取消令牌是否已请求取消。`None`（未提供令牌）永远不取消。
    fn sync_cancelled(token: Option<&SyncCancellationToken>) -> bool {
        token.is_some_and(SyncCancellationToken::is_cancelled)
    }

    /// 单个 target 的 Commit（短写锁）— 把 staging 的三方结果写进 live 终态，
    /// 并把该 target 的未解决冲突写进 live 的 `state.local.json / conflicts.json`。
    ///
    /// progress 必须在**本函数返回之后**才发：返回时该 target 的冲突状态已经是 live
    /// 最终值，用户收到 progress 立刻 resolve 不会被后续 Commit 覆盖。
    ///
    /// 返回 `(冲突已合并进结果的 target 结果, workspace-relative committed paths)`。
    /// 把 `record_staging_conflicts` 的结果合并进 target 结果。
    /// 从 `commit_target_after_transfer` 提取以控制 `with_sync_state_lock` 闭包嵌套深度
    /// （Issue #762 评论 5834136935）。
    fn apply_staging_conflicts_to_target(
        target_with_conflicts: &mut crate::sync::types::TargetSyncResult,
        run: &crate::sync::staging::StagingRun,
        target_sync_result: &crate::sync::types::TargetSyncResult,
        conflicts: &[crate::sync::staging::StagingConflict],
    ) {
        if conflicts.is_empty() {
            return;
        }
        let existing_conflicts = target_sync_result.result.conflicts.clone();
        match crate::sync::conflict::record_staging_conflicts_for_group(
            run.target_live_root(),
            &target_sync_result.remote_prefix,
            conflicts,
            &existing_conflicts,
            run.app_group(),
        ) {
            Ok(merged) => {
                target_with_conflicts.result.conflicts = merged;
                target_with_conflicts.result.status = crate::sync::SyncStatus::Conflict;
            }
            Err(e) => {
                target_with_conflicts.result.status = crate::sync::SyncStatus::RecoverableError(
                    format!("staging_conflict_persist_failed: {}", e),
                );
                target_with_conflicts.result.error =
                    Some(format!("failed to persist staging conflicts: {}", e));
            }
        }
    }

    /// Commit 已 Transfer 的 target；Prepare 阶段失败的目标跳过 Commit（#955）。
    fn commit_transferred_target(
        &self,
        target_index: usize,
        prepared_failure: bool,
        staging_runs: &[crate::sync::staging::StagingRun],
        target_sync_result: &crate::sync::types::TargetSyncResult,
    ) -> (
        crate::sync::types::TargetSyncResult,
        Vec<crate::sync::commit_helpers::CommittedPath>,
    ) {
        if prepared_failure {
            // 迁移失败时 staging 未 seed，本组本次不上传也不 Commit。
            return (target_sync_result.clone(), Vec::new());
        }
        match staging_runs.get(target_index) {
            Some(run) => self.commit_target_after_transfer(run, target_sync_result),
            None => (target_sync_result.clone(), Vec::new()),
        }
    }

    fn commit_target_after_transfer(
        &self,
        run: &crate::sync::staging::StagingRun,
        target_sync_result: &crate::sync::types::TargetSyncResult,
    ) -> (
        crate::sync::types::TargetSyncResult,
        Vec<crate::sync::commit_helpers::CommittedPath>,
    ) {
        // Issue #762 评论 5834136935：跨 WriterCoreApi 实例串行化同一 sync root 的
        // state/conflict mutation。整段 Commit（三方 merge + SaveTransaction 写回 +
        // record_staging_conflicts）持有该 project root 的共享锁，防止 UI resolve
        // 在 Commit 过程中写入后被旧 merged state 覆盖。
        crate::sync::state_lock::with_sync_state_lock(run.target_live_root(), || {
            let core = self.core_write();

            // 问题1：commit 前 flush 所有 Store dirty 到 live 磁盘，
            // 保证三方合并的 ours（live）包含用户在 Transfer 期间的编辑。
            // flush 失败则注入 target 错误状态，不继续 commit（避免基于过时 live 做合并）。
            if let Err(e) = core.flush_all_starmap_stores() {
                let mut target_with_conflicts = target_sync_result.clone();
                target_with_conflicts.result.status = crate::sync::SyncStatus::RecoverableError(
                    format!("pre_commit_flush_failed: {}", e),
                );
                target_with_conflicts.result.error = Some(format!(
                    "flush_all_starmap_stores before commit failed: {}",
                    e
                ));
                return (target_with_conflicts, Vec::new());
            }

            let sync_sources =
                staging_manifest_device_ids(run, target_sync_result.project_id.as_deref());
            let (commit_result, conflicts, committed_paths) =
                crate::sync::commit_helpers::commit_single_target_staging(run, target_sync_result);
            let mut target_with_conflicts = target_sync_result.clone();

            Self::apply_staging_conflicts_to_target(
                &mut target_with_conflicts,
                run,
                target_sync_result,
                &conflicts,
            );

            // commit 失败注入错误状态。
            if let crate::sync::commit_helpers::TargetCommitResult::Failed(msg) = &commit_result {
                crate::sync::commit_helpers::apply_commit_failure_to_target(
                    &mut target_with_conflicts,
                    msg,
                );
            }

            // Issue #1008：同步落盘的正文变化是明确的 SyncRemote 事实，不能等
            // 下次保存时被统归为 Unknown 外部变更。事件以 Core 当前证明链头为
            // before，Apply 读取刚写入 projects_root 的实际正文；Delete 记录真实
            // 删除事实。证明落盘失败必须反映到该 target，不能只打 warn 后假装成功。
            if matches!(
                commit_result,
                crate::sync::commit_helpers::TargetCommitResult::Ok
            ) {
                let mut proof_errors: Vec<String> = Vec::new();
                for (action_index, committed) in committed_paths.iter().enumerate() {
                    let path = &committed.path;
                    let Some((project_id, volume_id, chapter_id)) = chapter_path_parts(path) else {
                        continue;
                    };
                    let before_text = match &committed.before {
                        crate::sync::commit_helpers::CommittedBefore::Missing => None,
                        crate::sync::commit_helpers::CommittedBefore::Text(bytes) => {
                            match std::str::from_utf8(bytes) {
                                Ok(text) => Some(text.to_string()),
                                Err(e) => {
                                    proof_errors.push(format!(
                                        "同步章节前置正文不是 UTF-8 {}: {}",
                                        path.display(),
                                        e
                                    ));
                                    continue;
                                }
                            }
                        }
                        crate::sync::commit_helpers::CommittedBefore::Error(e) => {
                            proof_errors.push(e.clone());
                            continue;
                        }
                        crate::sync::commit_helpers::CommittedBefore::NotTracked => {
                            proof_errors
                                .push(format!("同步章节缺少 commit 前置快照: {}", path.display()));
                            continue;
                        }
                    };
                    let after_text = if committed.deleted {
                        None
                    } else {
                        let Some(live_path) = chapter_live_path(&core, path) else {
                            proof_errors.push(format!("无法映射同步章节路径: {}", path.display()));
                            continue;
                        };
                        match std::fs::read_to_string(&live_path) {
                            Ok(text) => Some(text),
                            Err(e) => {
                                proof_errors.push(format!(
                                    "读取同步章节 {} 失败: {}",
                                    live_path.display(),
                                    e
                                ));
                                continue;
                            }
                        }
                    };
                    let chapter_key = format!("{project_id}/{volume_id}/{chapter_id}");
                    let source_device = sync_sources
                        .get(&chapter_key)
                        .map(String::as_str)
                        .unwrap_or("");
                    // run_id + 落盘路径 + 动作序号唯一标识一次真实同步提交；同一
                    // 章节 A→B→A→B 不会被内容 hash 误去重。事件 ID 固定后重发复用。
                    let sync_event_id =
                        format!("sync:{}:{}:{}", run.run_id(), path.display(), action_index);
                    if let Err(e) = core.record_sync_content_change(
                        &project_id,
                        &volume_id,
                        &chapter_id,
                        before_text.as_deref(),
                        after_text.as_deref(),
                        &sync_event_id,
                        source_device,
                        "",
                    ) {
                        proof_errors.push(format!("{}: {e}", path.display()));
                    }
                }
                if !proof_errors.is_empty() {
                    target_with_conflicts.result.status = crate::sync::SyncStatus::RecoverableError(
                        "sync_proof_record_failed".to_string(),
                    );
                    target_with_conflicts.result.error = Some(format!(
                        "同步正文已落盘，但证明事件记录失败: {}",
                        proof_errors.join("; ")
                    ));
                }
            }

            // 同一写锁持有期内完成 Store 协调与索引失效（问题1+2）：
            // commit 前已 flush Store dirty 到 live，此处只 evict 变化的星图 Store 并失效索引。
            // 协调失败注入 target 错误状态（问题2），不假装成功。
            if let Err(e) = Self::coordinate_stores_after_commit(&core, &committed_paths) {
                target_with_conflicts.result.status = crate::sync::SyncStatus::RecoverableError(
                    format!("store_coordination_failed: {}", e),
                );
                target_with_conflicts.result.error =
                    Some(format!("store coordination after commit failed: {}", e));
            }

            (target_with_conflicts, committed_paths)
        })
    }

    /// 在已持有 Core 写锁时，按 `committed_paths` 协调 Store 并失效派生索引。
    ///
    /// commit 前已 flush 所有 Store dirty（问题1），此处只需 evict 变化的星图 Store
    ///（移除缓存，下次访问从 live 磁盘重新加载合并后的结果）并失效路径依赖索引。
    /// 返回 Result，失败时传播错误到 target 终态（问题2），不吞成 warn 后假装成功。
    fn coordinate_stores_after_commit(
        core: &crate::facade::WriterCore,
        committed_paths: &[crate::sync::commit_helpers::CommittedPath],
    ) -> crate::error::Result<()> {
        let changed_starmap_ids: std::collections::HashSet<String> = committed_paths
            .iter()
            .filter_map(|p| extract_starmap_id_from_committed_path(&p.path))
            .collect();
        for starmap_id in &changed_starmap_ids {
            // 问题1：不再 flush dirty（已在 commit 前 flush），只 evict Store。
            // 问题2：evict 失败传播错误，不 log::warn 后继续。
            core.evict_starmap_store_after_commit(starmap_id)?;
        }
        core.invalidate_path_dependency_index();
        Ok(())
    }

    /// Issue #779 评论 5854082763：generation GC maintenance 独立入口。
    ///
    /// 主同步（perform_full_sync）拿到终态后立即返回，GC 由平台层通过此方法
    /// 异步调用。此方法有自己独立的 cancellation token，不复用用户同步 token。
    ///
    /// 内部：创建 provider → discover catalog → 对每个 project target 调 run_generation_gc。
    /// GC 失败只 log warn，不传播（下一轮自然再清）。
    ///
    /// Issue #779 评论 5854912227：maintenance 用 [`create_sync_provider_for_maintenance`]
    /// 创建 provider，**不调用 `persist_full_sync_early_failure`**，不触碰磁盘
    /// `FullSyncState`。generation GC 失败只能作为维护错误，不能把正文同步重新
    /// 变成失败。后续 discover catalog 失败、`run_generation_gc` 失败、网络/认证/
    /// 权限失败都只 log warn / 返回 maintenance 错误，不调任何
    /// `persist_full_sync_*failure`。
    #[allow(clippy::excessive_nesting)]
    pub fn perform_generation_gc_maintenance(
        &self,
        config: SyncConfigDto,
        cancellation_token: Option<SyncCancellationToken>,
    ) -> ApiResult<()> {
        let sync_config: crate::sync::SyncConfig = config.into();
        if !sync_config.enabled {
            return Ok(());
        }
        if let Some(ref token) = cancellation_token {
            if token.is_cancelled() {
                return Ok(());
            }
        }
        let secrets = self.secrets_override_snapshot().unwrap_or_default();
        //   用无副作用 provider factory：transport 初始化失败只返回 Err，
        // 不调 persist_full_sync_early_failure，不污染用户同步终态（Issue #779 评论 5854912227）。
        let (provider, app_data_root) = {
            let core = self.core_write();
            (
                core.create_sync_provider_for_maintenance(&sync_config, &secrets)?,
                core.app_data_root.clone(),
            )
        };
        let remote_catalog_snapshot =
            match crate::sync::target_lifecycle::discover_legacy_remote_catalog(provider.as_ref()) {
                Ok(snapshot) => snapshot,
                Err(e) => {
                    log::warn!("[sync] gc_maintenance: discover catalog failed: {e}");
                    return Err(e.into());
                }
            };
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut first_gc_error = None;
        for record in &remote_catalog_snapshot.catalog.records {
            if !record.remote_prefix.starts_with("projects/") {
                continue;
            }
            // Issue #779 评论 5854511049：不再只处理 Upsert。
            // - Upsert：传当前 active_generation 作为保护。
            // - Delete：同样调 run_generation_gc，active hint 传 None（作品已删除，
            //   没有需要保护的 active generation，所有超过 retention 的 generation 都可清）。
            //   delete_all_remote_objects 故意跳过 __generations__，留给 GC 在 retention
            //   到期后清；若 maintenance 再跳过 Delete，这些旧 generation 将永久留在远端。
            //   仍由 run_generation_gc 自己执行 retention、incomplete lease、删除前 fresh
            //   catalog 二次确认这些安全规则。
            let active_hint = match record.op {
                crate::sync::types::TargetOp::Upsert => record.active_generation.as_deref(),
                crate::sync::types::TargetOp::Delete => None,
            };
            if let Some(ref token) = cancellation_token {
                if token.is_cancelled() {
                    log::info!("[sync] gc_maintenance: cancellation requested — stopping");
                    return Ok(());
                }
            }
            if let Err(e) = crate::sync::generation_gc::run_generation_gc(
                provider.as_ref(),
                &record.remote_prefix,
                active_hint,
                now_ms,
                crate::sync::generation_gc::GENERATION_RETENTION_MS,
                cancellation_token.as_ref(),
            ) {
                log::warn!(
                    "[sync] gc_maintenance: GC failed for {}: {e}",
                    record.remote_prefix
                );
                if first_gc_error.is_none() {
                    first_gc_error = Some(e);
                }
            }
        }
        if let Some(token) = cancellation_token.as_ref() {
            if token.is_cancelled() {
                log::info!("[sync] gc_maintenance: cancellation requested — skipping maintenance timestamp");
                return Ok(());
            }
        }
        if let Some(error) = first_gc_error {
            return Err(error.into());
        }
        crate::sync::generation_gc::save_maintenance_last_success_at(
            &app_data_root,
            chrono::Utc::now().timestamp(),
        )?;
        Ok(())
    }

    /// 与 sync disabled 相同的 no-op `FullSyncResult`。
    ///
    /// Issue #729：Prepare / Seed / 编排各阶段边界检查取消令牌后复用，避免重复构造。
    fn full_sync_noop_result() -> FullSyncResultDto {
        crate::sync::types::FullSyncResult {
            overall_status: crate::sync::SyncStatus::Success,
            targets: Vec::new(),
            total_uploaded: 0,
            total_downloaded: 0,
            total_local_deletes: 0,
            total_remote_deletes: 0,
            total_overwritten: 0,
            total_ignored: 0,
            total_conflicts: 0,
            error: None,
            error_category: None,
            message_key: None,
        }
        .into()
    }

    ///   处理单个 lifecycle receipt — history ack。
    ///
    /// history 成功 → ack 推进 journal；history 失败 → 不 ack → journal 保留 → 下次补记。
    fn process_lifecycle_receipt(&self, receipt: &crate::sync::types::LocalLifecycleCommitReceipt) {
        match self
            .record_workspace_change_set_history(&receipt.change_set, "remote_lifecycle_delete")
        {
            Ok(()) => {
                if let Err(e) = crate::storage::journal::project_delete::ack_project_delete_history(
                    &self.app_data_root,
                    &receipt.journal_token,
                ) {
                    log::warn!(
                        "[sync] perform_full_sync: ack_project_delete_history failed \
                         for {}: {} — journal retained",
                        receipt.journal_token,
                        e
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "[sync] perform_full_sync: record_workspace_change_set_history failed \
                     for {}: {} — journal retained, will retry on recover",
                    receipt.journal_token,
                    e
                );
            }
        }
    }

    /// 冲突解决：保留本地版本。
    ///
    /// `target_kind` 标识冲突所属的同步 target（`"app"` / `"project"`），
    /// `project_id` 在 `"project"` target 下必填，`"app"` target 下传 `None`。
    pub fn resolve_conflict_keep_local(
        &self,
        target_kind: &str,
        project_id: Option<&str>,
        path: &str,
    ) -> ApiResult<bool> {
        // Issue #762 评论 5834136935：跨 WriterCoreApi 实例串行化同一 target root 的
        // state/conflict mutation。resolve 拿同一把 root lock，与 Commit 互斥：
        // resolve 先拿锁 → Commit 后读 live 时能看到已解决的状态；
        // Commit 先拿锁 → resolve 等 Commit 完成，再基于最终 live 状态解决。
        let lock_root = self.conflict_lock_root(target_kind, project_id)?;
        crate::sync::state_lock::with_sync_state_lock(&lock_root, || {
            self.core_write()
                .resolve_conflict_keep_local(target_kind, project_id, path)
                .map(|_| true)
                .map_err(Into::into)
        })
    }

    /// 冲突解决：采用远端版本。
    ///
    /// 返回 `applied_live` bool：`true` 表示已立即修改 live 正文（snapshot 替换/移入 trash），
    /// 平台层据此触发编辑器重载；`false` 表示仅排队 pending_take_remote（老数据兼容）。
    ///
    /// `target_kind` 标识冲突所属的同步 target（`"app"` / `"project"`），
    /// `project_id` 在 `"project"` target 下必填，`"app"` target 下传 `None`。
    ///
    /// 星图冲突（`path` 以 `starmaps/` 开头）解决后 evict 对应 Store 缓存并失效
    /// 路径依赖索引，避免内存中持有已过期的 Store / 索引（Issue #902 第十九轮问题1）。
    pub fn resolve_conflict_take_remote(
        &self,
        target_kind: &str,
        project_id: Option<&str>,
        path: &str,
    ) -> ApiResult<bool> {
        // 冲突事务开始前协调 dirty Store，避免脏 Store 覆盖冲突解决结果。
        self.core_write().flush_all_starmap_stores()?;

        // Issue #762 评论 5834136935：跨 WriterCoreApi 实例串行化同一 target root 的
        // state/conflict mutation。resolve 拿同一把 root lock，与 Commit 互斥：
        // resolve 先拿锁 → Commit 后读 live 时能看到已解决的状态；
        // Commit 先拿锁 → resolve 等 Commit 完成，再基于最终 live 状态解决。
        let lock_root = self.conflict_lock_root(target_kind, project_id)?;
        let applied = crate::sync::state_lock::with_sync_state_lock(&lock_root, || {
            self.core_write()
                .resolve_conflict_take_remote(target_kind, project_id, path)
                .map_err(crate::api::error::WriterError::from)
        })?;

        // 星图冲突解决后 evict Store + 失效路径依赖索引，与同步 commit 后
        // `coordinate_stores_after_commit` 一致：commit 前已 flush dirty（上面已做），
        // 此处只 evict 变化的星图 Store（移除缓存，下次访问从 live 磁盘重新加载
        // 合并后的结果）并失效路径依赖索引。
        if applied && is_starmap_conflict_path(path) {
            if let Some(starmap_id) = extract_starmap_id_from_conflict_path(path) {
                let core = self.core_write();
                core.evict_starmap_store_after_commit(&starmap_id)?;
                core.invalidate_path_dependency_index();
            }
        }
        Ok(applied)
    }

    /// 冲突解决：标记为已合并。
    ///
    /// `target_kind` 标识冲突所属的同步 target（`"app"` / `"project"`），
    /// `project_id` 在 `"project"` target 下必填，`"app"` target 下传 `None`。
    pub fn resolve_conflict_mark_merged(
        &self,
        target_kind: &str,
        project_id: Option<&str>,
        path: &str,
    ) -> ApiResult<bool> {
        // Issue #762 评论 5834136935：跨 WriterCoreApi 实例串行化同一 target root 的
        // state/conflict mutation。resolve 拿同一把 root lock，与 Commit 互斥：
        // resolve 先拿锁 → Commit 后读 live 时能看到已解决的状态；
        // Commit 先拿锁 → resolve 等 Commit 完成，再基于最终 live 状态解决。
        let lock_root = self.conflict_lock_root(target_kind, project_id)?;
        crate::sync::state_lock::with_sync_state_lock(&lock_root, || {
            self.core_write()
                .resolve_conflict_mark_merged(target_kind, project_id, path)
                .map(|_| true)
                .map_err(Into::into)
        })
    }

    /// 加载冲突预览 — 返回本地/远端内容供平台层展示。
    ///
    /// 平台层只拿此 DTO，不直接读 `conflicts.json`，QML 更不能自己拼磁盘路径。
    ///
    /// `target_kind` 标识冲突所属的同步 target（`"app"` / `"project"`），
    /// `project_id` 在 `"project"` target 下必填，`"app"` target 下传 `None`。
    pub fn load_sync_conflict_preview(
        &self,
        target_kind: &str,
        project_id: Option<&str>,
        path: &str,
    ) -> ApiResult<SyncConflictPreviewDto> {
        let preview = self
            .core_read()
            .load_sync_conflict_preview(target_kind, project_id, path)
            .map_err(crate::api::error::WriterError::from)?;
        Ok(SyncConflictPreviewDto {
            path: preview.path,
            kind: sync_conflict_kind_to_wire(&preview.kind),
            created_at: preview.created_at,
            local_content: preview.local_content,
            remote_content: preview.remote_content,
            remote_deleted: preview.remote_deleted,
        })
    }

    /// 列出指定 target 的所有冲突记录。
    ///
    /// `target_kind` 标识同步 target（`"app"` / `"project"`），
    /// `project_id` 在 `"project"` target 下必填，`"app"` target 下传 `None`。
    pub fn list_sync_conflicts(
        &self,
        target_kind: &str,
        project_id: Option<&str>,
    ) -> ApiResult<Vec<SyncConflictDto>> {
        self.core_read()
            .list_sync_conflicts(target_kind, project_id)
            .map(|conflicts| conflicts.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    /// 列出所有 target 的所有冲突（聚合），只返回摘要条目，不含正文/快照。
    ///
    /// 诊断包用（Issue #763）。覆盖 App target 和所有 Project target，
    /// 单 target 读取失败不阻断全局聚合，跳过该 target 继续。
    /// 每个条目带 `target_kind` 标识来源（Issue #902 第十九轮问题1）。
    pub fn list_all_sync_conflicts(&self) -> ApiResult<Vec<AllSyncConflictEntryDto>> {
        self.core_read()
            .list_all_sync_conflicts()
            .map_err(Into::into)
    }

    /// 检查同步能力——综合 config 和 secrets 判断是否可执行全量同步。
    pub fn get_sync_capability(&self) -> ApiResult<SyncCapabilityDto> {
        let config = self.load_sync_config()?;
        let secrets = self.load_sync_secrets()?;

        let mut block_reason_code = None;
        let mut block_message_key = None;
        let message_args = std::collections::HashMap::new();
        let mut can_run = true;

        // 从 provider_config 读 remote_url。
        let remote_url = config
            .provider_config
            .as_ref()
            .map(|pc| match pc {
                crate::api::ProviderConfigDto::GitHttps { remote_url, .. } => remote_url.clone(),
                crate::api::ProviderConfigDto::GitHub { remote_url, .. } => remote_url.clone(),
            })
            .unwrap_or_default();
        // 从 provider_secrets 读 token。
        let token = secrets
            .provider_secrets
            .as_ref()
            .map(|ps| match ps {
                crate::api::ProviderSecretsDto::GitHttps { secret } => secret.clone(),
                crate::api::ProviderSecretsDto::GitHub { token } => token.clone(),
            })
            .unwrap_or_default();

        if !config.enabled {
            can_run = false;
            block_reason_code = Some("DISABLED".to_string());
            block_message_key = Some("sync.block.disabled".to_string());
        } else if self.core_read().secure_storage.is_none() {
            can_run = false;
            block_reason_code = Some("SECURE_STORAGE_UNAVAILABLE".to_string());
            block_message_key = Some("sync.block.secure_storage_unavailable".to_string());
        } else if remote_url.is_empty() {
            can_run = false;
            block_reason_code = Some("REMOTE_URL_MISSING".to_string());
            block_message_key = Some("sync.block.remote_url_missing".to_string());
        } else if token.is_empty() {
            can_run = false;
            block_reason_code = Some("TOKEN_MISSING".to_string());
            block_message_key = Some("sync.block.token_missing".to_string());
        }

        Ok(SyncCapabilityDto {
            can_run,
            block_reason_code,
            block_message_key,
            message_args,
        })
    }

    /// 根据 target kind 解析冲突操作的 state lock root。
    ///
    /// 与 facade `WriterCore::conflict_target_root` 一致：
    /// - `"app"` → `app_data_root`（星图等应用级数据）；
    /// - `"project"` → `projects_root/project_id`（作品正文，`project_id` 必填）。
    ///
    /// 锁根与 sync root 同路径，保证 resolve 与同 target 的 Commit 互斥
    /// （Issue #762 评论 5834136935）。
    fn conflict_lock_root(
        &self,
        target_kind: &str,
        project_id: Option<&str>,
    ) -> ApiResult<std::path::PathBuf> {
        match target_kind {
            "app" => Ok(self.app_data_root.clone()),
            "project" => {
                let pid = project_id.ok_or_else(|| {
                    crate::api::error::WriterError::Other(
                        "conflict target 'project' requires project_id".into(),
                    )
                })?;
                Ok(self.projects_root.join(pid))
            }
            _ => Err(crate::api::error::WriterError::Other(format!(
                "unknown conflict target kind: {}",
                target_kind
            ))),
        }
    }
}

/// 从 workspace-relative 路径提取星图 ID。
///
/// 路径格式：
/// - `starmaps/{id}.meta.json` → `Some(id)`
/// - `starmaps/{id}/...` → `Some(id)`
/// - `starmaps/index.json` → `None`（不对应特定星图）
/// - 其他路径 → `None`
///
/// 同步 per-target commit 后用此函数从 `committed_paths` 提取发生变化的星图 ID，
/// 以便移除内存中已过期的 Store（问题2）。
fn extract_starmap_id_from_committed_path(path: &std::path::Path) -> Option<String> {
    let rel = path.to_str()?;
    let rest = rel.strip_prefix("starmaps/")?;
    if rest == "index.json" {
        return None;
    }
    // starmaps/{id}.meta.json
    if let Some(id) = rest.strip_suffix(".meta.json") {
        return Some(id.to_string());
    }
    // starmaps/{id}/...
    if let Some(slash_idx) = rest.find('/') {
        return Some(rest[..slash_idx].to_string());
    }
    None
}

/// 冲突路径是否指向星图数据（`starmaps/` 前缀）。
///
/// `resolve_conflict_take_remote` 据此判断解决后是否需要 evict Store +
/// 失效路径依赖索引（Issue #902 第十九轮问题1）。
fn is_starmap_conflict_path(path: &str) -> bool {
    path.starts_with("starmaps/")
}

/// 从冲突路径（workspace-relative 字符串）提取星图 ID。
///
/// 与 `extract_starmap_id_from_committed_path` 同语义，但接受 `&str` 而非 `&Path`，
/// 供 `resolve_conflict_take_remote` 直接使用冲突 DTO 里的 `path` 字段。
fn extract_starmap_id_from_conflict_path(path: &str) -> Option<String> {
    extract_starmap_id_from_committed_path(std::path::Path::new(path))
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "github-api")]
    use super::*;
    #[cfg(feature = "github-api")]
    use tempfile::tempdir;

    #[test]
    #[cfg(feature = "github-api")]
    fn test_load_sync_secrets_global() {
        let temp_dir = tempdir().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
        let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));

        // Test loading when no secrets exist (should return default/empty struct)
        let loaded_empty = api.load_sync_secrets().unwrap();
        assert!(loaded_empty.provider_secrets.is_none());

        // Save some dummy secrets
        let dummy_secrets = SyncSecretsDto {
            provider_secrets: Some(ProviderSecretsDto::GitHub {
                token: "ghp_dummy123".to_string(),
            }),
        };
        api.save_sync_secrets(dummy_secrets.clone()).unwrap();

        // Test loading the saved secrets
        let loaded_secrets = api.load_sync_secrets().unwrap();
        assert_eq!(
            loaded_secrets.provider_secrets,
            dummy_secrets.provider_secrets
        );
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn save_sync_config_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
        let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));

        let config = SyncConfigDto {
            enabled: true,
            active_provider: "github_api".to_string(),
            provider_config: Some(ProviderConfigDto::GitHub {
                remote_url: "https://github.com/test/repo.git".to_string(),
                branch: "main".to_string(),
                username: "".to_string(),
                transport: "https_token".to_string(),
            }),
            auto_sync: false,
            sync_interval_seconds: 300,
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let result = api.save_sync_config(config);
        assert!(result.unwrap());
    }

    /// `extract_starmap_id_from_committed_path` 从 workspace-relative 路径提取星图 ID。
    ///
    /// 覆盖 meta 文件、graph/节点/边文件、index.json 以及非星图路径（问题2）。
    #[test]
    fn extract_starmap_id_from_committed_path_identifies_starmap_files() {
        use super::extract_starmap_id_from_committed_path;
        use std::path::PathBuf;

        // meta 文件
        assert_eq!(
            extract_starmap_id_from_committed_path(&PathBuf::from("starmaps/sm_1.meta.json")),
            Some("sm_1".to_string())
        );
        // graph 文件
        assert_eq!(
            extract_starmap_id_from_committed_path(&PathBuf::from("starmaps/sm_1/graph.json")),
            Some("sm_1".to_string())
        );
        // 节点文件（嵌套目录）
        assert_eq!(
            extract_starmap_id_from_committed_path(&PathBuf::from(
                "starmaps/sm_1/nodes/00/n1.json"
            )),
            Some("sm_1".to_string())
        );
        // 边文件
        assert_eq!(
            extract_starmap_id_from_committed_path(&PathBuf::from("starmaps/sm_1/edges/e1.json")),
            Some("sm_1".to_string())
        );
        // index.json 不对应特定星图
        assert_eq!(
            extract_starmap_id_from_committed_path(&PathBuf::from("starmaps/index.json")),
            None
        );
        // 非星图路径
        assert_eq!(
            extract_starmap_id_from_committed_path(&PathBuf::from("projects/p1/chapter.json")),
            None
        );
        // starmaps 前缀但无后续分隔
        assert_eq!(
            extract_starmap_id_from_committed_path(&PathBuf::from("starmaps/")),
            None
        );
    }
}

/// 从 workspace-relative 章节正文路径解析 `(project, volume, chapter)`。
fn chapter_path_parts(path: &std::path::Path) -> Option<(String, String, String)> {
    let parts: Vec<String> = path
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(ToString::to_string))
        .collect();
    let project = parts.iter().position(|part| part == "projects")?;
    let volume = parts.iter().position(|part| part == "volumes")?;
    let chapter = parts.iter().position(|part| part == "chapters")?;
    if volume + 1 >= parts.len()
        || !parts
            .get(chapter + 2)
            .is_some_and(|name| name == "chapter.md")
    {
        return None;
    }
    Some((
        parts.get(project + 1)?.clone(),
        parts.get(volume + 1)?.clone(),
        parts.get(chapter + 1)?.clone(),
    ))
}

/// 把 workspace-relative `projects/<pid>/...` 映射到独立注入的 projects_root。
///
/// 作品根与 app_data_root 可以分离，不能用 app_data_root.join(workspace_path)
/// 猜正文位置。
fn chapter_live_path(
    core: &crate::facade::WriterCore,
    workspace_path: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let mut components = workspace_path.components();
    if components.next()?.as_os_str() != "projects" {
        return None;
    }
    let project_id = components.next()?.as_os_str().to_str()?;
    Some(
        core.projects_root()
            .join(project_id)
            .join(components.as_path()),
    )
}

/// 从 staging manifest 收集 `project/volume/chapter -> 远端设备`，供 SyncRemote
/// 证明事件记录真实来源。只按 chapter_id 会在不同作品同名章节时串来源。
fn staging_manifest_device_ids(
    run: &crate::sync::staging::StagingRun,
    default_project_id: Option<&str>,
) -> std::collections::HashMap<String, String> {
    let path = run
        .staging_root()
        .join(crate::sync::lww::SYNC_MANIFEST_PATH);
    let Ok(text) = std::fs::read_to_string(path) else {
        return std::collections::HashMap::new();
    };
    let Ok(manifest) = serde_json::from_str::<crate::sync::types::SyncManifest>(&text) else {
        return std::collections::HashMap::new();
    };
    let mut out = std::collections::HashMap::new();
    for record in manifest.files {
        if record.device_id.is_empty() {
            continue;
        }
        if let Some(key) = chapter_key_from_path_str(&record.path, default_project_id) {
            out.insert(key, record.device_id);
        }
    }
    out
}

fn chapter_key_from_path_str(path: &str, default_project_id: Option<&str>) -> Option<String> {
    let parts: Vec<&str> = path.split('/').collect();
    let volumes = parts.iter().position(|part| *part == "volumes")?;
    let chapters = parts.iter().position(|part| *part == "chapters")?;
    if parts.get(chapters + 2) != Some(&"chapter.md") {
        return None;
    }
    let project_id = parts
        .iter()
        .position(|part| *part == "projects")
        .and_then(|projects| parts.get(projects + 1).copied())
        .or(default_project_id)?;
    Some(format!(
        "{}/{}/{}",
        project_id,
        parts.get(volumes + 1)?,
        parts.get(chapters + 1)?
    ))
}
