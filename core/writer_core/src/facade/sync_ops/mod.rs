//! 同步 facade — 全量同步统一入口。
//!
//! 一个全局 `SyncConfig` + 一份全局凭据，`perform_full_sync` 内部按 `SyncTarget`
//! 把不同本地根映射到同一个远端仓库的不同前缀：
//! - App target：`<app_data_root>` → `app/`
//! - Project target：`<project_root>` → `projects/<project_id>/`
//!
//! ## FullSyncState 生命周期
//!
//! 全量同步持久状态（`<app_data_root>/app-meta/sync/full_state.local.json`）在
//! 三个时点原子写入，保证失败/中断不会留下旧绿灯：
//! 1. `perform_full_sync()` 一进正式事务先写 `Syncing` + 本次 attempt 时间
//!    （[`WriterCore::persist_full_sync_started`]）；进程中断后重启读到 Syncing；
//! 2. target 开始执行前失败（transport 初始化 / `list_projects` / 平台预处理）
//!    写失败状态 + `"preflight"` / `"global"` 标记
//!    （[`WriterCore::persist_full_sync_early_failure`]、
//!    [`WriterCore::record_full_sync_preflight_failure`]）；
//! 3. target 全部执行、聚合完成后用 `FullSyncState::from_result_and_previous`
//!    覆盖为终态。
//!
//! 三个时点都保留旧 `last_success_time`，只有整体成功类才更新它。
//!
//! ## 聚合优先级
//!
//! `aggregate_full_sync_result` 按"需要用户处理的终态 > 可重试 > 成功"保留错误类型：
//! `Fatal/Error > Dirty > Conflict > Recoverable > Success`。`error` /
//! `error_category` / `message_key` 从与总体同优先级的第一个 dominant target 取得，
//! 避免"总体是认证失败、文案却拿到前一个网络错误"的错位。

mod commit;
mod dry_run;
mod prepare;

use crate::sync::full_sync::transport_init_failure_error;
use crate::sync::full_sync_utils::*;

impl super::WriterCore {
    /// 全量同步 — 先建立 App target，再枚举所有作品建立 Project target；
    /// 共享同一份 config / secrets snapshot，按 target 顺序执行。
    /// 一个 target 的状态/manifest 仍写在它自己的本地 root 下。
    ///
    /// 单个 target 的 `Err`（本地 root IO 错、transport 调用失败等）不提前打断
    /// 整个全量同步：该 target 的 Err 被转为 `SyncResult::error(...)` 后 push 到
    /// `targets`，继续下一 target。只有无法建立 target 列表（`list_projects`
    /// 失败）或全局配置无法解析/transport 初始化失败这类无法开始事务的错误才让
    /// 整个 `perform_full_sync` 返回 `Err`。
    ///
    ///   降级为 `pub(crate)` `#[cfg(test)]`，只给内部测试用作底层 helper。
    /// 生产同步唯一 pipeline 是 `WriterAppService::perform_full_sync` →
    /// `WriterCoreApi::perform_full_sync`（Prepare → Seed → Transfer → Commit），
    /// 它会加载 pending deleted targets、走三段式 staging + workspace history。
    /// 本方法不加载 pending deleted targets、不走 staging，是旧编排，不能被
    /// FFI/UniFFI 等生产入口调用。
    #[cfg(test)]
    pub(crate) fn perform_full_sync(
        &self,
        config: &crate::sync::SyncConfig,
        force_sync: bool,
    ) -> crate::error::Result<crate::sync::types::FullSyncResult> {
        // 一进正式事务先原子写 Syncing 本次 attempt
        // 时间（保留旧 last_success_time）。进程中断/被杀后重启读到的是 Syncing，
        // 而不是上一次 Success 绿灯。
        self.persist_full_sync_started();

        let secrets = self.load_sync_secrets().unwrap_or_default();
        let provider = self.create_sync_provider_for_plan(config, &secrets)?;
        let sync_policy = crate::sync::types::SyncPolicy::from_config(config);
        self.perform_full_sync_with_provider(provider.as_ref(), &sync_policy, force_sync)
    }

    /// 内部：用给定 provider 执行全量同步。
    ///
    /// `perform_full_sync` 创建 provider 后委托到此方法；测试通过此方法注入 mock provider。
    /// 语义与 `perform_full_sync` 一致：单个 target 的 `Err` 转为该 target 的
    /// `SyncResult::error(...)` 后继续，只有 `list_projects` 失败才整体 `Err`。
    ///
    ///   降级为 `#[cfg(test)]`，只给内部测试用。
    #[cfg(test)]
    pub(crate) fn perform_full_sync_with_provider(
        &self,
        provider: &dyn crate::sync::provider::SyncProvider,
        sync_policy: &crate::sync::types::SyncPolicy,
        force_sync: bool,
    ) -> crate::error::Result<crate::sync::types::FullSyncResult> {
        use crate::sync::types::{SyncTarget, TargetSyncResult};

        // 无法建立 target 列表才整体 Err —— 此时连 App target 都无法有序执行。
        // list_projects 失败也要先持久化提前失败状态
        let projects = match self.list_projects() {
            Ok(projects) => projects,
            Err(err) => {
                let msg = err.to_string();
                self.persist_full_sync_early_failure(
                    crate::sync::SyncStatus::RecoverableError(msg),
                    "global",
                );
                return Err(err);
            }
        };

        let mut targets: Vec<TargetSyncResult> = Vec::new();

        // App target
        let app_target = SyncTarget::app();
        let app_result = run_full_sync_target(
            provider,
            &self.app_data_root,
            sync_policy,
            &app_target,
            force_sync,
        );
        targets.push(TargetSyncResult {
            target_kind: "app".to_string(),
            project_id: None,
            remote_prefix: app_target.remote_prefix.clone(),
            result: app_result,
            deleted_resolution: None,
            local_lifecycle_action: crate::sync::types::LocalLifecycleCommitAction::None,
        });

        // Project targets
        for project in &projects {
            let target = SyncTarget::project(&project.id);
            let result = run_full_sync_target(
                provider,
                &self.project_root(&project.id),
                sync_policy,
                &target,
                force_sync,
            );
            targets.push(TargetSyncResult {
                target_kind: "project".to_string(),
                project_id: Some(project.id.clone()),
                remote_prefix: target.remote_prefix.clone(),
                result,
                deleted_resolution: None,
                local_lifecycle_action: crate::sync::types::LocalLifecycleCommitAction::None,
            });
        }

        let result = Self::aggregate_full_sync_result(targets);

        //    5308040939 ：聚合后把 FullSyncState
        // 原子写到 <app_data_root>/app-meta/sync/full_state.local.json，覆盖事务开始
        // 时写入的 Syncing。每次尝试更新 last_attempt_time；仅整体成功类更新
        // last_success_time；部分失败保留旧值。
        // 写失败只记录警告，不覆盖同步结果（同步本身已成功，状态持久化是副作用）。
        let previous_state = self.load_full_sync_state().unwrap_or(None);
        let new_state = crate::sync::full_sync_state::FullSyncState::from_result_and_previous(
            &result,
            previous_state.as_ref(),
            now_epoch_seconds(),
        );
        if let Err(e) = self.save_full_sync_state(&new_state) {
            log::warn!("Failed to persist full sync state: {e}");
        }

        // 同步成功后重建搜索索引
        if matches!(
            result.overall_status,
            crate::sync::SyncStatus::Success | crate::sync::SyncStatus::LatestWinsApplied
        ) {
            if let Err(e) = self.rebuild_search_index(None) {
                log::warn!("Failed to rebuild search index after full sync: {e}");
                // 不覆盖总体状态，只记录警告
            }
        }

        Ok(result)
    }

    /// 将各 target 的结果聚合为 `FullSyncResult`：统计上传/下载/删除/冲突数，
    /// 总体状态保留错误类型，优先级按"需要用户处理的终态 > 可重试 > 成功"：
    /// `Fatal/Error > Dirty > Conflict/PartialConflict > Recoverable > Success`
    /// 。
    ///
    /// `error` / `error_category` / `message_key` 从与 `overall_status` 同优先级的
    /// 第一个 dominant target 取得，避免"总体是认证失败、文案却拿到前一个网络错误"
    /// 的错位。
    ///
    ///   降级为 `#[cfg(test)]`，只给 `perform_full_sync_with_provider` 用。
    /// 生产路径用 `crate::sync::full_sync::aggregate_full_sync_result`（pub 函数）。
    #[cfg(test)]
    fn aggregate_full_sync_result(
        targets: Vec<crate::sync::types::TargetSyncResult>,
    ) -> crate::sync::types::FullSyncResult {
        use crate::sync::types::FullSyncResult;
        use crate::sync::SyncStatus;

        let total_uploaded: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.result.uploaded_files.len()).unwrap_or(u32::MAX))
            .sum();
        let total_downloaded: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.result.downloaded_files.len()).unwrap_or(u32::MAX))
            .sum();
        let total_local_deletes: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.result.local_deletes.len()).unwrap_or(u32::MAX))
            .sum();
        let total_remote_deletes: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.result.remote_deletes.len()).unwrap_or(u32::MAX))
            .sum();
        let total_overwritten: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.result.overwritten_files.len()).unwrap_or(u32::MAX))
            .sum();
        let total_ignored: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.result.ignored_files.len()).unwrap_or(u32::MAX))
            .sum();
        let total_conflicts: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.result.conflicts.len()).unwrap_or(u32::MAX))
            .sum();

        // 终态分两步聚合。
        // 第一步：任何 target 返回 Syncing/Idle/ConfiguredNotTested 都是协议错误
        // （这三个是非终态/未测试状态，不应出现在 target 结果里），直接生成
        // FatalError，绝不能当成功。
        if let Some((overall_status, error, error_category, message_key)) =
            build_protocol_error_fields(&targets)
        {
            return FullSyncResult {
                overall_status,
                targets,
                total_uploaded,
                total_downloaded,
                total_local_deletes,
                total_remote_deletes,
                total_overwritten,
                total_ignored,
                total_conflicts,
                error,
                error_category,
                message_key,
            };
        }

        let overall_priority = targets
            .iter()
            .map(|t| full_sync_status_priority(&t.result.status))
            .max()
            .unwrap_or(0);
        let overall_status = match overall_priority {
            4 => SyncStatus::FatalError("one_or_more_targets_failed".to_string()),
            3 => SyncStatus::PartialConflict,
            1 => SyncStatus::RecoverableError("one_or_more_targets_temporarily_failed".to_string()),
            _ => aggregate_success_status(&targets),
        };

        // dominant target：与 overall_status 同优先级的第一个 target。
        let dominant = targets
            .iter()
            .find(|t| full_sync_status_priority(&t.result.status) == overall_priority);
        let error = dominant.and_then(|t| t.result.error.clone());
        let error_category = dominant.and_then(|t| t.result.error_category.clone());
        let message_key = dominant
            .and_then(|t| t.result.message_key.clone())
            .or_else(|| {
                error_category.as_deref().map(|c| {
                    crate::sync::types::SyncErrorCategory::from_code(c, "")
                        .to_message_key()
                        .to_string()
                })
            });

        FullSyncResult {
            overall_status,
            targets,
            total_uploaded,
            total_downloaded,
            total_local_deletes,
            total_remote_deletes,
            total_overwritten,
            total_ignored,
            total_conflicts,
            error,
            error_category,
            message_key,
        }
    }

    // ── 共用内部 ──

    fn run_sync_diagnostics(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<crate::sync::SyncDiagnosticsResult> {
        // secrets 仅在 github-api feature 下使用；非 github-api 时消费以避免 unused。
        #[cfg(not(feature = "github-api"))]
        let _ = secrets;
        match config.active_provider.as_str() {
            #[cfg(feature = "github-api")]
            "github_api" => {
                let transport = self.init_sync_transport()?;
                let github_config = config
                    .provider_config
                    .as_ref()
                    .map(|pc| match pc {
                        crate::sync::provider::ProviderConfig::GitHub(c) => c,
                    })
                    .ok_or_else(|| crate::Error::Other("missing github provider config".into()))?;
                let runtime =
                    crate::sync::provider::github::config::GitHubRuntimeConfig::from_persisted(
                        github_config,
                        secrets.provider_secrets.as_ref(),
                    )
                    .map_err(crate::Error::from)?;
                let provider =
                    crate::sync::provider::github::GitHubProvider::new(runtime, transport);
                provider.diagnose().map_err(crate::Error::from)
            }
            #[cfg(not(feature = "github-api"))]
            "github_api" => Err(crate::Error::NotImplemented),
            _ => Err(crate::Error::NotImplemented),
        }
    }

    /// 初始化同步传输 — 从平台注入的 factory 构造 `Arc<dyn SyncTransport>`。
    ///
    /// transport 初始化失败返回类型化 `Error`；调用方决定是否持久化失败状态。
    /// 把 `match backend` → `if let Some(factory)` → `match factory()` 三层嵌套收成一个方法。
    fn init_sync_transport(
        &self,
    ) -> crate::error::Result<std::sync::Arc<dyn writer_platform_api::SyncTransport>> {
        match self.sync_transport.as_ref() {
            Some(transport_fn) => match transport_fn() {
                Ok(t) => Ok(std::sync::Arc::from(t)),
                Err(e) => Err(transport_init_failure_error(&e.category, &e.message)),
            },
            None => Err(crate::Error::SyncNetworkUnavailable {
                reason: "no SyncTransport configured".to_string(),
            }),
        }
    }

    /// 按 remote_prefix 移除 pending_remote_cleanup。
    fn remove_pending_remote_cleanup_by_prefix(&self, remote_prefix: &str) {
        if let Err(e) = crate::sync::pending_remote_cleanup::remove_pending_remote_cleanup(
            &self.app_data_root,
            remote_prefix,
        ) {
            log::warn!(
                "commit_full_sync: remove_pending_remote_cleanup failed for {}: {} \
                 — pending will be retried next sync",
                remote_prefix,
                e
            );
        }
    }

    /// 按 remote_prefix 找到 pending deleted target 并移除。
    fn remove_pending_deleted_by_prefix(&self, remote_prefix: &str) {
        let pending =
            crate::sync::pending_deleted::load_pending_deleted_targets(&self.app_data_root)
                .unwrap_or_default();
        let Some(matched) = pending
            .iter()
            .find(|p| p.target.remote_prefix == remote_prefix)
        else {
            return;
        };
        if let Err(e) = crate::sync::pending_deleted::remove_pending_deleted_target(
            &self.app_data_root,
            &matched.journal_token,
        ) {
            log::warn!(
                "commit_full_sync: remove_pending_deleted_target failed for {}: {} \
                 — entry retained, will retry next sync",
                remote_prefix,
                e
            );
        }
    }
}
