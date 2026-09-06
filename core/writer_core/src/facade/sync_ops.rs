//! 同步 facade — 全量同步统一入口（Issue #630）。
//!
//! 一个全局 `SyncConfig` + 一份全局凭据，`perform_full_sync` 内部按 `SyncTarget`
//! 把不同本地根映射到同一个远端仓库的不同前缀：
//! - App target：`<app_data_root>` → `app/`
//! - Project target：`<project_root>` → `projects/<project_id>/`
//!
//! ## FullSyncState 生命周期（Issue #630 评论 5308040939 Part 1）
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
//! ## 聚合优先级（Issue #630 评论 5308040939 Part 2）
//!
//! `aggregate_full_sync_result` 按"需要用户处理的终态 > 可重试 > 成功"保留错误类型：
//! `Fatal/Error > Dirty > Conflict > Recoverable > Success`。`error` /
//! `error_category` / `message_key` 从与总体同优先级的第一个 dominant target 取得，
//! 避免"总体是认证失败、文案却拿到前一个网络错误"的错位。

use crate::sync::full_sync_utils::*;

impl super::WriterCore {
    /// 全量同步诊断 — 只测一次仓库、分支、token。
    ///
    /// `secrets` 由调用方传入（API 层已 snapshot override），不再内部加载。
    pub fn perform_full_sync_diagnostics(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<crate::sync::types::FullSyncDiagnosticsResult> {
        let diagnostics = self.run_sync_diagnostics(config, secrets)?;
        Ok(crate::sync::types::FullSyncDiagnosticsResult { diagnostics })
    }

    /// #645 评论 5504296097 问题4：全量同步 dry-run — 枚举 App target + 所有 Project target + pending deleted targets，
    /// 构建每个 target 的计划。
    ///
    /// `secrets` 由调用方传入（API 层已 snapshot override），不再内部加载。
    ///
    /// #645 评论 5504296097 问题4：调用共享 `build_full_sync_target_plan` 枚举 targets，
    /// dry-run 也包含 pending deleted target（`target_kind="deleted_project"`），
    /// 不再只看 live Project targets。deleted target 的 `SyncPlan` 为空（dry-run 不读远端，
    /// 无法知道远端对象数；调用方据 `target_kind` 判断将删除/恢复）。
    ///
    /// #645 评论 5504296097 问题5：dry-run 读真实远端 catalog（read-only 网络 IO），
    /// 不再传空 catalog。catalog 读取失败时返回错误（dry-run 是预览，不能返回假的远端事实）。
    pub fn perform_full_sync_dry_run(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<crate::sync::types::FullSyncDryRunResult> {
        // #645 评论 5504296097 问题6：catalog 读取（网络 IO）由 API 层在
        // core_write() 锁外执行，传入已加载的 catalog。此处只做本地 plan 构建。
        // 保留 fallback：若 API 层未预加载 catalog（旧调用方），在此加载。
        let remote_catalog = if config.enabled {
            self.dry_run_load_remote_catalog(config, secrets)?
        } else {
            crate::sync::types::TargetLifecycleCatalog::default()
        };
        self.perform_full_sync_dry_run_with_catalog(config, &remote_catalog)
    }

    /// #645 评论 5504296097 问题6：dry-run plan 构建（纯本地 IO，不做网络 IO）。
    ///
    /// `remote_catalog` 由 API 层在 core_write() 锁外预加载后传入。
    pub(crate) fn perform_full_sync_dry_run_with_catalog(
        &self,
        config: &crate::sync::SyncConfig,
        remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    ) -> crate::error::Result<crate::sync::types::FullSyncDryRunResult> {
        use crate::sync::types::{FullSyncDryRunResult, SyncPlan, TargetSyncPlan};

        let projects = self.list_projects()?;

        // #645 评论 5504296097 问题4：加载 pending deleted targets，让 dry-run 也能看到 deleted target。
        let pending_deleted =
            crate::sync::pending_deleted::load_pending_deleted_targets(&self.app_data_root)?;

        let sync_policy = crate::sync::types::SyncPolicy::from_config(config);

        // #645 评论 5504296097 问题1：device_id 来自真实 DeviceInfo。
        let device_id = crate::settings::load_device_info(&self.app_data_root)
            .map(|info| info.device_id)
            .unwrap_or_default();

        // #645 评论 5504296097 问题4：调用共享 planner 枚举 targets，
        // 不复制一套 target 枚举逻辑。
        // #645 评论 5504296097 问题3 修复：加载 pending_remote_cleanups，
        // 让上一轮 cleanup 失败的远端残留能在本轮重试。
        // #645 评论 5504296097 问题3 修复：持久化错误向上传递，不再 unwrap_or_default()。
        let pending_remote_cleanups =
            crate::sync::pending_remote_cleanup::load_pending_remote_cleanups(&self.app_data_root)?;
        let planned_targets = crate::sync::full_sync::build_full_sync_target_plan(
            &self.app_data_root,
            &self.projects_root,
            &projects,
            &pending_deleted,
            remote_catalog,
            &sync_policy,
            false,
            &device_id,
            &pending_remote_cleanups,
        );

        let mut targets: Vec<TargetSyncPlan> = Vec::new();
        for planned in &planned_targets {
            // #645 评论 5504296097 问题5：dry-run 用 read-only state loader,
            // build_sync_plan 内部已改用 load_sync_state_read_only.
            let plan = if !config.enabled || planned.is_deleted_target() {
                SyncPlan::new()
            } else {
                crate::sync::SyncService::build_sync_plan(
                    &planned.local_root,
                    planned.target.scope,
                )?
            };
            targets.push(TargetSyncPlan {
                target_kind: planned.target_kind.as_target_kind_str().to_string(),
                project_id: planned.project_id.clone(),
                remote_prefix: planned.target.remote_prefix.clone(),
                plan,
            });
        }

        let total_to_upload: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.plan.files_to_upload.len()).unwrap_or(u32::MAX))
            .sum();
        let total_to_download: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.plan.files_to_download.len()).unwrap_or(u32::MAX))
            .sum();
        let total_to_delete_local: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.plan.files_to_delete_local.len()).unwrap_or(u32::MAX))
            .sum();
        let total_to_delete_remote: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.plan.files_to_delete_remote.len()).unwrap_or(u32::MAX))
            .sum();
        let total_ignored: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.plan.ignored_files.len()).unwrap_or(u32::MAX))
            .sum();
        let total_conflicts: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.plan.conflicts.len()).unwrap_or(u32::MAX))
            .sum();

        Ok(FullSyncDryRunResult {
            targets,
            total_to_upload,
            total_to_download,
            total_to_delete_local,
            total_to_delete_remote,
            total_ignored,
            total_conflicts,
        })
    }

    /// #645 评论 5504296097 回退问题：锁外构建 dry-run plan。
    ///
    /// 与 [`perform_full_sync_dry_run_with_catalog`] 的区别：本函数不持任何 Core 锁，
    /// 所有磁盘读取（list_projects / pending / device / planner / scan）在锁外执行。
    /// `app_data_root` / `projects_root` 由调用方在短锁内 snapshot 后传入。
    pub(crate) fn build_full_sync_dry_run_unlocked(
        app_data_root: &std::path::Path,
        projects_root: &std::path::Path,
        config: &crate::sync::SyncConfig,
        remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    ) -> crate::error::Result<crate::sync::types::FullSyncDryRunResult> {
        use crate::sync::types::{FullSyncDryRunResult, SyncPlan, TargetSyncPlan};

        let projects = crate::project::list_projects(projects_root)?;

        let pending_deleted =
            crate::sync::pending_deleted::load_pending_deleted_targets(app_data_root)?;

        let sync_policy = crate::sync::types::SyncPolicy::from_config(config);

        let device_id = crate::settings::load_device_info(app_data_root)
            .map(|info| info.device_id)
            .unwrap_or_default();

        // #645 评论 5504296097 问题3 修复：加载 pending_remote_cleanups。
        // #645 评论 5504296097 问题3 修复：持久化错误向上传递，不再 unwrap_or_default()。
        let pending_remote_cleanups =
            crate::sync::pending_remote_cleanup::load_pending_remote_cleanups(app_data_root)?;
        let planned_targets = crate::sync::full_sync::build_full_sync_target_plan(
            app_data_root,
            projects_root,
            &projects,
            &pending_deleted,
            remote_catalog,
            &sync_policy,
            false,
            &device_id,
            &pending_remote_cleanups,
        );

        let mut targets: Vec<TargetSyncPlan> = Vec::new();
        for planned in &planned_targets {
            let plan = if !config.enabled || planned.is_deleted_target() {
                SyncPlan::new()
            } else {
                crate::sync::SyncService::build_sync_plan(
                    &planned.local_root,
                    planned.target.scope,
                )?
            };
            targets.push(TargetSyncPlan {
                target_kind: planned.target_kind.as_target_kind_str().to_string(),
                project_id: planned.project_id.clone(),
                remote_prefix: planned.target.remote_prefix.clone(),
                plan,
            });
        }

        let total_to_upload: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.plan.files_to_upload.len()).unwrap_or(u32::MAX))
            .sum();
        let total_to_download: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.plan.files_to_download.len()).unwrap_or(u32::MAX))
            .sum();
        let total_to_delete_local: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.plan.files_to_delete_local.len()).unwrap_or(u32::MAX))
            .sum();
        let total_to_delete_remote: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.plan.files_to_delete_remote.len()).unwrap_or(u32::MAX))
            .sum();
        let total_ignored: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.plan.ignored_files.len()).unwrap_or(u32::MAX))
            .sum();
        let total_conflicts: u32 = targets
            .iter()
            .map(|t| u32::try_from(t.plan.conflicts.len()).unwrap_or(u32::MAX))
            .sum();

        Ok(FullSyncDryRunResult {
            targets,
            total_to_upload,
            total_to_download,
            total_to_delete_local,
            total_to_delete_remote,
            total_ignored,
            total_conflicts,
        })
    }

    /// #645 评论 5504296097 问题5：dry-run 读真实远端 catalog 的 helper。
    ///
    /// 创建 provider + 读 catalog。任一步骤失败时返回错误
    /// （dry-run 是预览，不能返回假的远端事实）。
    fn dry_run_load_remote_catalog(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<crate::sync::types::TargetLifecycleCatalog> {
        let provider = self.create_sync_provider_for_plan(config, secrets)?;
        let snapshot = crate::sync::target_lifecycle::load_remote_catalog(provider.as_ref())?;
        Ok(snapshot.catalog)
    }

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
    /// #645 评论 5504296097 问题2：降级为 `pub(crate)` + `#[cfg(test)]`，只给内部测试用作底层 helper。
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
        // #630 评论 5308040939 Part 1：一进正式事务先原子写 Syncing + 本次 attempt
        // 时间（保留旧 last_success_time）。进程中断/被杀后重启读到的是 Syncing，
        // 而不是上一次 Success 绿灯。
        self.persist_full_sync_started();

        let secrets = self.load_sync_secrets().unwrap_or_default();
        let provider = self.create_sync_provider_for_plan(config, &secrets)?;
        let sync_policy = crate::sync::types::SyncPolicy::from_config(config);
        self.perform_full_sync_with_provider(provider.as_ref(), &sync_policy, force_sync)
    }

    /// #644 评论 5467821839 第7节：三段式全量同步 — Prepare 阶段（短写锁内调用）。
    ///
    /// 写 `Syncing` 状态、枚举 targets、算出每个 target 的 `local_root`，
    /// 产出 [`crate::sync::full_sync::FullSyncPlan`]（owned，不依赖 core）。
    ///
    /// #644 评论 5473401065 第1节：**不在**写锁内创建或 seed `StagingRun`。
    /// seed 涉及磁盘扫描/复制，会把"短写锁"变成"磁盘长锁"，阻塞冷启动卷章读取。
    /// staging 的创建和 seed 积到 `prepare_staging_runs`，在无锁状态下执行。
    ///
    /// `secrets` 由调用方传入（API 层已 snapshot override），不再内部加载。
    ///
    /// #645 评论 5504296097 问题1：`remote_catalog` 由调用方传入（在创建 provider 后
    /// 读取），planner 真正使用它做 target-level LWW 决策。`device_id` 来自真实
    /// `DeviceInfo.device_id`。
    ///
    /// transport 初始化失败时返回 Err（已持久化失败状态）。
    /// `list_projects` 失败时返回 Err（已持久化失败状态）。
    pub fn prepare_full_sync(
        &self,
        config: &crate::sync::SyncConfig,
        force_sync: bool,
        _secrets: crate::sync::SyncSecrets,
        remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
        remote_catalog_snapshot: crate::sync::types::RemoteTargetCatalogSnapshot,
    ) -> crate::error::Result<crate::sync::full_sync::FullSyncPlan> {
        use crate::sync::full_sync::FullSyncPlan;

        self.persist_full_sync_started();

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

        // #645 评论 5504296097 问题1：加载 pending deleted targets，
        // 让 prepare_full_sync 为已删除作品生成 target，run_transfer 走
        // target-delete 计划清理远端 projects/<id>/ 下所有对象。
        let pending_deleted =
            match crate::sync::pending_deleted::load_pending_deleted_targets(&self.app_data_root) {
                Ok(targets) => targets,
                Err(err) => {
                    let msg = err.to_string();
                    self.persist_full_sync_early_failure(
                        crate::sync::SyncStatus::RecoverableError(msg),
                        "global",
                    );
                    return Err(err);
                }
            };

        let sync_policy = crate::sync::types::SyncPolicy::from_config(config);

        // #645 评论 5504296097 问题1：device_id 来自真实 DeviceInfo。
        let device_id = crate::settings::load_device_info(&self.app_data_root)
            .map(|info| info.device_id)
            .unwrap_or_default();

        // #645 评论 5504296097 问题1：调用共享 planner，传入真实 remote_catalog
        // 做 target-level LWW 决策。
        // #645 评论 5504296097 问题3 修复：加载 pending_remote_cleanups。
        // #645 评论 5504296097 问题3 修复：持久化错误向上传递，不再 unwrap_or_default()。
        let pending_remote_cleanups =
            crate::sync::pending_remote_cleanup::load_pending_remote_cleanups(&self.app_data_root)?;
        let targets = crate::sync::full_sync::build_full_sync_target_plan(
            &self.app_data_root,
            &self.projects_root,
            &projects,
            &pending_deleted,
            remote_catalog,
            &sync_policy,
            force_sync,
            &device_id,
            &pending_remote_cleanups,
        );

        // #645 评论 5504296097 第2点：不再携带 workspace_git_layout。
        // 本地 Git 仓库由 bootstrap 阶段初始化，同步计划不负责 Git 生命周期。

        Ok(FullSyncPlan {
            sync_policy,
            force_sync,
            targets,
            app_data_root: self.app_data_root.clone(),
            remote_catalog_snapshot,
        })
    }

    /// #645 评论 5504296097 回退问题：锁外构建 full sync plan。
    ///
    /// 与 [`prepare_full_sync`] 的区别：本函数不调 `persist_full_sync_started`（调用方
    /// 已在短锁内完成），不持任何 Core 锁，所有磁盘读取（list_projects / pending /
    /// device / planner / scan）在锁外执行，避免阻塞正文/作品读取。
    ///
    /// `app_data_root` / `projects_root` 由调用方在短锁内 snapshot 后传入。
    pub(crate) fn build_full_sync_plan_unlocked(
        app_data_root: &std::path::Path,
        projects_root: &std::path::Path,
        config: &crate::sync::SyncConfig,
        force_sync: bool,
        remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
        remote_catalog_snapshot: crate::sync::types::RemoteTargetCatalogSnapshot,
    ) -> crate::error::Result<crate::sync::full_sync::FullSyncPlan> {
        use crate::sync::full_sync::FullSyncPlan;

        let projects = crate::project::list_projects(projects_root)?;

        let pending_deleted =
            crate::sync::pending_deleted::load_pending_deleted_targets(app_data_root)?;

        let sync_policy = crate::sync::types::SyncPolicy::from_config(config);

        let device_id = crate::settings::load_device_info(app_data_root)
            .map(|info| info.device_id)
            .unwrap_or_default();

        // #645 评论 5504296097 问题3 修复：加载 pending_remote_cleanups。
        // #645 评论 5504296097 问题3 修复：持久化错误向上传递，不再 unwrap_or_default()。
        let pending_remote_cleanups =
            crate::sync::pending_remote_cleanup::load_pending_remote_cleanups(app_data_root)?;
        let targets = crate::sync::full_sync::build_full_sync_target_plan(
            app_data_root,
            projects_root,
            &projects,
            &pending_deleted,
            remote_catalog,
            &sync_policy,
            force_sync,
            &device_id,
            &pending_remote_cleanups,
        );

        Ok(FullSyncPlan {
            sync_policy,
            force_sync,
            targets,
            app_data_root: app_data_root.to_path_buf(),
            remote_catalog_snapshot,
        })
    }

    /// #644 评论 5467821839 第7节：三段式全量同步 — 创建 provider（Prepare 阶段、写锁内）。
    ///
    /// transport 初始化失败时返回 Err（已持久化失败状态）。
    /// 根据 `config.active_provider` 选择对应的 Provider 实现。
    pub fn create_sync_provider_for_plan(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<Box<dyn crate::sync::provider::SyncProvider>> {
        // secrets 仅在 github-api feature 下使用；非 github-api 时消费以避免 unused。
        #[cfg(not(feature = "github-api"))]
        let _ = secrets;
        match config.active_provider.as_str() {
            #[cfg(feature = "github-api")]
            "github_api" => {
                let transport = self.init_sync_transport().inspect_err(|err| {
                    let status = crate::sync::full_sync::error_to_persist_status(err);
                    self.persist_full_sync_early_failure(status, "preflight");
                })?;
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
                Ok(Box::new(
                    crate::sync::provider::github::GitHubProvider::new(runtime, transport),
                ))
            }
            #[cfg(not(feature = "github-api"))]
            "github_api" => Err(crate::Error::NotImplemented),
            _ => Err(crate::Error::NotImplemented),
        }
    }

    /// #644 评论 5467821839 第7节：三段式全量同步 — Commit 阶段（短写锁内调用）。
    ///
    /// 聚合 [`crate::sync::full_sync::FullSyncTransferResult`] → `FullSyncResult`，
    /// 原子写终态 `FullSyncState`，成功类重建搜索索引。
    ///
    /// #644 评论 5472584126 第1节：staging run 的三方 commit 逻辑正式接入。
    /// #644 评论 5473105049 第3节：逐 target 判断 — 只有该 target 的 Transfer 结果
    /// 属于允许提交的终态，才计算/应用它的 commit plan；失败 target 直接丢弃 staging。
    ///
    /// #644 评论 5473401065 第4节：三方冲突不再只改 overall_status。
    /// 冲突按 target 保留完整元数据（rel_path + base/local/incoming hash），
    /// 映射成 `SyncConflict` 写入对应 target 的 `SyncResult.conflicts`，
    /// 同时持久化到该 target live root 的 `SyncState.conflicts/conflicted_files`。
    ///
    /// `staging_runs` 来自 Prepare 阶段，与 `transfer_result.targets` 按索引对应；
    /// commit 完成后显式 cleanup（`Drop` 也会兜底）。
    ///
    /// #645 评论 5504296097 Blocker 2：返回 `(FullSyncResult, committed_paths, lifecycle_receipts)`，
    /// `committed_paths` 是本次 commit 真正落盘的 workspace-relative paths，
    /// `lifecycle_receipts` 是 RemoteLifecycle 删除事务的完整 receipt，
    /// 供 API 层调 `record_workspace_change_set` + `ack_project_delete_history`。
    #[allow(clippy::excessive_nesting)]
    pub fn commit_full_sync(
        &self,
        transfer_result: crate::sync::full_sync::FullSyncTransferResult,
        staging_runs: Vec<crate::sync::staging::StagingRun>,
    ) -> (
        crate::sync::types::FullSyncResult,
        Vec<std::path::PathBuf>,
        Vec<crate::sync::types::LocalLifecycleCommitReceipt>,
    ) {
        // #645 评论 5504296097 问题1：先处理 local_lifecycle_action（DeleteProject）。
        // 对有 DeleteProject action 的 target，执行完整 Project 本地删除事务
        // （move worktree / unbind starmaps / history），不生成 PendingDeletedTarget
        // （远端已删，不反向要求删远端）。staging commit 会跳过这些 target。
        // #645 评论 5504296097 问题2修复：收集 LocalLifecycleCommitReceipt，
        // API 层负责记 history + ack（facade 没有 workspace_git_layout）。
        let mut targets = transfer_result.targets;
        let (lifecycle_committed_paths, lifecycle_receipts) =
            self.apply_local_lifecycle_deletes(&mut targets);

        // #644 评论 5473105049 第3/4节：逐 target 判断 transfer 结果，
        // 只对成功终态的 target 做 staging commit；commit IO 失败向上传播。
        let commit_outcome =
            crate::sync::commit_helpers::apply_staging_commits_for_targets(&staging_runs, &targets);

        // #644 评论 5473105049 第4节：commit 失败的 target 需要把失败信息
        // 注入到对应的 TargetSyncResult 中，让聚合逻辑产生 Recoverable/Fatal 状态。
        // #645 评论 5504296097 问题1：targets 已在上方 lifecycle 循环中声明并修改，
        // 不再重新从 transfer_result.targets 取值（否则会丢失 lifecycle 修改）。
        for (idx, commit_result) in commit_outcome.target_results.iter().enumerate() {
            if let crate::sync::commit_helpers::TargetCommitResult::Failed(msg) = commit_result {
                if let Some(target) = targets.get_mut(idx) {
                    // commit 失败视为 RecoverableError（下次同步可重试）
                    target.result.status = crate::sync::SyncStatus::RecoverableError(format!(
                        "staging_commit_failed: {}",
                        msg
                    ));
                    target.result.error = Some(format!("staging commit failed: {}", msg));
                }
            }
        }

        // #644 评论 5473401065 第4节 + #644 评论 5473551127 第3节：
        // 三方冲突按 target 映射成 SyncConflict，复用 conflict.rs 的
        // record_staging_conflicts() 统一写 conflicts.json + SyncState。
        // 持久化失败必须传播到对应 target 的错误状态，不能只打日志。
        //
        // #644 评论 5474772497 第3节：不再用 `=` 覆盖 target.result.conflicts，
        // 而是把 Transfer 阶段已有的冲突（如 GitHub LWW 发现的正文冲突）
        // 传给 record_staging_conflicts 做合并，保留两层冲突。
        for (idx, target_conflicts) in commit_outcome.target_conflicts.iter().enumerate() {
            if target_conflicts.is_empty() {
                continue;
            }
            if let Some(target) = targets.get_mut(idx) {
                let live_root = staging_runs[idx].target_live_root();
                // 保留 Transfer 阶段已有的冲突（如 GitHub LWW 发现的正文冲突）。
                let existing_conflicts = target.result.conflicts.clone();
                match crate::sync::conflict::record_staging_conflicts(
                    live_root,
                    &target.remote_prefix,
                    target_conflicts,
                    &existing_conflicts,
                ) {
                    Ok(merged_conflicts) => {
                        // #644 评论 5474772497 第3节：合并后的完整冲突列表
                        // （Transfer + staging），不再覆盖。
                        target.result.conflicts = merged_conflicts;
                        target.result.status = crate::sync::SyncStatus::Conflict;
                    }
                    Err(e) => {
                        // 持久化失败：target 进入 RecoverableError，下次同步可重试
                        target.result.status = crate::sync::SyncStatus::RecoverableError(format!(
                            "staging_conflict_persist_failed: {}",
                            e
                        ));
                        target.result.error =
                            Some(format!("failed to persist staging conflicts: {}", e));
                    }
                }
            }
        }

        let result = crate::sync::full_sync::aggregate_full_sync_result(targets);

        // #645 评论 5504296097 问题1：deleted target 远端清理成功后，
        // 从 pending_deleted_targets.json 移除该条目。
        self.cleanup_completed_deleted_targets(&result);

        let previous_state = self.load_full_sync_state().unwrap_or(None);
        let new_state = crate::sync::full_sync_state::FullSyncState::from_result_and_previous(
            &result,
            previous_state.as_ref(),
            now_epoch_seconds(),
        );
        if let Err(e) = self.save_full_sync_state(&new_state) {
            log::warn!("Failed to persist full sync state: {e}");
        }

        if matches!(
            result.overall_status,
            crate::sync::SyncStatus::Success | crate::sync::SyncStatus::LatestWinsApplied
        ) {
            if let Err(e) = self.rebuild_search_index(None) {
                log::warn!("Failed to rebuild search index after full sync: {e}");
            }
        }

        // #645 评论 5504296097 问题1：合并 lifecycle 删除产生的 committed_paths
        // 与 staging commit 产生的 committed_paths。
        let mut all_committed_paths = lifecycle_committed_paths;
        all_committed_paths.extend(commit_outcome.committed_paths);
        (result, all_committed_paths, lifecycle_receipts)
    }

    /// #645 评论 5504296097 问题1/2 修复：对有 `DeleteProject` lifecycle action 的 target
    /// 执行完整 Project 本地删除事务（move worktree / unbind starmaps / history），
    /// 不生成 PendingDeletedTarget（远端已删，不反向要求删远端）。staging commit 会
    /// 跳过这些 target。
    ///
    /// #645 评论 5504296097 问题2 修复：DeleteProject 在执行删除前先用
    /// `snapshot_local_records_read_only` 重新计算当前 local target LWW，与
    /// `expected_local_lww` guard 比较。current_local > expected → 不动 live →
    /// target 进入 RecoverableError（下次同步重试）。ReplaceProject 的 guard
    /// 在 `apply_staging_commits_for_targets` 里检查（走 replace plan）。
    ///
    /// 返回 `(lifecycle_committed_paths, lifecycle_receipts)`：
    /// - `lifecycle_committed_paths`：本次删除真正落盘的 workspace-relative paths；
    /// - `lifecycle_receipts`：`LocalLifecycleCommitReceipt`，供 API 层记 history + ack。
    #[allow(clippy::excessive_nesting)]
    fn apply_local_lifecycle_deletes(
        &self,
        targets: &mut [crate::sync::types::TargetSyncResult],
    ) -> (
        Vec<std::path::PathBuf>,
        Vec<crate::sync::types::LocalLifecycleCommitReceipt>,
    ) {
        let lifecycle_committed_paths: Vec<std::path::PathBuf> = Vec::new();
        let mut lifecycle_receipts: Vec<crate::sync::types::LocalLifecycleCommitReceipt> =
            Vec::new();
        for target in targets {
            if let crate::sync::types::LocalLifecycleCommitAction::DeleteProject {
                project_id,
                expected_local_lww,
            } = &target.local_lifecycle_action
            {
                // #645 评论 5504296097 问题2 修复：DeleteProject guard —
                // 用 snapshot_local_records_read_only 重新计算当前 local target LWW，
                // 与 expected_local_lww 严格比较。current_local == expected 才放行，
                // 其他任何情况都拒绝。
                // #645 评论 5504296097 问题2 修复：expected_local_lww 非 Option —
                // 破坏性 action 必须携带 guard。
                let expected_lww = crate::sync::full_sync::LiveTargetLww {
                    lww_time_ms: expected_local_lww.lww_time_ms,
                    device_id: expected_local_lww.device_id.clone(),
                };
                let project_root = self.projects_root.join(project_id);
                match crate::sync::staging::replace::check_replace_project_guard(
                    &project_root,
                    &expected_lww,
                ) {
                    crate::sync::staging::replace::ReplaceProjectGuardResult::Ok => {
                        // guard 通过，继续执行删除。
                    }
                    crate::sync::staging::replace::ReplaceProjectGuardResult::Err(e) => {
                        let msg = match e {
                            crate::sync::staging::replace::ReplaceProjectGuardError::LocalAdvanced {
                                expected,
                                current,
                            } => format!(
                                "DeleteProject guard failed: local diverged \
                                 (expected lww_time={} device_id={}, current lww_time={} device_id={}) \
                                 — not touching live",
                                expected.lww_time_ms,
                                expected.device_id,
                                current.lww_time_ms,
                                current.device_id
                            ),
                            crate::sync::staging::replace::ReplaceProjectGuardError::SnapshotFailed(err) => {
                                format!("DeleteProject guard snapshot failed: {err}")
                            }
                        };
                        log::warn!(
                            "[sync] commit_full_sync: DeleteProject {} guard failed: {}",
                            project_id,
                            msg
                        );
                        target.result.status =
                            crate::sync::SyncStatus::RecoverableError(msg.clone());
                        target.result.error = Some(msg);
                        continue;
                    }
                }

                // #645 评论 5504296097 问题1：RemoteLifecycle origin — 不生成
                // PendingDeletedTarget（远端已删，不反向要求删远端）。
                log::info!(
                    "[sync] commit_full_sync: DeleteProject (remote lifecycle) project_id={}",
                    project_id
                );
                let device_id = crate::settings::load_device_info(&self.app_data_root)
                    .map(|info| info.device_id)
                    .unwrap_or_default();
                match crate::project::delete_project_with_changes(
                    &self.projects_root,
                    project_id,
                    &self.app_data_root,
                    &device_id,
                    crate::project::ProjectDeleteOrigin::RemoteLifecycle,
                ) {
                    Ok(outcome) => {
                        // #645 评论 5504296097 问题4 修复：RemoteLifecycle delete 走单一
                        // durable 路线 — 不把 outcome.changes.to_flat_paths() 塞进
                        // lifecycle_committed_paths（避免与 receipt.change_set 双重记 history）。
                        // API 层用 receipt.change_set 调 record_workspace_change_set_history，
                        // 成功后调 ack_project_delete_history。
                        lifecycle_receipts.push(crate::sync::types::LocalLifecycleCommitReceipt {
                            journal_token: outcome.journal_token.clone(),
                            change_set: outcome.changes.clone(),
                            unbound_starmap_ids: outcome.unbound_starmap_ids.clone(),
                            origin: crate::project::ProjectDeleteOrigin::RemoteLifecycle,
                        });
                        // #645 评论 5504296097 问题2修复：真实删除是实际变更，
                        // 用 Success 触发 rebuild_search_index（NoChanges 不触发）。
                        target.result = crate::sync::types::SyncResult::success();
                    }
                    Err(e) => {
                        let msg = format!("remote lifecycle delete failed: {e}");
                        target.result.status =
                            crate::sync::SyncStatus::RecoverableError(msg.clone());
                        target.result.error = Some(msg);
                    }
                }
            }
        }
        (lifecycle_committed_paths, lifecycle_receipts)
    }

    /// #645 评论 5504296097 问题1/2：deleted target 远端清理/恢复成功后，
    /// 从 pending_deleted_targets.json 移除该条目。
    ///
    /// 按 typed `DeletedTargetResolution` 精确确认（不再按 `SyncStatus` 猜）：
    /// - `LocalDeleteWins` 且 `result.status` 成功类 → 移除 pending（远端删除+catalog tombstone 写入成功）；
    /// - `RemoteTargetWins` 且 `result.status` 成功类 → 移除 pending（本地恢复成功）；
    /// - `Retry` 或 `None` → 保留 pending（下次同步重试）。
    fn cleanup_completed_deleted_targets(&self, result: &crate::sync::types::FullSyncResult) {
        use crate::sync::types::DeletedTargetResolution;

        for t in &result.targets {
            if t.target_kind != "deleted_project" {
                continue;
            }
            let should_remove = match t.deleted_resolution {
                Some(DeletedTargetResolution::LocalDeleteWins)
                | Some(DeletedTargetResolution::RemoteTargetWins) => {
                    // 且 result.status 是成功类（远端删除/恢复真正落盘成功）。
                    matches!(
                        t.result.status,
                        crate::sync::SyncStatus::Success
                            | crate::sync::SyncStatus::LatestWinsApplied
                    )
                }
                // Retry 或 None（未走 deleted target 决策路径）→ 保留 pending。
                Some(DeletedTargetResolution::Retry) | None => false,
            };
            if should_remove {
                self.remove_pending_deleted_by_prefix(&t.remote_prefix);
                // #645 评论 5504296097 问题3 修复：同时移除 pending_remote_cleanup
                // （RemoteCleanupProject 成功时）。如果不存在则是幂等 no-op。
                self.remove_pending_remote_cleanup_by_prefix(&t.remote_prefix);
            }
        }
    }

    /// #645 评论 5504296097 问题3 修复：按 remote_prefix 移除 pending_remote_cleanup。
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

    /// 内部：用给定 provider 执行全量同步。
    ///
    /// `perform_full_sync` 创建 provider 后委托到此方法；测试通过此方法注入 mock provider。
    /// 语义与 `perform_full_sync` 一致：单个 target 的 `Err` 转为该 target 的
    /// `SyncResult::error(...)` 后继续，只有 `list_projects` 失败才整体 `Err`。
    ///
    /// #645 评论 5504296097 问题2：降级为 `#[cfg(test)]`，只给内部测试用。
    #[cfg(test)]
    pub(crate) fn perform_full_sync_with_provider(
        &self,
        provider: &dyn crate::sync::provider::SyncProvider,
        sync_policy: &crate::sync::types::SyncPolicy,
        force_sync: bool,
    ) -> crate::error::Result<crate::sync::types::FullSyncResult> {
        use crate::sync::types::{SyncTarget, TargetSyncResult};

        // 无法建立 target 列表才整体 Err —— 此时连 App target 都无法有序执行。
        // #630 评论 5308040939 Part 1：list_projects 失败也要先持久化提前失败状态
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

        // #630 评论 5307423953 Part B + 5308040939 Part 1：聚合后把 FullSyncState
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

    /// #630 评论 5308040939 Part 1：平台端预处理失败写同一份 Core FullSyncState 的窄接口。
    ///
    /// 只负责更新 `<app_data_root>/app-meta/sync/full_state.local.json`（与
    /// `perform_full_sync` 同一份），不新建平台第二份状态、不恢复旧双同步 API。
    /// 覆盖 Android 正文 flush / app data barrier / credentials override 等
    /// Core 根本没进入 full sync 的失败路径。
    ///
    /// - `status`：按失败类型给定（通常 `FatalError`）；
    /// - `failed_target`：传 `"preflight"`（或 `"global"`），不要伪造某个 project id。
    ///
    /// 保留旧 `last_success_time`，保证重启后顶部不会出现旧绿灯。
    pub fn record_full_sync_preflight_failure(
        &self,
        status: crate::sync::SyncStatus,
        failed_target: &str,
    ) -> crate::error::Result<()> {
        self.persist_full_sync_early_failure(status, failed_target);
        Ok(())
    }

    /// 正式事务开始：原子写 `Syncing` + 本次 attempt 时间，保留旧 last_success_time。
    /// 写失败只记录警告（同步本身继续，状态持久化是副作用）。
    /// #645 评论 5504296097 回退问题：改为 `pub(crate)` 供 API 层在短锁内调用。
    pub(crate) fn persist_full_sync_started(&self) {
        let previous = self.load_full_sync_state().unwrap_or(None);
        let state = crate::sync::full_sync_state::FullSyncState::started(
            previous.as_ref(),
            now_epoch_seconds(),
        );
        if let Err(e) = self.save_full_sync_state(&state) {
            log::warn!("Failed to persist full sync started state: {e}");
        }
    }

    /// target 开始执行前失败：原子写失败状态 + failed_target（"global"/"preflight"），
    /// 保留旧 last_success_time。写失败只记录警告。
    /// `pub(super)`：仅供 facade 内部与 `sync_ops_tests` 验证提前失败语义。
    pub(super) fn persist_full_sync_early_failure(
        &self,
        status: crate::sync::SyncStatus,
        failed_target: &str,
    ) {
        let previous = self.load_full_sync_state().unwrap_or(None);
        let state = crate::sync::full_sync_state::FullSyncState::failed_before_targets(
            previous.as_ref(),
            status,
            now_epoch_seconds(),
            failed_target,
        );
        if let Err(e) = self.save_full_sync_state(&state) {
            log::warn!("Failed to persist full sync early failure state: {e}");
        }
    }

    /// 将各 target 的结果聚合为 `FullSyncResult`：统计上传/下载/删除/冲突数，
    /// 总体状态保留错误类型，优先级按"需要用户处理的终态 > 可重试 > 成功"：
    /// `Fatal/Error > Dirty > Conflict/PartialConflict > Recoverable > Success`
    /// （Issue #630 评论 5308040939 Part 2）。
    ///
    /// `error` / `error_category` / `message_key` 从与 `overall_status` 同优先级的
    /// 第一个 dominant target 取得，避免"总体是认证失败、文案却拿到前一个网络错误"
    /// 的错位。
    ///
    /// #645 评论 5504296097 问题2：降级为 `#[cfg(test)]`，只给 `perform_full_sync_with_provider` 用。
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

        // Issue #630 评论 5308439467 Part 3：终态分两步聚合。
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
}
