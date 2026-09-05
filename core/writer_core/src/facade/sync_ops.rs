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

    /// 全量同步 dry-run — 枚举 App target + 所有 Project target，构建每个 target 的计划。
    ///
    /// `secrets` 由调用方传入（API 层已 snapshot override），不再内部加载。
    pub fn perform_full_sync_dry_run(
        &self,
        config: &crate::sync::SyncConfig,
        _secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<crate::sync::types::FullSyncDryRunResult> {
        use crate::sync::types::{FullSyncDryRunResult, SyncTarget, TargetSyncPlan};

        let mut targets: Vec<TargetSyncPlan> = Vec::new();

        // App target
        let app_target = SyncTarget::app();
        let app_plan = if !config.enabled {
            crate::sync::SyncPlan::new()
        } else {
            crate::sync::SyncService::build_sync_plan(&self.app_data_root, app_target.scope)?
        };
        targets.push(TargetSyncPlan {
            target_kind: "app".to_string(),
            project_id: None,
            remote_prefix: app_target.remote_prefix.clone(),
            plan: app_plan,
        });

        // Project targets
        let projects = self.list_projects()?;
        for project in &projects {
            let target = SyncTarget::project(&project.id);
            let plan = if !config.enabled {
                crate::sync::SyncPlan::new()
            } else {
                crate::sync::SyncService::build_sync_plan(
                    &self.project_root(&project.id),
                    target.scope,
                )?
            };
            targets.push(TargetSyncPlan {
                target_kind: "project".to_string(),
                project_id: Some(project.id.clone()),
                remote_prefix: target.remote_prefix.clone(),
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

    /// 全量同步 — 先建立 App target，再枚举所有作品建立 Project target；
    /// 共享同一份 config / secrets snapshot，按 target 顺序执行。
    /// 一个 target 的状态/manifest 仍写在它自己的本地 root 下。
    ///
    /// 单个 target 的 `Err`（本地 root IO 错、transport 调用失败等）不提前打断
    /// 整个全量同步：该 target 的 Err 被转为 `SyncResult::error(...)` 后 push 到
    /// `targets`，继续下一 target。只有无法建立 target 列表（`list_projects`
    /// 失败）或全局配置无法解析/transport 初始化失败这类无法开始事务的错误才让
    /// 整个 `perform_full_sync` 返回 `Err`。
    pub fn perform_full_sync(
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
    /// staging 的创建和 seed 移到 `prepare_staging_runs`，在无锁状态下执行。
    ///
    /// `secrets` 由调用方传入（API 层已 snapshot override），不再内部加载。
    ///
    /// transport 初始化失败时返回 Err（已持久化失败状态）。
    /// `list_projects` 失败时返回 Err（已持久化失败状态）。
    pub fn prepare_full_sync(
        &self,
        config: &crate::sync::SyncConfig,
        force_sync: bool,
        _secrets: crate::sync::SyncSecrets,
    ) -> crate::error::Result<crate::sync::full_sync::FullSyncPlan> {
        use crate::sync::full_sync::{FullSyncPlan, PlannedTarget};

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

        let mut targets = Vec::new();

        // App target — 只生成 plan，不创建 staging。
        let app_target = crate::sync::types::SyncTarget::app();
        targets.push(PlannedTarget {
            target: app_target,
            local_root: self.app_data_root.clone(),
            staging_root: None, // prepare_staging_runs 会填充
            target_kind: "app".to_string(),
            project_id: None,
            target_live_root: self.app_data_root.clone(),
            deleted_journal_token: None,
        });

        // Project targets — 只生成 plan，不创建 staging
        for project in &projects {
            let target = crate::sync::types::SyncTarget::project(&project.id);
            let project_local_root = self.project_root(&project.id);
            targets.push(PlannedTarget {
                target,
                local_root: project_local_root.clone(),
                staging_root: None, // prepare_staging_runs 会填充
                target_kind: "project".to_string(),
                project_id: Some(project.id.clone()),
                target_live_root: project_local_root,
                deleted_journal_token: None,
            });
        }

        // #645 评论 5504296097 问题1：Pending deleted targets —
        // 已删除作品的远端前缀需要清理。target_kind="deleted_project"，
        // run_transfer 走 target-delete 计划。local_root 指向 app_data_root
        // （deleted target 不读本地目录，只枚举远端），staging_root=None。
        for pending in &pending_deleted {
            targets.push(PlannedTarget {
                target: pending.target.clone(),
                local_root: self.app_data_root.clone(),
                staging_root: None,
                target_kind: "deleted_project".to_string(),
                project_id: Some(
                    pending
                        .target
                        .remote_prefix
                        .strip_prefix("projects/")
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                ),
                target_live_root: self.app_data_root.clone(),
                deleted_journal_token: Some(pending.journal_token.clone()),
            });
        }

        // #645 评论 5504296097 第2点：不再携带 workspace_git_layout。
        // 本地 Git 仓库由 bootstrap 阶段初始化，同步计划不负责 Git 生命周期。

        Ok(FullSyncPlan {
            sync_policy: crate::sync::types::SyncPolicy::from_config(config),
            force_sync,
            targets,
            app_data_root: self.app_data_root.clone(),
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
    /// #645 评论 5504296097 Blocker 2：返回 `(FullSyncResult, committed_paths)`，
    /// `committed_paths` 是本次 commit 真正落盘的 workspace-relative paths，
    /// 供 API 层调 `record_workspace_history` 精确 stage，替代全量 `&[]` 扫描。
    #[allow(clippy::excessive_nesting)]
    pub fn commit_full_sync(
        &self,
        transfer_result: crate::sync::full_sync::FullSyncTransferResult,
        staging_runs: Vec<crate::sync::staging::StagingRun>,
    ) -> (crate::sync::types::FullSyncResult, Vec<std::path::PathBuf>) {
        // #644 评论 5473105049 第3/4节：逐 target 判断 transfer 结果，
        // 只对成功终态的 target 做 staging commit；commit IO 失败向上传播。
        let commit_outcome = crate::sync::commit_helpers::apply_staging_commits_for_targets(
            &staging_runs,
            &transfer_result.targets,
        );

        // #644 评论 5473105049 第4节：commit 失败的 target 需要把失败信息
        // 注入到对应的 TargetSyncResult 中，让聚合逻辑产生 Recoverable/Fatal 状态。
        let mut targets = transfer_result.targets;
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

        (result, commit_outcome.committed_paths)
    }

    /// #645 评论 5504296097 问题1：deleted target 远端清理成功后，
    /// 从 pending_deleted_targets.json 移除该条目。
    /// 用 remote_prefix 匹配（target_kind=="deleted_project" 且 transfer 成功类终态）。
    fn cleanup_completed_deleted_targets(&self, result: &crate::sync::types::FullSyncResult) {
        for t in &result.targets {
            if t.target_kind != "deleted_project" {
                continue;
            }
            if !matches!(
                t.result.status,
                crate::sync::SyncStatus::Success
                    | crate::sync::SyncStatus::NoChanges
                    | crate::sync::SyncStatus::LatestWinsApplied
            ) {
                continue;
            }
            self.remove_pending_deleted_by_prefix(&t.remote_prefix);
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
    /// `pub(super)`：仅供 facade 内部与 `sync_ops_tests` 验证中断语义。
    pub(super) fn persist_full_sync_started(&self) {
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
