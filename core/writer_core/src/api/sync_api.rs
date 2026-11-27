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

/// Whether a target failure means the round's merged tree is incomplete and
/// must not be published.  Isolated `migration` failures (for example one
/// stats partition) do not invalidate the other groups.
fn target_blocks_round_publish(target: &crate::sync::types::TargetSyncResult) -> bool {
    if target.failure_phase.as_deref() == Some("migration") {
        return false;
    }
    matches!(
        target.result.status,
        crate::sync::SyncStatus::RecoverableError(_)
            | crate::sync::SyncStatus::FatalError(_)
            | crate::sync::SyncStatus::Error(_)
    )
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

/// 一次同步的显式运行模式。
///
/// `force_sync` 只表示"绕过周期间隔/防抖"（手动、启动、交接、前台、网络恢复），
/// 不再表示"绕过本地/远端版本比对"。真正需要重建索引或重新核对全树的场景
/// （迁移、基线缺失、用户显式要求修复）走 [`SyncRunMode::RebuildIndex`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncRunMode {
    Incremental,
    RebuildIndex,
}

impl SyncRunMode {
    /// 平台触发名 `rebuild` / `full_scan` 表示显式重建；其余一律增量。
    fn from_trigger(trigger: &str) -> Self {
        if trigger.eq_ignore_ascii_case("rebuild") || trigger.eq_ignore_ascii_case("full_scan") {
            Self::RebuildIndex
        } else {
            Self::Incremental
        }
    }
}

/// 一轮编排的实际产出。
///
/// 只返回 `FullSyncResultDto` 会让"本轮到底推送了哪个提交"丢失在内部函数里，
/// 外层只能靠重新读 HEAD 猜测（Issue #1065 复审 #1）。远端发布事实必须和
/// 结果一起返回：checkpoint 用真实推送回执推进，而不是前后 HEAD 相等。
struct RoundOutcome {
    result: FullSyncResultDto,
    /// 本轮实际创建的远端提交（未发布时为 None）。
    published_revision: Option<String>,
    /// 本轮远端发布是否成功完成（含"确认无需提交"）。只有成功发布才能确认
    /// 本地 mutation；失败/跳过时账本事实必须保持未确认（复审 #3）。
    publish_succeeded: bool,
}

fn add_fingerprint_field(hasher: &mut Sha256, key: &str, value: &[u8]) {
    hasher.update((key.len() as u64).to_le_bytes());
    hasher.update(key.as_bytes());
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

/// 对象级本地版本快照。
///
/// 只由已确认的 manifest 对象版本与持久化 mutation 账本组成；不读取
/// `state.local.json` 的运行状态字段，也不扫描磁盘。`NoChanges` 落盘的
/// `last_sync_time` 因此不会让下一轮 checkpoint 失效（Issue #1065）。
fn local_sync_version_snapshot(
    app_data_root: &std::path::Path,
    projects_root: &std::path::Path,
) -> crate::Result<crate::sync::version_snapshot::LocalVersionSnapshot> {
    crate::sync::version_snapshot::collect(app_data_root, projects_root)
}

/// 把运行时快照转换成 checkpoint 里的持久化版本。
fn to_checkpoint_version(
    version: &crate::sync::version_snapshot::LocalTargetVersion,
) -> crate::sync::full_sync_state::TargetVersionCheckpoint {
    crate::sync::full_sync_state::TargetVersionCheckpoint {
        object_fingerprint: version.object_fingerprint.clone(),
        pending_fingerprint: version.pending_fingerprint.clone(),
        pending_objects: u32::try_from(version.pending_objects).unwrap_or(u32::MAX),
        eligible: version.eligible,
    }
}

/// 章节正文与 meta 是同一业务对象的两个文件：给出另一个的路径。
fn chapter_sibling_path(path: &str) -> Option<String> {
    if let Some(prefix) = path.strip_suffix("/chapter.md") {
        return Some(format!("{prefix}/chapter.meta.json"));
    }
    if let Some(prefix) = path.strip_suffix("/chapter.meta.json") {
        return Some(format!("{prefix}/chapter.md"));
    }
    None
}

/// 从结果 DTO 取 checkpoint 键：作品看 project_id，App 组按远端前缀里的
/// storage key 反查组 id（DTO 不携带组 id）。
fn dto_target_checkpoint_key(
    target: &crate::api::types::TargetSyncResultDto,
    app_groups: &[crate::sync::types::AppDataGroup],
) -> Option<String> {
    if let Some(project_id) = target.project_id.as_deref() {
        return Some(format!("project:{project_id}"));
    }
    let prefix = target.remote_prefix.trim_end_matches('/');
    let storage_key = prefix.strip_prefix("app/__groups__/")?;
    app_groups
        .iter()
        .find(|group| group.storage_key() == storage_key)
        .map(|group| format!("app:{}", group.id()))
}

/// 远端路径 → target 相对路径（`projects/<id>/...` 与
/// `app/__groups__/<storage_key>/...` 两种前缀）。
fn target_relative_remote_path(
    remote_path: &str,
    key: &str,
    app_groups: &[crate::sync::types::AppDataGroup],
) -> Option<String> {
    let path = remote_path.replace('\\', "/");
    if let Some(project_id) = key.strip_prefix("project:") {
        return path
            .strip_prefix(&format!("projects/{project_id}/"))
            .map(str::to_string);
    }
    let group_id = key.strip_prefix("app:")?;
    let group = app_groups.iter().find(|group| group.id() == group_id)?;
    path.strip_prefix(&format!("app/__groups__/{}/", group.storage_key()))
        .map(str::to_string)
}

/// 本轮开始前各 target 的未确认 mutation 快照（path → op/hash/seq）。
///
/// 推送成功后只按这份快照确认"确实随本轮提交发布"的事实；同步期间新写入的
/// 更大序号保留为下一轮 outbox（Issue #1065 复审 #3）。
type PendingMutationMap = std::collections::BTreeMap<
    String,
    std::collections::BTreeMap<String, crate::sync::local_mutations::LocalMutationRecord>,
>;

/// checkpoint 里的 target key → 同步根，用于校验 dirty 索引。
///
/// 只遍历 checkpoint 自己记录的 target，不枚举作品目录；索引里出现未知但仍
/// 存在的 root 时由 `is_clean_for_checkpoint` 判脏。
fn checkpoint_expected_roots(
    app_data_root: &std::path::Path,
    projects_root: &std::path::Path,
    checkpoint: &crate::sync::full_sync_state::FullSyncCheckpoint,
) -> Vec<(String, std::path::PathBuf)> {
    checkpoint
        .target_versions
        .keys()
        .map(|key| {
            let root = key
                .strip_prefix("project:")
                .map(|project_id| projects_root.join(project_id))
                .unwrap_or_else(|| app_data_root.to_path_buf());
            (key.clone(), root)
        })
        .collect()
}

fn collect_preflight_pending(
    app_data_root: &std::path::Path,
    plan: &crate::sync::full_sync::FullSyncPlan,
) -> crate::error::Result<PendingMutationMap> {
    // Issue #1065 复审 #7214314073 第 3 项：未确认事实的读取必须与写事务
    // 互斥；读失败也不能悄悄省略 target，否则本轮会把它当成“没有待上传”。
    let _workspace_lock = crate::storage::WorkspaceWriteLock::acquire(app_data_root)?;
    let mut pending = PendingMutationMap::new();
    for planned in &plan.targets {
        let Some(key) = crate::sync::full_sync::sync_target_checkpoint_key(planned) else {
            continue;
        };
        let scope = if planned.target.app_group.is_some() {
            crate::sync::types::SyncScope::App
        } else {
            crate::sync::types::SyncScope::Project
        };
        let records = crate::sync::local_mutations::pending_records(
            &planned.target_live_root,
            scope,
            planned.target.app_group.as_ref(),
        )?;
        pending.insert(key, records);
    }
    Ok(pending)
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
            // 短读锁只 clone 平台路径与 transport factory；provider 构造（含 Git 探测、
            // GitHub API 初始化）在锁外完成，不阻塞设置/正文读取。
            let (app_data_root, sync_transport) = {
                let core = self.core_read();
                (core.app_data_root.clone(), core.sync_transport.clone())
            };
            match crate::facade::WriterCore::create_sync_provider_unlocked(
                &app_data_root,
                sync_transport.as_ref(),
                &sync_config,
                &secrets,
            ) {
                Ok(p) => p,
                Err(e) => {
                    //   provider 创建失败 → 返回错误，
                    // 不降级为空 catalog（dry-run 是预览，但不能返回假的远端事实）。
                    log::warn!("[sync] dry-run: create_sync_provider_unlocked failed: {e}");
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
        // Issue #1008：进入同步前先补记上次进程留下的“正文已提交、证明未落盘”
        // 意图，保证本次 Prepare 使用的前置事实已包含原同步编辑。
        // 恢复会检查/修改 live 事实，必须与正文写事务共用 workspace 锁；
        // 只在恢复窗口持锁，不覆盖后续网络传输。
        let _recovery_workspace_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)
                .map_err(|error| crate::api::error::WriterError::from(error))?;
        let core = self.core_read();
        let recovery = core
            .sync_proof_intents_pending()
            .then(|| core.recover_pending_sync_proof_intents());
        if let Some(Err(error)) = recovery {
            log::warn!("recover pending sync proof intents before sync failed: {error}");
        }
        drop(core);
        drop(_recovery_workspace_lock);
        // 所有 durable 未投影 / 未结算事实必须在任何 fast path、target 过滤之前
        // 恢复：账本重建与补号、待确认 ack、章节 journal 的同步投影（复审 #4）。
        self.recover_sync_state_before_preflight();
        // 平台触发原因只决定"是否绕过周期间隔"（force_sync），以及是否走显式
        // 重建；它不再影响本地/远端版本比对，也不影响冲突裁决。
        let trigger_label = progress_sink
            .as_ref()
            .and_then(SyncProgressSink::trigger)
            .or(trigger_override.as_deref())
            .unwrap_or(if force_sync { "manual" } else { "auto" })
            .to_string();
        let run_mode = SyncRunMode::from_trigger(&trigger_label);
        record_sync_diagnostic("sync.run.start", "sync.run", {
            let mut fields = sync_diagnostic_fields(&operation_id, "prepare", None);
            fields.insert(
                "trigger".to_string(),
                serde_json::Value::String(trigger_label.clone()),
            );
            fields.insert(
                "run_mode".to_string(),
                serde_json::Value::String(
                    match run_mode {
                        SyncRunMode::Incremental => "incremental",
                        SyncRunMode::RebuildIndex => "rebuild",
                    }
                    .to_string(),
                ),
            );
            fields
        });
        let mut result = self.perform_full_sync_once(
            config.clone(),
            force_sync,
            run_mode,
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
                    run_mode,
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
                    run_mode,
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

    /// 持久化 checkpoint。
    ///
    /// - `succeeded == None`：这一轮的目标全部确认（无变化快速路径 / 无 target 需要处理）；
    /// - `succeeded == Some(set)`：只确认集合内的 target，其余 target 保留上一轮条目，
    ///   下一轮自然重试，不会因为一个 target 失败而让整份 checkpoint 失效。
    /// - `published_revision`：本轮实际 push 成功的提交。自己推送产生的 HEAD 就是
    ///   已被消费的远端状态，不再要求"前后 HEAD 相等"。
    #[allow(clippy::too_many_arguments)]
    fn persist_checkpoint(
        &self,
        app_data_root: &std::path::Path,
        repository_identity: Option<&str>,
        previous: Option<&crate::sync::full_sync_state::FullSyncCheckpoint>,
        snapshot: Option<&crate::sync::version_snapshot::LocalVersionSnapshot>,
        succeeded: Option<&std::collections::BTreeSet<String>>,
        consumed_remote_revision: Option<&str>,
        published_revision: Option<&str>,
        success: bool,
        operation_id: &str,
    ) {
        let Some(repository_identity) = repository_identity else {
            return;
        };
        let now = chrono::Utc::now().timestamp();
        let mut next =
            previous
                .cloned()
                .unwrap_or(crate::sync::full_sync_state::FullSyncCheckpoint {
                    repository_identity: repository_identity.to_string(),
                    remote_revision: String::new(),
                    local_object_fingerprint: String::new(),
                    target_fingerprints: std::collections::BTreeMap::new(),
                    target_versions: std::collections::BTreeMap::new(),
                    last_attempt_at: 0,
                    next_retry_at: 0,
                    consecutive_failures: 0,
                });
        next.repository_identity = repository_identity.to_string();
        next.record_attempt(now);
        if success {
            next.record_success(now);
            if let Some(snapshot) = snapshot {
                if let Some(succeeded) = succeeded {
                    let mut versions = std::collections::BTreeMap::new();
                    for (key, version) in &snapshot.targets {
                        // 还有未确认 mutation 的 target 不写入 checkpoint：
                        // 下一轮必须重新进入计划确认它们，不能靠"指纹相同"跳过
                        // （Issue #1065 复审 #3）。
                        if version.pending_objects > 0 {
                            continue;
                        }
                        if succeeded.contains(key) {
                            versions.insert(key.clone(), to_checkpoint_version(version));
                        } else if let Some(existing) = next.target_versions.get(key) {
                            versions.insert(key.clone(), existing.clone());
                        }
                    }
                    next.target_versions = versions;
                } else {
                    next.target_versions = snapshot
                        .targets
                        .iter()
                        .filter(|(_, version)| version.pending_objects == 0)
                        .map(|(key, version)| (key.clone(), to_checkpoint_version(version)))
                        .collect();
                }
            }
            // 远端已消费版本：优先本轮自己的推送，其次本轮固定快照的 HEAD。
            if let Some(revision) = published_revision {
                next.remote_revision = revision.to_string();
            } else if let Some(revision) = consumed_remote_revision {
                next.remote_revision = revision.to_string();
            }
        } else {
            next.record_failure(now);
            // 失败不推进远端消费游标：只有真正落盘的 target 才能宣称消费了
            // 那个 HEAD，否则下一轮会把没处理的远端变化当成已处理（复审 #4）。
            // 已确认的本地事实由账本保留，重试时重新计算差异即可。
            log::info!(
                "[sync] operation {} failed; next auto retry at {} (consecutive={})",
                operation_id,
                next.next_retry_at,
                next.consecutive_failures
            );
        }
        if let Err(error) = next.save(app_data_root) {
            log::warn!("[sync] failed to persist full-sync checkpoint: {error}");
        }
    }

    /// 是否存在尚未随远端提交确认的本地业务变更。
    ///
    /// 只读 mutation 账本（对象级 dirty index），不做网络探测、不扫描磁盘内容：
    /// 平台端用它决定"这一轮只需要轻量探测"还是"有东西要上传"。读不到时按
    /// 保守值 `true` 返回，避免把"不知道"当成"没有变更"。
    pub fn sync_has_pending_mutations(&self) -> bool {
        let (app_data_root, projects_root) = {
            let core = self.core_read();
            (core.app_data_root.clone(), core.projects_root.clone())
        };
        // 持久 dirty/recovery 索引干净且账本未被未登记追加时，直接回答“没有
        // 待上传”，不再枚举全部作品（Qt/鸿蒙都走这一个 Core 接口）。
        if let Some(checkpoint) =
            crate::sync::full_sync_state::FullSyncCheckpoint::load(&app_data_root)
                .ok()
                .flatten()
        {
            let expected = checkpoint_expected_roots(&app_data_root, &projects_root, &checkpoint);
            let index_clean = crate::sync::dirty_index::load(&app_data_root).is_some_and(|index| {
                crate::sync::dirty_index::is_clean_for_checkpoint(&index, &expected)
            });
            let lifecycle_clear =
                crate::sync::pending_deleted::load_pending_deleted_targets(&app_data_root)
                    .map(|targets| targets.is_empty())
                    .unwrap_or(false)
                    && crate::sync::pending_remote_cleanup::load_pending_remote_cleanups(
                        &app_data_root,
                    )
                    .map(|cleanups| cleanups.is_empty())
                    .unwrap_or(false);
            if index_clean && lifecycle_clear {
                return false;
            }
        }
        // App 组共享同一份账本：只读一次，再按 group 过滤。
        let ledger = match crate::sync::local_mutations::latest_ledger(&app_data_root) {
            Ok(ledger) => ledger,
            // 账本损坏/读失败：宁可让平台做一次完整探测，也不要误报"无待上传"。
            Err(_) => return true,
        };
        match crate::sync::app_data_group_catalog::discover_local_app_data_groups(&app_data_root) {
            Ok(groups) => {
                for group in groups {
                    let pending = crate::sync::local_mutations::pending_records_in(
                        &ledger,
                        &app_data_root,
                        crate::sync::types::SyncScope::App,
                        Some(&group),
                    );
                    match pending {
                        Ok(records) if records.is_empty() => {}
                        Ok(_) => return true,
                        Err(_) => return true,
                    }
                }
            }
            Err(_) => return true,
        }
        match crate::project::list_projects(&projects_root) {
            Ok(projects) => {
                for project in projects {
                    let project_root = projects_root.join(&project.id);
                    match crate::sync::local_mutations::pending_records(
                        &project_root,
                        crate::sync::types::SyncScope::Project,
                        None,
                    ) {
                        Ok(records) if records.is_empty() => {}
                        Ok(_) => return true,
                        Err(_) => return true,
                    }
                }
                false
            }
            Err(_) => true,
        }
    }

    /// 同步入口的持久恢复：在任何 fast path / target 过滤之前对所有 root 执行。
    ///
    /// 只做幂等重放：损坏账本归档重建并补号、待确认 ack 应用、章节 journal 的
    /// 同步投影重放。失败保留 durable 事实并在本轮继续，不会因"快照没变化"
    /// 而永远跳过（复审 #4）。
    fn recover_sync_state_before_preflight(&self) {
        let (app_data_root, projects_root) = {
            let core = self.core_read();
            (core.app_data_root.clone(), core.projects_root.clone())
        };
        // 校对待发布事务独立于正文与 scanner 代际：在任何 fast path 之前
        // 遍历实际待办目录重放，不能等用户重新打开失败章节。
        match crate::correction::retry_pending_publications(&app_data_root, &projects_root) {
            Ok(0) => {}
            Ok(published) => log::info!(
                "[correction] published {published} pending correction results before sync"
            ),
            Err(error) => {
                log::warn!("[correction] retry pending correction publications failed: {error}")
            }
        }
        let mut roots: Vec<std::path::PathBuf> = vec![app_data_root.clone()];
        match crate::project::list_projects(&projects_root) {
            Ok(projects) => {
                for project in projects {
                    roots.push(projects_root.join(project.id));
                }
            }
            Err(error) => {
                log::warn!("[sync] list projects for recovery failed: {error}");
            }
        }
        for root in &roots {
            let mut recovery_error: Option<String> = None;
            // 读标记放在账本恢复之后：恢复本身可能产生新的恢复责任，提前读会
            // 漏掉这一次（复审 #7213115893 第 4 项）。
            let start_generation = crate::sync::local_mutations::recovery_pending_generation(root);
            let had_marker = start_generation.is_some();
            // 正常空闲轮不读整份账本：只有存在未完成恢复标记时才做显式恢复。
            // 损坏账本由写入路径（append_batch）归档重建，读路径如实报错。
            let needs_ledger_recovery =
                had_marker || crate::sync::local_mutations::needs_ledger_recovery(root);
            if needs_ledger_recovery {
                match crate::sync::local_mutations::recover_ledger(root) {
                    Ok(true) => {
                        log::warn!("[sync] local mutation ledger rebuilt at {}", root.display())
                    }
                    Ok(false) => {}
                    Err(error) => {
                        log::warn!(
                            "[sync] local mutation ledger recovery failed at {}: {error}",
                            root.display()
                        );
                        recovery_error = Some(format!("ledger: {error}"));
                    }
                }
            }
            if recovery_error.is_none() {
                match crate::sync::local_mutations::replay_pending_acks(root) {
                    Ok(0) => {}
                    Ok(applied) => log::info!(
                        "[sync] replayed {applied} pending mutation acks at {}",
                        root.display()
                    ),
                    Err(error) => {
                        log::warn!(
                            "[sync] replay pending mutation acks failed at {}: {error}",
                            root.display()
                        );
                        recovery_error = Some(format!("acks: {error}"));
                    }
                }
            }
            if recovery_error.is_none() && *root != app_data_root {
                match crate::chapter::commit::replay_pending_sync_mutations(root) {
                    Ok(0) => {}
                    Ok(replayed) => log::info!(
                        "[sync] replayed {replayed} chapter mutation projection outboxes at {}",
                        root.display()
                    ),
                    Err(error) => {
                        log::warn!(
                            "[sync] replay chapter mutation projections failed at {}: {error}",
                            root.display()
                        );
                        recovery_error = Some(format!("journal: {error}"));
                    }
                }
            }
            // 有未完成责任时才做显式磁盘重建；正常空闲轮不扫描目录。账本恢复
            // 本身也可能产生新责任（归档重建），所以这里重新看一次标记。
            let needs_rescan = had_marker || crate::sync::local_mutations::recovery_pending(root);
            if needs_rescan && recovery_error.is_none() {
                let rescan: crate::Result<usize> = if *root == app_data_root {
                    crate::sync::app_data_group_catalog::discover_local_app_data_groups(root)
                        .and_then(|groups| {
                            let mut total = 0usize;
                            for group in groups {
                                total += crate::sync::local_mutations::rebuild_pending_from_disk(
                                    root,
                                    crate::sync::types::SyncScope::App,
                                    Some(&group),
                                )?;
                            }
                            Ok(total)
                        })
                } else {
                    crate::sync::local_mutations::rebuild_pending_from_disk(
                        root,
                        crate::sync::types::SyncScope::Project,
                        None,
                    )
                };
                match rescan {
                    Ok(count) => log::info!(
                        "[sync] rebuilt {count} pending mutations from disk at {}",
                        root.display()
                    ),
                    Err(error) => {
                        log::warn!(
                            "[sync] rebuild pending mutations failed at {}: {error}",
                            root.display()
                        );
                        recovery_error = Some(format!("rescan: {error}"));
                    }
                }
            }
            match recovery_error {
                // 恢复没做完：整个根在下一轮之前都不可走无变化快速路径。
                Some(reason) => crate::sync::local_mutations::mark_recovery_pending(root, &reason),
                // 只清自己开始时那一代责任：恢复过程中又产生的新责任保留。
                None => crate::sync::local_mutations::clear_recovery_pending_if_generation(
                    root,
                    start_generation,
                ),
            }
        }
    }

    /// 按推送回执确认本轮实际发布的对象事实（Issue #1065 复审 #3）。
    ///
    /// 只确认 preflight 快照里存在、且本轮确实随提交发布的 mutation：推进
    /// known_files、记录确认序号、按序号压缩账本历史前缀。同步期间新写入的
    /// 更新事实序号更大，保留到下一轮；失败/跳过的发布不确认任何事实。
    /// 返回未能结算的 target 数：>0 表示已推送成功但本地确认没有落盘，
    /// 调用方必须把本轮标成明确的未结算状态（不能只 warn 后报成功）。
    fn confirm_published_targets(
        &self,
        preflight_pending: &PendingMutationMap,
        targets: &[crate::sync::types::TargetSyncResult],
        receipts: &[crate::sync::provider::round::PublishReceipt],
        consumed: &std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
        round_id: &str,
        published_revision: Option<&str>,
    ) -> usize {
        let (app_data_root, projects_root) = {
            let core = self.core_read();
            (core.app_data_root.clone(), core.projects_root.clone())
        };
        let mut unsettled = 0usize;
        for target in targets {
            let key = match (&target.group_id, &target.project_id) {
                (Some(group_id), _) => format!("app:{group_id}"),
                (None, Some(project_id)) => format!("project:{project_id}"),
                _ => continue,
            };
            let Some(pending) = preflight_pending.get(&key) else {
                continue;
            };
            let (sync_root, sync_target) = match (&target.group_id, &target.project_id) {
                (Some(group_id), _) => match crate::sync::types::AppDataGroup::from_id(group_id) {
                    Ok(group) => (
                        app_data_root.clone(),
                        crate::sync::types::SyncTarget::app_data_group(group),
                    ),
                    Err(_) => continue,
                },
                (None, Some(project_id)) => (
                    projects_root.join(project_id),
                    crate::sync::types::SyncTarget::project(project_id),
                ),
                _ => continue,
            };
            let mut published = Vec::new();
            // 只认 flush 真实写出去的路径 + 内容摘要：计划里的 uploaded_files
            // 可能与实际发布的对象不一致（回执口径，复审 #3）。
            let prefix = format!("{}/", target.remote_prefix.trim_end_matches('/'));
            for receipt in receipts {
                let Some(rel) = receipt.path.strip_prefix(prefix.as_str()) else {
                    continue;
                };
                let Some(record) = pending.get(rel) else {
                    continue;
                };
                match receipt.op.as_str() {
                    "upsert" if record.is_upsert() => {
                        if receipt.content_digest.as_deref() == Some(record.content_hash.as_str()) {
                            published.push(crate::sync::local_mutations::PublishedMutation {
                                path: rel.to_string(),
                                op: "upsert".to_string(),
                                content_hash: record.content_hash.clone(),
                                seq: record.seq,
                                updated_at_ms: record.updated_at_ms,
                                deleted_at_ms: record.deleted_at_ms,
                            });
                        }
                    }
                    "delete" if record.is_delete() => {
                        published.push(crate::sync::local_mutations::PublishedMutation {
                            path: rel.to_string(),
                            op: "delete".to_string(),
                            content_hash: record.content_hash.clone(),
                            seq: record.seq,
                            updated_at_ms: record.updated_at_ms,
                            deleted_at_ms: record.deleted_at_ms,
                        });
                    }
                    _ => {}
                }
            }
            // 已经与远端一致的 pending 事实（编辑后又还原、旧账本遗留）不需要
            // 发布就能确认：用提交后的 known_files 证明远端基线已是同一内容。
            // 删除仍然只认推送回执，不能用"known_files 里没有"推断（复审 #3）。
            if let Ok(state) =
                crate::sync::SyncService::load_sync_state_for_target(&sync_root, &sync_target)
            {
                for (path, record) in pending {
                    if !record.is_upsert() {
                        continue;
                    }
                    if published.iter().any(|entry| entry.path == *path) {
                        continue;
                    }
                    if state.known_files.get(path) == Some(&record.content_hash) {
                        published.push(crate::sync::local_mutations::PublishedMutation {
                            path: path.clone(),
                            op: "upsert".to_string(),
                            content_hash: record.content_hash.clone(),
                            seq: record.seq,
                            updated_at_ms: record.updated_at_ms,
                            deleted_at_ms: record.deleted_at_ms,
                        });
                    }
                }
            }
            // 远端胜出的事实不再作为本地编辑回推：overwritten/远端删除说明
            // 本地这次修改已被合并决议放弃，直接标记 superseded（复审 #3）。
            // 被远端消费/让位的路径来自提交阶段的对象决议，不再拿 transfer
            // 的 overwritten_files/remote_deletes 反推（复审 #7213234617 第 1 项）。
            for path in consumed.get(&key).into_iter().flatten() {
                let Some(record) = pending.get(path) else {
                    continue;
                };
                if published.iter().any(|entry| entry.path == *path) {
                    continue;
                }
                published.push(crate::sync::local_mutations::PublishedMutation {
                    path: path.clone(),
                    op: "superseded".to_string(),
                    content_hash: record.content_hash.clone(),
                    seq: record.seq,
                    updated_at_ms: record.updated_at_ms,
                    deleted_at_ms: record.deleted_at_ms,
                });
            }
            if published.is_empty() {
                continue;
            }
            match crate::sync::local_mutations::mark_confirmed(
                &sync_root,
                &sync_target,
                &published,
                round_id,
                published_revision,
            ) {
                Ok(_) => {}
                Err(error) => {
                    log::warn!("[sync] confirm published mutations failed for {key}: {error}");
                    unsettled += 1;
                }
            }
        }
        unsettled
    }

    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn perform_full_sync_once(
        &self,
        config: SyncConfigDto,
        force_sync: bool,
        run_mode: SyncRunMode,
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
        // 1a. 短读锁：只 snapshot 平台路径与 transport factory。
        // provider 构造（transport 初始化、Git 探测、GitHub API 初始化）在锁外执行，
        // 避免手动同步期间长时间占用 Core 锁阻塞设置/正文读取。
        let provider_phase =
            SyncDiagnosticPhase::start(diagnostic_operation_id, "provider_init", None);
        let (app_data_root, sync_transport) = {
            let core = self.core_read();
            (core.app_data_root.clone(), core.sync_transport.clone())
        };
        let provider_result = crate::facade::WriterCore::create_sync_provider_unlocked(
            &app_data_root,
            sync_transport.as_ref(),
            &sync_config,
            &secrets,
        );
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
        let checkpoint = crate::sync::full_sync_state::FullSyncCheckpoint::load(&app_data_root)
            .ok()
            .flatten();
        let previous_was_converged = previous_full_sync_state.as_ref().is_some_and(|state| {
            crate::sync::full_sync_state::FullSyncState::is_overall_success(&state.overall_status)
                && state.failed_targets.is_empty()
        });
        let checkpoint_applicable = checkpoint.as_ref().is_some_and(|checkpoint| {
            previous_was_converged
                && repo_identity
                    .as_ref()
                    .is_some_and(|identity| checkpoint.repository_identity == *identity)
        });

        // 预检/传输阶段的失败也要写退避：否则自动调度会在每次前台唤醒里
        // 重试同一个坏状态，而 checkpoint 停留在旧版本（复审 #3）。
        let persist_failure_checkpoint = |consumed: Option<&str>| {
            self.persist_checkpoint(
                &app_data_root,
                repo_identity.as_deref(),
                checkpoint.as_ref(),
                None,
                None,
                consumed,
                None,
                false,
                diagnostic_operation_id,
            );
        };

        // 失败退避只约束自动触发。手动/启动/交接/前台/网络恢复（force_sync）
        // 不受退避窗口限制，否则用户手动重试会被上一次失败挡住。
        if !force_sync {
            if let Some(checkpoint) = checkpoint.as_ref() {
                let now = chrono::Utc::now().timestamp();
                if !checkpoint.auto_retry_allowed(now) {
                    log::info!(
                        "[sync] auto sync skipped: retry backoff active until {} (failures={})",
                        checkpoint.next_retry_at,
                        checkpoint.consecutive_failures
                    );
                    return make_noop_result();
                }
            }
        }

        // A. 轻量远端探测：只读远端 refs。HEAD 与上次消费的提交一致且本地对象版本
        // 也一致时，本轮不 fetch、不建 plan、不 staging、不发布，直接结束。
        let revision_phase = SyncDiagnosticPhase::start_with_provider(
            diagnostic_operation_id,
            "remote_probe",
            None,
            provider.as_ref(),
        );
        let probed_revision = match provider.probe_remote_revision() {
            Ok(revision) => revision,
            Err(error) => {
                revision_phase.finish_with_provider("error", provider.as_ref());
                let core_error = crate::error::Error::from(error);
                let status = crate::sync::full_sync::error_to_persist_status(&core_error);
                let error_category = core_sync_category(&core_error);
                let writer_error = crate::api::error::WriterError::from(core_error);
                record_sync_phase_failure(diagnostic_operation_id, "remote_probe", &writer_error);
                persist_failure_checkpoint(None);
                log::warn!(
                    "[sync] remote_probe failed: error_code={}",
                    writer_error.code()
                );
                let _ = self.record_full_sync_preflight_failure_typed(
                    sync_status_to_wire(&status),
                    "remote_probe".to_string(),
                    error_category,
                    "remote_probe".to_string(),
                    writer_error.to_string(),
                );
                return Err(writer_error);
            }
        };
        revision_phase.finish_with_provider("success", provider.as_ref());

        // 空闲快路径只查持久 dirty/recovery 索引 + 固定远端 HEAD（评审
        // #7214314073 第 3/4 项）：索引干净且账本字节数与登记一致时，不再
        // 枚举作品、解析 manifest 或扫描恢复目录。索引缺失/不一致/有恢复
        // 责任一律回落到下面的完整本地快照，绝不把“不知道”当“没有变化”。
        if run_mode == SyncRunMode::Incremental && checkpoint_applicable {
            if let Some(checkpoint) = checkpoint.as_ref() {
                if probed_revision.as_deref() == Some(checkpoint.remote_revision.as_str()) {
                    let expected =
                        checkpoint_expected_roots(&app_data_root, &projects_root, checkpoint);
                    let lifecycle_pending_cheap = match (
                        crate::sync::pending_deleted::load_pending_deleted_targets(&app_data_root),
                        crate::sync::pending_remote_cleanup::load_pending_remote_cleanups(
                            &app_data_root,
                        ),
                    ) {
                        (Ok(deleted), Ok(cleanups)) => !deleted.is_empty() || !cleanups.is_empty(),
                        // 读不到收口清单：必须完整核对，不能跳过远端删除。
                        _ => true,
                    };
                    if !lifecycle_pending_cheap {
                        let index_clean = crate::sync::dirty_index::load(&app_data_root)
                            .is_some_and(|index| {
                                crate::sync::dirty_index::is_clean_for_checkpoint(&index, &expected)
                            });
                        if index_clean {
                            log::info!(
                                "[sync] workspace dirty index clean; returning no_changes without local snapshot"
                            );
                            let mut fields = sync_diagnostic_fields(
                                diagnostic_operation_id,
                                "dirty_index_match",
                                None,
                            );
                            fields.insert(
                                "skipped_targets".to_string(),
                                serde_json::Value::Number(serde_json::Number::from(
                                    u64::try_from(expected.len()).unwrap_or(u64::MAX),
                                )),
                            );
                            record_sync_diagnostic("sync.checkpoint.skip", "sync.run", fields);
                            // 只带了 index/HEAD 证据，没有新的对象快照：保留上一轮
                            // checkpoint 的 target_versions，不伪造版本推进。
                            self.persist_checkpoint(
                                &app_data_root,
                                repo_identity.as_deref(),
                                Some(checkpoint),
                                None,
                                None,
                                probed_revision.as_deref(),
                                None,
                                true,
                                diagnostic_operation_id,
                            );
                            let (mut result, _, _) =
                                self.core_write().finalize_full_sync(Vec::new(), None, None);
                            result.targets.clear();
                            return Ok(result.into());
                        }
                    }
                }
            }
        }

        let local_snapshot_phase = SyncDiagnosticPhase::start_with_provider(
            diagnostic_operation_id,
            "local_snapshot",
            None,
            provider.as_ref(),
        );
        let preflight_snapshot = match local_sync_version_snapshot(&app_data_root, &projects_root) {
            Ok(snapshot) => {
                local_snapshot_phase.finish_with_provider("success", provider.as_ref());
                Some(snapshot)
            }
            Err(error) => {
                local_snapshot_phase.finish_with_provider("error", provider.as_ref());
                log::warn!(
                    "[sync] local object version snapshot failed; continuing with LWW: {error}"
                );
                None
            }
        };

        // 待完成的删除/远端清理事务不在任何 target 的对象版本里，必须让它们
        // 进入完整计划，不能被"无变化"快速路径或 target 过滤挡掉。
        let lifecycle_pending = preflight_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.lifecycle_pending);
        let remote_probed_unchanged = checkpoint_applicable
            && !lifecycle_pending
            && checkpoint.as_ref().is_some_and(|checkpoint| {
                probed_revision.as_deref() == Some(checkpoint.remote_revision.as_str())
            });

        // 无变化快速路径：远端 HEAD 未变 + 每个 target 的业务对象版本与
        // mutation 代次都等于 checkpoint 记录的那一轮。任何 target 不 eligible
        // （冲突/错误/缺基线）都回落到完整 LWW，但只影响该 target 的判定。
        if run_mode == SyncRunMode::Incremental && remote_probed_unchanged {
            if let (Some(checkpoint), Some(snapshot)) =
                (checkpoint.as_ref(), preflight_snapshot.as_ref())
            {
                // 还有未确认 mutation 的 target 必须再走一轮合并来确认
                // （发布成功只代表远端已有内容，本地"已确认基线"要由下一次
                // 合并把 known_files 推进到远端实际内容后才能清账）。
                let all_match = checkpoint.target_versions.len() == snapshot.targets.len()
                    && snapshot.targets.iter().all(|(key, version)| {
                        version.pending_objects == 0
                            && checkpoint.target_versions.get(key).is_some_and(|stored| {
                                stored.matches(&to_checkpoint_version(version))
                            })
                    });
                if all_match {
                    log::info!(
                        "[sync] full-sync checkpoint matched; returning no_changes without fetch or transfers"
                    );
                    // 完整快照已证明所有 target 无未确认事实：校准持久 dirty
                    // 索引，使后续空闲轮不必再枚举作品/解析 manifest。
                    let expected =
                        checkpoint_expected_roots(&app_data_root, &projects_root, checkpoint);
                    if let Err(error) =
                        crate::sync::dirty_index::seed_clean(&app_data_root, &expected)
                    {
                        log::warn!("[sync] cannot seed workspace dirty index: {error}");
                    }
                    let mut fields =
                        sync_diagnostic_fields(diagnostic_operation_id, "checkpoint_match", None);
                    fields.insert(
                        "skipped_targets".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(
                            u64::try_from(snapshot.targets.len()).unwrap_or(u64::MAX),
                        )),
                    );
                    record_sync_diagnostic("sync.checkpoint.skip", "sync.run", fields);
                    self.persist_checkpoint(
                        &app_data_root,
                        repo_identity.as_deref(),
                        Some(checkpoint),
                        Some(snapshot),
                        None,
                        probed_revision.as_deref(),
                        None,
                        true,
                        diagnostic_operation_id,
                    );
                    let (mut result, _, _) = self.core_write().finalize_full_sync(
                        snapshot.no_change_targets.clone(),
                        None,
                        None,
                    );
                    // The target results were persisted by finalize_full_sync. Returning
                    // an empty list lets the platform distinguish this checkpoint no-op
                    // and avoid launching generation GC on every polling interval.
                    result.targets.clear();
                    return Ok(result.into());
                }
            }
        }

        // 远端 HEAD 未变（只是本地有修改）时，用本地缓存的 commit 直接构造固定
        // 快照，不再 fetch；ls-remote 已确认远端仍是那个提交。
        if remote_probed_unchanged {
            if let Some(checkpoint) = checkpoint.as_ref() {
                provider.warm_snapshot_from_cache(&checkpoint.remote_revision);
            }
        }
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
                persist_failure_checkpoint(None);
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

        // 每个受影响 target 的远端变化相对路径（strip 掉 target 前缀），用于
        // 构造对象级增量计划（复审 #7213234617 第 2 项）。
        let mut remote_changed_paths: std::collections::BTreeMap<
            String,
            std::collections::BTreeSet<String>,
        > = std::collections::BTreeMap::new();
        let mut remote_unmapped_keys: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        // 远端差异 → 受影响 target。只有 checkpoint 可比较且 Provider 能给出
        // tree diff 时才做增量选择；基线缺失/迁移/无法给出差异一律完整核对。
        let affected_targets: Option<std::collections::BTreeSet<String>> = match (
            run_mode,
            checkpoint_applicable,
            preflight_snapshot.as_ref(),
            checkpoint.as_ref(),
        ) {
            (SyncRunMode::Incremental, true, Some(snapshot), Some(checkpoint))
                if !snapshot.lifecycle_pending =>
            {
                let app_groups =
                    crate::sync::app_data_group_catalog::discover_local_app_data_groups(
                        &app_data_root,
                    )
                    .unwrap_or_default();
                let project_ids: Vec<String> = match crate::project::list_projects(&projects_root) {
                    Ok(projects) => projects.into_iter().map(|project| project.id).collect(),
                    Err(_) => Vec::new(),
                };
                let mut remote_keys = std::collections::BTreeSet::new();
                let mut remote_paths_known = remote_probed_unchanged;
                if !remote_probed_unchanged {
                    match provider.changed_paths_since(&checkpoint.remote_revision) {
                        Ok(Some(paths)) => {
                            remote_paths_known = true;
                            for path in paths {
                                match crate::sync::version_snapshot::target_key_for_remote_path(
                                    &path,
                                    &app_groups,
                                    &project_ids,
                                ) {
                                    Some(key) => {
                                        if let Some(relative) =
                                            target_relative_remote_path(&path, &key, &app_groups)
                                        {
                                            remote_changed_paths
                                                .entry(key.clone())
                                                .or_default()
                                                .insert(relative);
                                        } else {
                                            remote_unmapped_keys.insert(key.clone());
                                        }
                                        remote_keys.insert(key);
                                    }
                                    None => {
                                        // 无法归因的远端路径（catalog 顶层、未知目录）
                                        // 必须影响全部 target，不能静默漏掉。
                                        remote_paths_known = false;
                                        break;
                                    }
                                }
                            }
                        }
                        Ok(None) | Err(_) => {
                            remote_paths_known = false;
                        }
                    }
                }
                if remote_paths_known {
                    for (key, version) in &snapshot.targets {
                        let locally_changed = !checkpoint
                            .target_versions
                            .get(key)
                            .is_some_and(|stored| stored.matches(&to_checkpoint_version(version)));
                        // 未确认 mutation 必须进入本轮计划：即使指纹与 checkpoint
                        // 记录相同（例如上一轮推送成功但尚未确认），也不能被过滤掉，
                        // 否则这些事实永远等不到下一轮（Issue #1065 复审 #3）。
                        if locally_changed || version.pending_objects > 0 {
                            remote_keys.insert(key.clone());
                        }
                    }
                    // 已消失的 target（例如本地删除了作品）仍需交给正常流程收口。
                    for key in checkpoint.target_versions.keys() {
                        if !snapshot.targets.contains_key(key) {
                            remote_keys.insert(key.clone());
                        }
                    }
                    Some(remote_keys)
                } else {
                    log::info!(
                        "[sync] remote tree diff unavailable; keeping every target in the LWW plan"
                    );
                    None
                }
            }
            _ => None,
        };

        // Issue #1012: create the one-round buffer before bootstrap so the
        // first catalog write is part of the same final commit.
        let round = crate::sync::provider::round::RoundProvider::new(
            provider.as_ref(),
            remote_revision_before
                .clone()
                .map(crate::sync::provider::model::RemoteVersion::new),
        );

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
                    persist_failure_checkpoint(None);
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
                    &round,
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
                        persist_failure_checkpoint(None);
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

        // 只对受影响的 target 建 staging、做 LWW 和发布：远端 tree diff 映射出的
        // target ∪ 本地对象版本变化的 target。远端与本地都没变的 target 不进入
        // 计划，也不产生任何远端写。
        if let Some(affected) = affected_targets.as_ref() {
            let previous_target_count = plan.targets.len();
            plan.targets.retain(
                |planned| match crate::sync::full_sync::sync_target_checkpoint_key(planned) {
                    Some(key) => affected.contains(&key),
                    // 没有稳定 key 的 target（生命周期/删除收口）总是保留。
                    None => true,
                },
            );
            let skipped_target_count = previous_target_count.saturating_sub(plan.targets.len());
            if skipped_target_count > 0 {
                log::info!(
                    "[sync] unchanged targets skipped from transfer plan: count={skipped_target_count}"
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
                if let (Some(checkpoint), Some(snapshot)) =
                    (checkpoint.as_ref(), preflight_snapshot.as_ref())
                {
                    self.persist_checkpoint(
                        &app_data_root,
                        repo_identity.as_deref(),
                        Some(checkpoint),
                        Some(snapshot),
                        None,
                        remote_revision_before.as_deref(),
                        None,
                        true,
                        diagnostic_operation_id,
                    );
                    let (mut result, _, _) = self.core_write().finalize_full_sync(
                        snapshot.no_change_targets.clone(),
                        None,
                        None,
                    );
                    result.targets.clear();
                    return Ok(result.into());
                }
                let (mut result, _, _) =
                    self.core_write().finalize_full_sync(Vec::new(), None, None);
                result.targets.clear();
                return Ok(result.into());
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
        // 在 seed 之前固定各 target 的未确认事实：seed 复制的是这一刻的 live
        // 内容，推送成功后只按这份快照的序号确认；seed 之后写入的更新事实
        // 序号更大，保留为下一轮 outbox（Issue #1065 复审 #3）。
        let preflight_pending = match collect_preflight_pending(&self.app_data_root, &plan) {
            Ok(pending) => pending,
            Err(error) => {
                let msg = format!("collect preflight pending mutations failed: {error}");
                log::warn!("[sync] prepare: {msg}");
                let status = crate::sync::full_sync::error_to_persist_status(&error);
                let error_category = core_sync_category(&error);
                let _ = self.record_full_sync_preflight_failure_typed(
                    sync_status_to_wire(&status),
                    "preflight_pending".to_string(),
                    error_category,
                    "preflight_pending".to_string(),
                    msg,
                );
                return Err(crate::api::error::WriterError::from(error));
            }
        };
        let staging_phase = SyncDiagnosticPhase::start_with_provider(
            diagnostic_operation_id,
            "staging",
            None,
            provider.as_ref(),
        );
        // 对象级增量计划：只为"远端确实变了 + 本地确有未确认事实"的 target
        // 裁剪 staging。条件全部满足才裁剪，否则整树 seed（完整核对）。
        let sparse_plans: std::collections::HashMap<
            String,
            crate::sync::sparse_plan::SparseObjectPlan,
        > = {
            let mut plans = std::collections::HashMap::new();
            // 只有在"真增量"模式下、且远端 diff 完整可归因时才允许裁剪；
            // 显式重建/迁移与 diff 未知一律完整核对。
            let incremental_inputs_ready = run_mode == SyncRunMode::Incremental
                && checkpoint_applicable
                && affected_targets.is_some();
            if incremental_inputs_ready {
                if let Some(snapshot) = preflight_snapshot.as_ref() {
                    for planned in &plan.targets {
                        let Some(key) = crate::sync::full_sync::sync_target_checkpoint_key(planned)
                        else {
                            continue;
                        };
                        if remote_unmapped_keys.contains(&key) {
                            continue;
                        }
                        let Some(version) = snapshot.targets.get(&key) else {
                            continue;
                        };
                        if !version.eligible {
                            continue;
                        }
                        // 星图组的 graph↔对象依赖闭包必须先显式建模，暂不裁剪。
                        if matches!(
                            planned.target.app_group.as_ref(),
                            Some(crate::sync::types::AppDataGroup::Starmap(_))
                                | Some(crate::sync::types::AppDataGroup::StarmapIndex)
                        ) {
                            continue;
                        }
                        let pending = preflight_pending.get(&key);
                        let pending_count = pending.map_or(0, std::collections::BTreeMap::len);
                        // 本地脏事实必须全部来自未确认 mutation，否则整树核对。
                        if pending_count != version.pending_objects {
                            continue;
                        }
                        let mut remote_paths: Vec<String> = remote_changed_paths
                            .get(&key)
                            .map(|paths| paths.iter().cloned().collect())
                            .unwrap_or_default();
                        let mut pending_paths: Vec<String> = pending
                            .map(|records| records.keys().cloned().collect())
                            .unwrap_or_default();
                        if remote_paths.is_empty() && pending_paths.is_empty() {
                            continue;
                        }
                        remote_paths.sort();
                        remote_paths.dedup();
                        pending_paths.sort();
                        pending_paths.dedup();
                        // 业务对象依赖：章节正文与 meta 属于同一对象，必须同时加载，
                        // 否则三方比较会把同一个章节的另一半当成未加载而沿用旧事实。
                        let mut expanded: Vec<String> = remote_paths
                            .iter()
                            .chain(pending_paths.iter())
                            .cloned()
                            .collect();
                        for path in &expanded {
                            if let Some(sibling) = chapter_sibling_path(path) {
                                pending_paths.push(sibling);
                            }
                        }
                        expanded.clear();
                        pending_paths.sort();
                        pending_paths.dedup();
                        let plan = crate::sync::sparse_plan::SparseObjectPlan::new(
                            checkpoint
                                .as_ref()
                                .map(|stored| stored.remote_revision.clone()),
                            pending_paths,
                            remote_paths,
                        )
                        // 短事务锁内已经取到的不可变事实：seq/op/base hash/content
                        // hash/业务时间/删除时间；本轮后续编辑不改这份输入。
                        .with_mutations(pending.cloned().unwrap_or_default());
                        plans.insert(key, plan);
                    }
                }
            }
            plans
        };
        if !sparse_plans.is_empty() {
            log::info!(
                "[sync] object-level incremental plan: {} target(s)",
                sparse_plans.len()
            );
        }
        let staging_runs =
            match crate::sync::staging::prepare_staging_runs(&mut plan, &sparse_plans) {
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
        let RoundOutcome {
            result,
            published_revision,
            publish_succeeded,
        } = match self.perform_full_sync_with_provider_for_run(
            provider.as_ref(),
            &round,
            &plan,
            staging_runs,
            cancellation_token,
            progress_sink,
            target_progress,
            diagnostic_operation_id,
            remote_revision_before.as_deref(),
            &preflight_pending,
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                // 失败也要落退避与已消费的远端版本：否则自动调度会在每个前台
                // 唤醒里重试同一个坏状态，而 checkpoint 却停留在旧版本
                // （Issue #1065 复审 #3）。已落盘的本地结果不回滚。
                self.persist_checkpoint(
                    &app_data_root,
                    repo_identity.as_deref(),
                    checkpoint.as_ref(),
                    None,
                    None,
                    remote_revision_before.as_deref(),
                    None,
                    false,
                    diagnostic_operation_id,
                );
                return Err(error);
            }
        };

        let fully_converged = matches!(
            result.overall_status.as_str(),
            "success" | "no_changes" | "latest_wins_applied"
        ) && result.targets.iter().all(|target| {
            matches!(
                target.result.status.as_str(),
                "success" | "no_changes" | "latest_wins_applied"
            )
        });
        // checkpoint 只确认"这一轮 preflight 快照对应的版本"：本轮期间新到的
        // mutation 不在快照里，下一轮 pending 代次不同，自然重新处理。
        // 失败 target 不推进（保留旧条目 → 下一轮重试该 target）；成功 target
        // 的远端版本用本轮实际发布的提交，而不是"前后 HEAD 相等"。
        let app_groups =
            crate::sync::app_data_group_catalog::discover_local_app_data_groups(&app_data_root)
                .unwrap_or_default();
        if let Some(snapshot) = local_sync_version_snapshot(&app_data_root, &projects_root).ok() {
            let succeeded: std::collections::BTreeSet<String> = result
                .targets
                .iter()
                .filter(|target| {
                    matches!(
                        target.result.status.as_str(),
                        "success" | "no_changes" | "latest_wins_applied"
                    )
                })
                .filter_map(|target| dto_target_checkpoint_key(target, &app_groups))
                .collect();
            self.persist_checkpoint(
                &app_data_root,
                repo_identity.as_deref(),
                checkpoint.as_ref(),
                Some(&snapshot),
                if fully_converged && publish_succeeded {
                    Some(&succeeded)
                } else {
                    None
                },
                remote_revision_before.as_deref(),
                published_revision.as_deref(),
                fully_converged && publish_succeeded,
                diagnostic_operation_id,
            );
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
        let base_revision = provider
            .remote_revision()
            .ok()
            .flatten()
            .map(crate::sync::provider::model::RemoteVersion::new);
        let round = crate::sync::provider::round::RoundProvider::new(provider, base_revision);
        self.perform_full_sync_with_provider_for_run(
            provider,
            &round,
            plan,
            staging_runs,
            cancellation_token,
            progress_sink,
            target_progress,
            &operation_id,
            None,
            &PendingMutationMap::new(),
        )
        .map(|outcome| outcome.result)
    }

    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn perform_full_sync_with_provider_for_run(
        &self,
        provider: &dyn crate::sync::provider::SyncProvider,
        round: &crate::sync::provider::round::RoundProvider<'_>,
        plan: &crate::sync::full_sync::FullSyncPlan,
        staging_runs: Vec<crate::sync::staging::StagingRun>,
        cancellation_token: Option<SyncCancellationToken>,
        progress_sink: Option<SyncProgressSink>,
        target_progress: Option<&crate::sync::full_sync::SyncProgressCallback>,
        diagnostic_operation_id: &str,
        remote_revision: Option<&str>,
        preflight_pending: &PendingMutationMap,
    ) -> ApiResult<RoundOutcome> {
        struct PendingTargetCommit {
            target_index: usize,
            prepared_failure: bool,
            result: crate::sync::types::TargetSyncResult,
            target_digest: String,
            target_kind: String,
            target_id_digest: Option<String>,
            active_generation: Option<String>,
            started_at: Instant,
        }

        // 本轮实际发布的远端提交（仅当 flush 真正创建了 commit）。checkpoint 用它
        // 记录"自己成功推送后的 HEAD"，不再要求前后 HEAD 相等。
        let mut published_revision: Option<String> = None;
        let mut all_targets: Vec<crate::sync::types::TargetSyncResult> = Vec::new();
        let mut all_committed_paths: Vec<std::path::PathBuf> = Vec::new();
        let mut chapter_changes: Vec<crate::chapter::ChapterChangeReceipt> = Vec::new();
        let mut pending_commits: Vec<PendingTargetCommit> = Vec::new();
        let mut completed_transfer_results: Vec<crate::sync::types::TargetSyncResult> = Vec::new();
        let mut catalog_snapshot = plan.remote_catalog_snapshot.clone();

        let total_targets = u32::try_from(plan.targets.len()).unwrap_or(u32::MAX);

        for target_index in 0..plan.targets.len() {
            // Issue #729：每个 target 前检查取消令牌。
            if Self::sync_cancelled(cancellation_token.as_ref()) {
                log::info!(
                    "[sync] perform_full_sync: cancellation requested — breaking after {} targets",
                    pending_commits.len()
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
                    round,
                    plan,
                    target_index,
                    &mut catalog_snapshot,
                    cancellation_token.as_ref(),
                    &completed_transfer_results,
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

            // Local live/state commit is delayed until the single remote
            // publication succeeds.  This keeps known_files from advancing for
            // content that was never pushed.
            completed_transfer_results.push(target_sync_result.clone());
            pending_commits.push(PendingTargetCommit {
                target_index,
                prepared_failure: prepared_failure.is_some(),
                result: target_sync_result,
                target_digest,
                target_kind,
                target_id_digest,
                active_generation,
                started_at: target_started_at,
            });
        }

        // Issue #1012: cancellation discards this round's remote writes.  A
        // target error that makes the merged tree incomplete also blocks the
        // final publish instead of pushing a partial round.
        if Self::sync_cancelled(cancellation_token.as_ref()) {
            round.discard();
            for run in &staging_runs {
                run.cleanup();
            }
            log::info!(
                "[sync] perform_full_sync: cancellation requested after per-target loop — discarding buffered remote writes"
            );
            self.core_write().persist_full_sync_cancelled();
            return Ok(RoundOutcome {
                result: Self::full_sync_noop_result(),
                published_revision: None,
                publish_succeeded: false,
            });
        }
        // 有 target 失败时仍然先提交已下载/已合并的内容到 live：单侧下载和
        // 合并结果不要求远端写成功才能落盘。只是不发布不完整的 tree。
        let mut publish_blocked = pending_commits
            .iter()
            .any(|pending| target_blocks_round_publish(&pending.result));
        if publish_blocked {
            round.discard();
            log::warn!(
                "[sync] full-sync remote publish discarded because a target failed before publish"
            );
        }

        // C 阶段：把下载、合并与冲突记录持久提交到 live。发布失败不回滚这些
        // 已落盘结果；本地未发布对象版本通过 mutation 账本保持未确认，
        // 下一轮继续推进（见 commit_helpers 的 deferred upload 处理）。
        // Issue #1021 复核：恢复只在一轮同步内做一次，所有 target 复用同一
        // 已落盘事件集合/未完成问题，避免每组 target 重解压全部历史事件。
        // 证明恢复会读 live 并写统计/意图，必须持有 workspace 锁，
        // 且锁序为 workspace → Core。
        let _proof_recovery_workspace_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root).map_err(|error| {
                crate::api::error::WriterError::from(crate::error::Error::from(error))
            })?;
        let mut proof_recovery_state = match self.core_write().recover_pending_sync_proof_state() {
            Ok(state) => state,
            Err(error) => crate::sync::proof_intent::ProofRecoveryState::failed(format!(
                "recover pending sync proof intents before commit failed: {error}"
            )),
        };
        drop(_proof_recovery_workspace_lock);
        // 本轮已经 durable 落进 live 的对象事实：发布前用它把待发布内容
        // 冻结成刚提交的字节，杜绝推送旧 staging（复审 #2）。
        let mut publish_decisions: Vec<(String, crate::sync::decision::DurableObjectDecision)> =
            Vec::new();
        // 被远端消费的本地事实（target key → target 相对路径），用于确认。
        let mut consumed_by_target: std::collections::BTreeMap<
            String,
            std::collections::BTreeSet<String>,
        > = std::collections::BTreeMap::new();
        for pending in pending_commits {
            let PendingTargetCommit {
                target_index,
                prepared_failure,
                result: target_sync_result,
                target_digest,
                target_kind,
                target_id_digest,
                active_generation,
                started_at,
            } = pending;
            let planned = &plan.targets[target_index];

            if let Some(ref sink) = progress_sink {
                sink.set_target_phase(
                    &planned.target.remote_prefix,
                    planned.project_id.as_deref(),
                    "commit",
                );
            }

            let commit_phase = SyncDiagnosticPhase::start_with_provider(
                diagnostic_operation_id,
                "commit",
                Some(target_digest.clone()),
                provider,
            );
            // 决议键必须在提交前确定：facts 是 preflight 在短事务里取得的
            // durable 事实，提交后不再重新读 live 拼决议。
            let decision_key = crate::sync::full_sync::sync_target_checkpoint_key(planned);
            let decision_facts = decision_key
                .as_ref()
                .and_then(|key| preflight_pending.get(key));
            let (
                target_with_conflicts,
                target_committed_paths,
                target_chapter_changes,
                target_decisions,
            ) = self.commit_transferred_target(
                target_index,
                prepared_failure,
                &staging_runs,
                &target_sync_result,
                &mut proof_recovery_state,
                diagnostic_operation_id,
                decision_facts,
            );
            let commit_outcome =
                crate::api::types::sync_status_to_wire(&target_with_conflicts.result.status);
            commit_phase.finish_with_provider(&commit_outcome, provider);
            // 本地提交产出的不可变对象决议：内容已在提交阶段冻结，这里只做
            // identity → 远端路径映射并收集，发布时不再回读 live。
            let remote_prefix = planned.target.remote_prefix.trim_end_matches('/');
            for decision in &target_decisions {
                let remote_path = match planned.project_id.as_deref() {
                    Some(project_id) => {
                        format!("projects/{project_id}/{}", decision.identity)
                    }
                    None => format!("{remote_prefix}/{}", decision.identity),
                };
                publish_decisions.push((remote_path, decision.clone()));
            }
            if let Some(key) = decision_key.as_ref() {
                // 被远端消费的本地事实只认提交阶段冻结的决议（origin =
                // RemoteConsumed），不再从 transfer 结果列表推测（评审 #7214314073）。
                let consumed: std::collections::BTreeSet<String> = target_decisions
                    .iter()
                    .filter(|decision| {
                        decision.origin == crate::sync::decision::DecisionOrigin::RemoteConsumed
                    })
                    .map(|decision| decision.identity.clone())
                    .collect();
                consumed_by_target.insert(key.clone(), consumed);
            }
            if let Some(ref sink) = progress_sink {
                let finished = u32::try_from(target_index + 1).unwrap_or(u32::MAX);
                sink.update_target_finish(
                    &planned.target.remote_prefix,
                    planned.project_id.as_deref(),
                    finished,
                    total_targets,
                );
            }

            if let Some(run) = staging_runs.get(target_index) {
                run.cleanup();
            }

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
                    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
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
            chapter_changes.extend(target_chapter_changes);
            all_targets.push(target_with_conflicts);
            all_committed_paths.extend(target_committed_paths.into_iter().map(|path| path.path));
        }

        // 本地 commit 之后再判定一次：commit 可能失败（工作区锁/flush/三方合并），
        // 也可能因为并发编辑得到与 staging 不同的结果。任一 target 没有真正落盘
        // 成功就不发布整轮远端树，避免推送未落盘的旧 staging 内容（复审 #2）。
        // 只有"确实失败/未落盘"的 target 才阻断发布：冲突（Conflict /
        // PartialConflict）是成功落盘的终态，冲突路径由 generation 复用远端
        // blob、LWW 排除上传处理，不能因此冻结其它独立对象的发布
        // （Issue #1065 复审 #2、#4）。
        let commit_blocked = all_targets.iter().any(target_blocks_round_publish);
        if commit_blocked {
            round.discard();
            publish_blocked = true;
            log::warn!(
                "[sync] local commit did not fully succeed; remote publish discarded for this round"
            );
        }

        // Issue #729 + #1065 复审 #2：取消只阻止**远端发布**，已经落盘的本地结果
        // 仍要收口——finalize、history、索引失效都要照常执行。提前 return 会让
        // 平台看不到已提交的变更，也会漏写本地 history。
        let cancelled_after_commit = Self::sync_cancelled(cancellation_token.as_ref());
        if cancelled_after_commit {
            round.discard();
            publish_blocked = true;
            log::info!(
                "[sync] cancellation requested after per-target loop — skipping remote publish, keeping local bookkeeping"
            );
        }

        let mut publish_succeeded = false;
        let mut publish_error: Option<String> = None;
        let mut publish_error_category: Option<String> = None;
        let mut publish_ack_unsettled = 0usize;
        // 按不可变决议重建本轮发布批次：内容取自提交阶段冻结的字节，
        // 纯远端消费的条目移除；不再回读 live、不再二次覆盖 manifest。
        // 本轮已决定不发布（冲突/失败）时不再改写缓冲：没有 flush，改写只会
        // 留下口惠而实不至的"决议已应用"。
        let decision_outcome = if publish_blocked {
            crate::sync::decision::DecisionApplyOutcome::default()
        } else {
            round.apply_durable_decisions(&publish_decisions)
        };
        if decision_outcome.rewritten > 0
            || decision_outcome.withheld > 0
            || decision_outcome.missing > 0
        {
            log::info!(
                "[sync] publish batch rebuilt from durable decisions: rewritten={} withheld={} missing={}",
                decision_outcome.rewritten,
                decision_outcome.withheld,
                decision_outcome.missing
            );
        }
        if decision_outcome.missing > 0 {
            log::warn!(
                "[sync] {} decided object(s) missing before publish; they stay unconfirmed",
                decision_outcome.missing
            );
        }
        if !publish_blocked {
            // Issue #1012：整轮 target 的写入在此合并为一次 Git commit + push。
            // 只有真正改变了 tree/catalog 时才 push；完全相同则直接结束。
            // 该 phase 单独记录耗时与网络请求数，日志可区分“传输慢”和“真实失败”。
            let publish_phase = SyncDiagnosticPhase::start_with_provider(
                diagnostic_operation_id,
                "remote_publish",
                None,
                provider,
            );
            let pending_paths = round.pending_len();
            let publish_started_at = Instant::now();
            match round.flush() {
                Ok(outcome) => {
                    // 结算只认 flush 实际放进远端提交的对象事实（评审 #7214314073
                    // 第 2 项），不再用提交前从缓冲推测的 pending 回执。
                    let publish_receipts = outcome.receipts;
                    publish_succeeded = true;
                    if outcome.committed {
                        published_revision = Some(outcome.result.revision.0.clone());
                    }
                    let outcome = if outcome.committed {
                        "committed"
                    } else {
                        "no_changes"
                    };
                    log::info!(
                        "[sync] full-sync remote publish: outcome={} buffered_paths={} duration_ms={}",
                        outcome,
                        pending_paths,
                        publish_started_at.elapsed().as_millis()
                    );
                    publish_phase.finish_with_provider(outcome, provider);
                    // 远端提交成功：按回执确认本轮实际发布的对象事实，推进
                    // known_files 并按序号压缩账本历史前缀。失败/跳过不确认。
                    publish_ack_unsettled = self.confirm_published_targets(
                        preflight_pending,
                        &all_targets,
                        &publish_receipts,
                        &consumed_by_target,
                        diagnostic_operation_id,
                        published_revision.as_deref(),
                    );
                }
                Err(error) => {
                    log::warn!(
                        "[sync] full-sync remote publish failed: buffered_paths={} duration_ms={} error={}",
                        pending_paths,
                        publish_started_at.elapsed().as_millis(),
                        error
                    );
                    publish_phase.finish_with_provider("error", provider);
                    for run in &staging_runs {
                        run.cleanup();
                    }
                    // 本地 commit 已经落盘：远端发布失败不回滚、也不提前返回，
                    // 仍然走 finalize/history/索引失效，让平台看到实际变更与
                    // 明确的失败终态（复审 #2、#3）。
                    // 保留 provider 的真实失败类别：前置条件失败要按"远端已变、
                    // 需要重新拉取差异再推"处理，不能一律丢成通用网络失败。
                    publish_error_category = Some(match &error {
                        crate::sync::provider::ProviderError::PreconditionFailed { .. } => {
                            "remote_precondition_failed".to_string()
                        }
                        crate::sync::provider::ProviderError::RateLimited { .. } => {
                            "remote_rate_limited".to_string()
                        }
                        crate::sync::provider::ProviderError::AuthFailed { .. }
                        | crate::sync::provider::ProviderError::PermissionDenied { .. } => {
                            "remote_auth_failed".to_string()
                        }
                        _ => "remote_publish_failed".to_string(),
                    });
                    publish_error = Some(format!("remote_publish_failed: {error}"));
                }
            }
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

        // Issue #763 评论 5831610228：finalize 前用 set_global_phase("commit") 清掉
        // 单一 current target，表示整轮进入全局 Commit 收口，没有单一 current target。
        if let Some(ref sink) = progress_sink {
            sink.set_global_phase("commit");
        }

        // Phase 5: 聚合 + lifecycle 处理 + FullSyncState 持久化（短写锁）。
        // Issue #779 评论 5853718466：传 None — GC 失败不影响同步终态。
        // 阶段失败（发布失败 / 取消）必须在 finalize 之前汇入持久终态。
        let round_phase_failure = if publish_ack_unsettled > 0 && publish_error.is_none() {
            // 推送成功但确认没有落盘：明确回报未结算，已推送的事实由账本与
            // pending-acks 保留，下一轮重放后再确认（复审 #3）。
            Some(crate::facade::RoundPhaseFailure {
                category: "mutation_ack_unsettled".to_string(),
                message: format!(
                    "remote publish succeeded but {publish_ack_unsettled} target(s) could not be confirmed locally"
                ),
                message_key: None,
            })
        } else if let Some(message) = publish_error.clone() {
            let category = publish_error_category
                .as_deref()
                .unwrap_or("remote_publish_failed");
            Some(crate::facade::RoundPhaseFailure {
                category: category.to_string(),
                message,
                // 前置条件失败不是网络问题：不套 network_failed 文案。
                message_key: if category == "remote_precondition_failed" {
                    None
                } else {
                    Some("sync.result.network_failed".to_string())
                },
            })
        } else if cancelled_after_commit {
            Some(crate::facade::RoundPhaseFailure {
                category: "cancelled".to_string(),
                message: "sync cancelled after local commit; remote publish skipped".to_string(),
                message_key: None,
            })
        } else {
            None
        };
        let (result, committed_paths, lifecycle_receipts) = {
            let core = self.core_write();
            core.finalize_full_sync(all_targets, None, round_phase_failure)
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

        // 远端发布失败：把失败注入终态，平台与 checkpoint 都按失败处理，
        // 但已提交的本地结果、history 与索引失效照常收口（复审 #2、#3）。
        // 阶段失败已在 finalize 前汇入持久终态（复审 #4）：这里只做 DTO 转换，
        // 不能再事后改内存结果，否则 Core 的 last_success 与 UI 不一致。
        let mut result: FullSyncResultDto = result.into();
        result.local_content_changed = local_content_changed;
        result.chapter_changes = chapter_changes.into_iter().map(Into::into).collect();
        Ok(RoundOutcome {
            result,
            published_revision,
            publish_succeeded,
        })
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
        proof_recovery_state: &mut crate::sync::proof_intent::ProofRecoveryState,
        operation_id: &str,
        facts: Option<
            &std::collections::BTreeMap<String, crate::sync::local_mutations::LocalMutationRecord>,
        >,
    ) -> (
        crate::sync::types::TargetSyncResult,
        Vec<crate::sync::commit_helpers::CommittedPath>,
        Vec<crate::chapter::ChapterChangeReceipt>,
        Vec<crate::sync::decision::DurableObjectDecision>,
    ) {
        if prepared_failure {
            // 迁移失败时 staging 未 seed，本组本次不上传也不 Commit。
            return (
                target_sync_result.clone(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );
        }
        match staging_runs.get(target_index) {
            Some(run) => self.commit_target_after_transfer(
                run,
                target_sync_result,
                proof_recovery_state,
                operation_id,
                facts,
            ),
            None => (
                target_sync_result.clone(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        }
    }

    fn commit_target_after_transfer(
        &self,
        run: &crate::sync::staging::StagingRun,
        target_sync_result: &crate::sync::types::TargetSyncResult,
        proof_recovery_state: &mut crate::sync::proof_intent::ProofRecoveryState,
        operation_id: &str,
        facts: Option<
            &std::collections::BTreeMap<String, crate::sync::local_mutations::LocalMutationRecord>,
        >,
    ) -> (
        crate::sync::types::TargetSyncResult,
        Vec<crate::sync::commit_helpers::CommittedPath>,
        Vec<crate::chapter::ChapterChangeReceipt>,
        Vec<crate::sync::decision::DurableObjectDecision>,
    ) {
        // Issue #1039 复核：同步 Commit 与独立 MCP 写事务必须共享应用数据根上的
        // 跨进程工作区锁。锁必须先于 state_lock 获取，并覆盖 live 读取、Store
        // flush、三方 plan、SaveTransaction、组级 state/manifest 写回与账本压缩；
        // 否则 MCP 在 plan 读取 live 后写入的对象会被旧计划覆盖。
        let _workspace_lock = match crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)
        {
            Ok(lock) => lock,
            Err(error) => {
                let mut target_with_error = target_sync_result.clone();
                target_with_error.result.status = crate::sync::SyncStatus::RecoverableError(
                    "workspace_write_lock_failed".to_string(),
                );
                target_with_error.result.error = Some(format!(
                    "cannot acquire workspace write lock before sync commit: {error}"
                ));
                return (target_with_error, Vec::new(), Vec::new(), Vec::new());
            }
        };

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
                return (target_with_conflicts, Vec::new(), Vec::new(), Vec::new());
            }

            let sync_sources =
                staging_manifest_device_ids(run, target_sync_result.project_id.as_deref());
            // Issue #1008 / #1021 复核：上一轮崩溃留下的证明恢复已经在本轮
            // Commit 循环前统一执行；这里只复用结果，不再为每个 target 重扫。
            if let Some(error) = proof_recovery_state.failure().map(str::to_string) {
                let mut target_with_error = target_sync_result.clone();
                target_with_error.result.status = crate::sync::SyncStatus::RecoverableError(
                    "sync_proof_recovery_failed".to_string(),
                );
                target_with_error.result.error = Some(error);
                return (target_with_error, Vec::new(), Vec::new(), Vec::new());
            }
            let intent_root = core.sync_proof_intent_root();
            let intent_sink =
                crate::sync::proof_intent::ProofIntentSink::new(&intent_root, &sync_sources);
            let mut target_with_conflicts = target_sync_result.clone();
            // 积压证明未完成时不能继续改同一项目正文：后续 Commit 会让新的
            // before/after 越过旧事件，证明链将无法按发生顺序接续。
            if let Some(project_id) = target_sync_result.project_id.as_deref() {
                let issues = proof_recovery_state.unresolved_for_project(Some(project_id));
                if !issues.is_empty() {
                    target_with_conflicts.result.status = crate::sync::SyncStatus::RecoverableError(
                        "sync_proof_recovery_pending".to_string(),
                    );
                    let detail = issues
                        .iter()
                        .take(3)
                        .map(|issue| issue.description())
                        .collect::<Vec<_>>()
                        .join("；");
                    target_with_conflicts.result.error = Some(format!(
                        "上次同步正文已落盘但证明事件尚未恢复；先完成证明恢复再同步该作品: {detail}"
                    ));
                    return (target_with_conflicts, Vec::new(), Vec::new(), Vec::new());
                }
            }
            let mut chapter_commit_receipts = Vec::new();
            let (commit_result, conflicts, committed_paths) =
                crate::sync::commit_helpers::commit_single_target_staging(
                    run,
                    target_sync_result,
                    Some(&intent_sink),
                    &mut chapter_commit_receipts,
                );
            let journal_chapter_keys: std::collections::HashSet<(String, String, String)> =
                chapter_commit_receipts
                    .iter()
                    .map(|receipt| {
                        (
                            receipt.key.project_id.clone(),
                            receipt.key.volume_id.clone(),
                            receipt.key.chapter_id.clone(),
                        )
                    })
                    .collect();
            core.refresh_sync_proof_intents_pending();

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
            // 文件已经落盘但事务 finish/冲突快照失败时，committed_paths 仍
            // 非空；证明事实必须补记，否则下一轮会因缺少 SyncRemote 事件而
            // 把真实同步变更误判为旧快照。
            if matches!(
                commit_result,
                crate::sync::commit_helpers::TargetCommitResult::Ok
            ) || !committed_paths.is_empty()
                || !chapter_commit_receipts.is_empty()
            {
                let mut proof_errors: Vec<String> = Vec::new();
                let mut proof_completed_ids = std::collections::HashSet::new();
                // 普通同步的章节正文/证明事实已写入同一 chapter journal；
                // 这些事实由 receipt 的 transaction_id/event_id 驱动投影 worker，
                // 不能再用 committed_paths 的枚举序号反查旧 intent。
                for (action_index, committed) in committed_paths.iter().enumerate() {
                    let path = &committed.path;
                    let Some((project_id, volume_id, chapter_id)) = chapter_path_parts(path) else {
                        continue;
                    };
                    if journal_chapter_keys.contains(&(
                        project_id.clone(),
                        volume_id.clone(),
                        chapter_id.clone(),
                    )) {
                        continue;
                    }
                    let before_text = match &committed.before {
                        crate::sync::commit_helpers::CommittedBefore::Missing => None,
                        crate::sync::commit_helpers::CommittedBefore::Text { bytes, .. } => {
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
                    // prepare 对无内容变化的动作不生成意图；证明循环也必须跳过，
                    // 否则会把正常的 no-op 当成“意图缺失”。
                    if before_text.as_deref().unwrap_or("") == after_text.as_deref().unwrap_or("") {
                        continue;
                    }
                    // run_id + 落盘路径 + 动作序号唯一标识一次真实同步提交；同一
                    // 章节 A→B→A→B 不会被内容 hash 误去重。事件 ID 固定后重发复用。
                    let sync_event_id =
                        format!("sync:{}:{}:{}", run.run_id(), path.display(), action_index);
                    // Issue #1062 1.4：已提交正文不能因为旧 SyncProofIntent 不存在
                    // 而判同步失败。意图只是“设备/发生时间”的持久化载体；缺失时用
                    // 本次提交的真实 before/after、固定 event ID 和当前时间补记，
                    // 证明链仍按真实正文接续。
                    let (source_device, occurred_at_ms) = match intent_sink.load(&sync_event_id) {
                        Ok(Some(intent)) => {
                            let expected_action = if committed.deleted {
                                crate::sync::proof_intent::SyncProofAction::Delete
                            } else {
                                crate::sync::proof_intent::SyncProofAction::Apply
                            };
                            if intent.event_id != sync_event_id
                                || intent.action_index != action_index
                                || intent.workspace_path != path.display().to_string()
                                || intent.action != expected_action
                                || intent.is_delete() != committed.deleted
                                || intent.project_id != project_id
                                || intent.volume_id != volume_id
                                || intent.chapter_id != chapter_id
                                || intent.occurred_at_ms <= 0
                                || intent.before_text.as_deref() != before_text.as_deref()
                                || intent.after_text.as_deref() != after_text.as_deref()
                            {
                                proof_errors.push(format!(
                                    "同步证明意图与本次提交不匹配，已保留待恢复: {}",
                                    sync_event_id
                                ));
                                continue;
                            }
                            (intent.source_device_id, intent.occurred_at_ms)
                        }
                        Ok(None) => {
                            // 缺意图不再阻塞：device 交给 Core 的稳定身份，
                            // 时间取本次同步提交的真实时刻。
                            log::warn!(
                                "sync proof intent missing for committed chapter, \
                                 recording fact without intent: {sync_event_id}"
                            );
                            (String::new(), chrono::Utc::now().timestamp_millis())
                        }
                        Err(error) => {
                            proof_errors.push(format!(
                                "读取同步证明意图失败，已保留待恢复 {}: {error}",
                                sync_event_id
                            ));
                            continue;
                        }
                    };
                    match core.record_sync_content_change_at(
                        &project_id,
                        &volume_id,
                        &chapter_id,
                        before_text.as_deref(),
                        after_text.as_deref(),
                        &sync_event_id,
                        source_device.as_str(),
                        "",
                        occurred_at_ms,
                    ) {
                        Ok(_) => {
                            proof_recovery_state.mark_event_persisted(&sync_event_id);
                            proof_completed_ids.insert(sync_event_id.clone());
                        }
                        Err(e) => proof_errors.push(format!("{}: {e}", path.display())),
                    }
                }
                // 先删除仅因证明未写完而保留的事务目录，再删除意图。若进程在
                // 两步之间退出，残留意图会因事件已落盘而被下次恢复直接清理；
                // 反过来先删意图则会留下“事务已提交但意图缺失”的不可判定状态。
                let retained_ids = intent_sink
                    .cleanup_completed_transactions(run.target_live_root(), &proof_completed_ids);
                for event_id in proof_completed_ids.difference(&retained_ids) {
                    if let Err(error) = intent_sink.remove(event_id) {
                        log::warn!("remove completed sync proof intent {event_id} failed: {error}");
                    }
                }
                core.refresh_sync_proof_intents_pending();
                if !chapter_commit_receipts.is_empty() {
                    // 正文已落盘；立即按回执所属 project root 结算 journal 证明。
                    // app target 的 live_root 是 projects 的父目录，不能拿它当
                    // 项目根扫描 journal。投影失败只保留 pending，不影响正文提交。
                    let target_root = std::path::Path::new(run.target_live_root());
                    let mut project_roots = std::collections::HashSet::new();
                    for receipt in &chapter_commit_receipts {
                        let project_root = if target_root.join("projects").is_dir() {
                            target_root.join("projects").join(&receipt.key.project_id)
                        } else {
                            target_root.to_path_buf()
                        };
                        project_roots.insert(project_root);
                    }
                    for project_root in project_roots {
                        core.retry_pending_chapter_proof_projections(&project_root);
                    }
                }
                if !proof_errors.is_empty() {
                    let detail = proof_errors.join("; ");
                    // 本轮后续 target 复用同一状态：本组证明未写完时，同项目
                    // 不应继续改正文，避免旧事件被新的 before/after 越过。
                    proof_recovery_state.record_issue(
                        crate::sync::proof_intent::ProofRecoveryIssue {
                            project_id: target_sync_result.project_id.clone(),
                            transaction_id: None,
                            event_id: None,
                            missing_fields: Vec::new(),
                            message: format!("同步正文已落盘，但证明事件记录失败: {detail}"),
                        },
                    );
                    target_with_conflicts.result.status = crate::sync::SyncStatus::RecoverableError(
                        "sync_proof_record_failed".to_string(),
                    );
                    target_with_conflicts.result.error =
                        Some(format!("同步正文已落盘，但证明事件记录失败: {detail}"));
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

            // receipt 在持有 workspace 锁时按真实 meta/正文生成；释放锁后不再
            // 回读磁盘，避免晚到的外部写入污染本次同步结果。
            let chapter_receipts = crate::sync::commit_helpers::build_chapter_change_receipts(
                &self.app_data_root,
                &committed_paths,
                operation_id,
            );
            // Issue #1062 评论 #7214330885（接手 #1065 P0 第 1 项）：决议在
            // 已持有工作区写锁、刚完成业务落盘时冻结，之后不再回读 live。
            // 这里仍读取一次刚提交的字节，但读取期间没有并发写者；事务身份、
            // 版本/hash 与内容来自同一次持锁结果，不会出现 A 事务 + B 正文。
            let empty_facts = std::collections::BTreeMap::new();
            let mut decisions = crate::sync::commit_helpers::durable_decisions_for_target(
                run.target_live_root(),
                target_with_conflicts.project_id.as_deref(),
                &target_with_conflicts.result,
                &committed_paths,
                facts.unwrap_or(&empty_facts),
            );
            // manifest 与对象同源：本次持锁提交刚写回的 manifest 在同一快照里
            // 冻结成决议，发布路径不再在锁外回读。
            let manifest_live = match target_with_conflicts
                .group_id
                .as_deref()
                .and_then(|group_id| crate::sync::types::AppDataGroup::from_id(group_id).ok())
            {
                Some(group) => run.target_live_root().join(group.local_manifest_path()),
                None => run
                    .target_live_root()
                    .join(crate::sync::lww::SYNC_MANIFEST_PATH),
            };
            if let Ok(bytes) = std::fs::read(&manifest_live) {
                let bytes = serde_json::from_slice::<crate::sync::types::SyncManifest>(&bytes)
                    .ok()
                    .and_then(|manifest| manifest.canonical_json().ok())
                    .map(String::into_bytes)
                    .unwrap_or(bytes);
                decisions.push(crate::sync::decision::DurableObjectDecision {
                    transaction_id: operation_id.to_string(),
                    identity: crate::sync::lww::SYNC_MANIFEST_PATH.to_string(),
                    op: crate::sync::decision::DecisionOp::Upsert,
                    base_hash: None,
                    final_hash: crate::sync::hash::content_md5(&bytes),
                    final_version: None,
                    mutation_seq: 0,
                    origin: crate::sync::decision::DecisionOrigin::Local,
                    content: Some(bytes),
                    dependencies: Vec::new(),
                });
            }
            (
                target_with_conflicts,
                committed_paths,
                chapter_receipts,
                decisions,
            )
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
        let changed_starmap_ids = starmap_ids_for_committed_paths(
            core,
            committed_paths.iter().map(|path| path.path.as_path()),
        )?;
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
        // 与 MCP/自动同步共用应用数据根的跨进程写锁；锁顺序固定为
        // workspace → state，避免与同步提交互相交错。
        let _workspace_lock = crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)
            .map_err(crate::api::error::WriterError::from)?;
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
        // 冲突解决会直接替换/删除 live 星图对象，必须与独立 MCP 写事务、
        // 同步提交共用同一把应用数据根写锁，并保持到索引刷新结束。
        let _workspace_lock = crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)
            .map_err(crate::api::error::WriterError::from)?;

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
        // 合并后的结果）并失效路径依赖索引。共享节点定义变更要通过派生引用索引
        // 找出所有受影响的真实画布，不能把 `library` 当成一张星图。
        if applied {
            let conflict_path = std::path::Path::new(path);
            if classify_starmap_committed_path(conflict_path).is_some() {
                let core = self.core_write();
                let changed_starmap_ids =
                    starmap_ids_for_committed_paths(&core, std::iter::once(conflict_path))?;
                for starmap_id in &changed_starmap_ids {
                    core.evict_starmap_store_after_commit(starmap_id)?;
                }
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
        // 与 MCP/自动同步共用应用数据根写锁；锁顺序固定为 workspace → state。
        let _workspace_lock = crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)
            .map_err(crate::api::error::WriterError::from)?;
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

/// workspace-relative 星图路径的语义分类。
///
/// 同步 commit 后要失效 Store，但共享节点定义位于
/// `starmaps/library/nodes/...`，不能再被解析成一张名为 `library` 的画布。
#[derive(Debug, Clone, PartialEq, Eq)]
enum StarmapCommittedPath {
    /// 某张真实画布：`starmaps/{id}.meta.json` 或 `starmaps/{id}/...`
    Canvas(String),
    /// 共享节点定义：`starmaps/library/nodes/{bucket}/{asset_id}.json`
    SharedNode(String),
    /// 素材库索引/派生索引等不属于单张画布的库文件。
    LibraryIndex,
}

/// 从 workspace-relative 路径分类星图变更。
///
/// `starmaps/index.json` 不对应任何画布或共享定义，返回 `None`。
fn classify_starmap_committed_path(path: &std::path::Path) -> Option<StarmapCommittedPath> {
    let rel = path.to_str()?;
    let rest = rel.strip_prefix("starmaps/")?;
    if rest.is_empty() || rest == "index.json" {
        return None;
    }
    if rest == "library" || rest.starts_with("library/") {
        if let Some(asset_path) = rest.strip_prefix("library/nodes/") {
            let mut parts = asset_path.split('/');
            let bucket = parts.next()?;
            let file = parts.next()?;
            if !bucket.is_empty() && parts.next().is_none() {
                if let Some(asset_id) = file.strip_suffix(".json") {
                    if !asset_id.is_empty() {
                        return Some(StarmapCommittedPath::SharedNode(asset_id.to_string()));
                    }
                }
            }
        }
        return Some(StarmapCommittedPath::LibraryIndex);
    }
    // starmaps/{id}.meta.json
    if let Some(id) = rest.strip_suffix(".meta.json") {
        if !id.is_empty() {
            return Some(StarmapCommittedPath::Canvas(id.to_string()));
        }
        return None;
    }
    // starmaps/{id}/...
    if let Some(slash_idx) = rest.find('/') {
        let id = &rest[..slash_idx];
        if !id.is_empty() {
            return Some(StarmapCommittedPath::Canvas(id.to_string()));
        }
    }
    None
}

/// 收集一次 commit / 冲突解决后需要 evict 的真实画布 ID。
///
/// 共享节点定义变更通过同步 commit 后已刷新的派生引用索引定位所有引用画布；
/// 库索引本身不产生画布缓存失效。
fn starmap_ids_for_committed_paths<'a>(
    core: &crate::facade::WriterCore,
    paths: impl IntoIterator<Item = &'a std::path::Path>,
) -> crate::error::Result<std::collections::HashSet<String>> {
    let mut starmap_ids = std::collections::HashSet::new();
    let mut shared_assets = std::collections::HashSet::new();
    for path in paths {
        match classify_starmap_committed_path(path) {
            Some(StarmapCommittedPath::Canvas(starmap_id)) => {
                starmap_ids.insert(starmap_id);
            }
            Some(StarmapCommittedPath::SharedNode(asset_id)) => {
                shared_assets.insert(asset_id);
            }
            Some(StarmapCommittedPath::LibraryIndex) | None => {}
        }
    }
    for asset_id in shared_assets {
        for instance in
            crate::starmap::library::instances_for_asset(&core.app_data_root, &asset_id)?
        {
            starmap_ids.insert(instance.starmap_id);
        }
    }
    Ok(starmap_ids)
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

    /// `classify_starmap_committed_path` 区分真实画布、共享定义和库索引。
    #[test]
    fn classify_starmap_committed_paths_keeps_library_out_of_canvases() {
        use super::{classify_starmap_committed_path, StarmapCommittedPath};
        use std::path::PathBuf;

        let canvas = |path: &str| classify_starmap_committed_path(&PathBuf::from(path));
        assert_eq!(
            canvas("starmaps/sm_1.meta.json"),
            Some(StarmapCommittedPath::Canvas("sm_1".to_string()))
        );
        assert_eq!(
            canvas("starmaps/sm_1/graph.json"),
            Some(StarmapCommittedPath::Canvas("sm_1".to_string()))
        );
        assert_eq!(
            canvas("starmaps/sm_1/nodes/00/n1.json"),
            Some(StarmapCommittedPath::Canvas("sm_1".to_string()))
        );
        assert_eq!(
            canvas("starmaps/sm_1/edges/e1.json"),
            Some(StarmapCommittedPath::Canvas("sm_1".to_string()))
        );
        assert_eq!(
            canvas("starmaps/library/nodes/af/asset_1.json"),
            Some(StarmapCommittedPath::SharedNode("asset_1".to_string()))
        );
        assert_eq!(
            canvas("starmaps/library/index.json"),
            Some(StarmapCommittedPath::LibraryIndex)
        );
        assert_eq!(
            canvas("starmaps/library/references.json"),
            Some(StarmapCommittedPath::LibraryIndex)
        );
        assert_eq!(canvas("starmaps/index.json"), None);
        assert_eq!(canvas("projects/p1/chapter.json"), None);
        assert_eq!(canvas("starmaps/"), None);
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

#[cfg(test)]
mod shared_definition_path_tests {
    use super::{
        classify_starmap_committed_path, starmap_ids_for_committed_paths, StarmapCommittedPath,
    };
    use crate::facade::WriterCore;
    use crate::starmap::semantic::{StarMapNodeContent, StarMapProvenance};
    use crate::starmap::types::{StarMapNode, StarMapNodeKind, StarMapNodeStyle, StarMapPoint};
    use tempfile::tempdir;

    fn node(id: &str) -> StarMapNode {
        StarMapNode {
            id: id.to_string(),
            asset_id: id.to_string(),
            title: id.to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            content: StarMapNodeContent::Empty,
            anchors: vec![],
            portal: None,
            position: StarMapPoint::default(),
            style: StarMapNodeStyle::default(),
            provenance: StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
            definition_revision: 0,
        }
    }

    /// 同步共享定义后，所有引用该 asset 的画布都必须进入 evict 集合。
    #[test]
    fn shared_node_definition_path_resolves_all_referencing_canvases() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join("projects");
        std::fs::create_dir_all(&projects_root).unwrap();
        let core = WriterCore::new(temp.path(), &projects_root);

        let first = core.create_starmap("画布 A", "", None).unwrap().starmap_id;
        let second = core.create_starmap("画布 B", "", None).unwrap().starmap_id;
        let source = node("n_shared");
        core.add_starmap_node(&first, source, 0.0, 0.0).unwrap();
        core.add_starmap_node_reference(&second, "n_shared", StarMapPoint::default(), None, None)
            .unwrap();

        let definition_path = crate::starmap::library::node_definition_rel_path("n_shared");
        assert_eq!(
            classify_starmap_committed_path(&definition_path),
            Some(StarmapCommittedPath::SharedNode("n_shared".to_string()))
        );

        let affected = starmap_ids_for_committed_paths(&core, [definition_path.as_path()]).unwrap();
        assert!(affected.contains(&first));
        assert!(affected.contains(&second));
    }
}
