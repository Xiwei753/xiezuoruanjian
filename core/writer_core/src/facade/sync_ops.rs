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
        let backend_type = crate::sync::resolved_backend_type(config);
        let backend = if let Some(transport) = self.sync_transport.as_ref() {
            match transport() {
                Ok(t) => crate::sync::create_sync_backend_with_transport(&backend_type, t),
                Err(e) => {
                    // transport 初始化失败：用唯一的类型化转换生成 Error，
                    // 同一个 Error 同时用于持久化状态和返回（Issue #630 评论 5308439467 Part 2）。
                    // 避免"磁盘写 FatalError 但返回 Io → Android 视为 Retryable"的错位。
                    let err = transport_init_failure_error(&e.category, &e.message);
                    let status = error_to_persist_status(&err);
                    self.persist_full_sync_early_failure(status, "preflight");
                    return Err(err);
                }
            }
        } else {
            crate::sync::create_sync_backend(&backend_type)
        };

        self.perform_full_sync_with_backend(backend.as_ref(), config, &secrets, force_sync)
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
        secrets: crate::sync::SyncSecrets,
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

        let mut targets = Vec::new();

        // App target — 只生成 plan，不创建 staging
        let app_target = crate::sync::types::SyncTarget::app();
        targets.push(PlannedTarget {
            target: app_target,
            local_root: self.app_data_root.clone(),
            staging_root: None, // prepare_staging_runs 会填充
            target_kind: "app".to_string(),
            project_id: None,
            target_live_root: self.app_data_root.clone(),
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
            });
        }

        Ok(FullSyncPlan {
            secrets,
            config: config.clone(),
            force_sync,
            targets,
            app_data_root: self.app_data_root.clone(),
        })
    }

    /// #644 评论 5467821839 第7节：三段式全量同步 — 创建 backend（Prepare 阶段、写锁内）。
    ///
    /// transport 初始化失败时返回 Err（已持久化失败状态）。
    pub fn create_sync_backend_for_plan(
        &self,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<Box<dyn crate::sync::SyncBackend>> {
        let backend_type = crate::sync::resolved_backend_type(config);
        if let Some(transport) = self.sync_transport.as_ref() {
            match transport() {
                Ok(t) => Ok(crate::sync::create_sync_backend_with_transport(
                    &backend_type,
                    t,
                )),
                Err(e) => {
                    let err = crate::sync::full_sync::transport_init_failure_error(
                        &e.category,
                        &e.message,
                    );
                    let status = crate::sync::full_sync::error_to_persist_status(&err);
                    self.persist_full_sync_early_failure(status, "preflight");
                    Err(err)
                }
            }
        } else {
            Ok(crate::sync::create_sync_backend(&backend_type))
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
    #[allow(clippy::excessive_nesting)]
    pub fn commit_full_sync(
        &self,
        transfer_result: crate::sync::full_sync::FullSyncTransferResult,
        staging_runs: Vec<crate::sync::staging::StagingRun>,
    ) -> crate::sync::types::FullSyncResult {
        // #644 评论 5473105049 第3/4节：逐 target 判断 transfer 结果，
        // 只对成功终态的 target 做 staging commit；commit IO 失败向上传播。
        let commit_outcome =
            apply_staging_commits_for_targets(&staging_runs, &transfer_result.targets);

        // #644 评论 5473105049 第4节：commit 失败的 target 需要把失败信息
        // 注入到对应的 TargetSyncResult 中，让聚合逻辑产生 Recoverable/Fatal 状态。
        let mut targets = transfer_result.targets;
        for (idx, commit_result) in commit_outcome.target_results.iter().enumerate() {
            if let TargetCommitResult::Failed(msg) = commit_result {
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

        result
    }

    /// 内部：用给定 backend 执行全量同步。
    ///
    /// `perform_full_sync` 创建 backend 后委托到此方法；测试通过此方法注入 mock backend。
    /// 语义与 `perform_full_sync` 一致：单个 target 的 `Err` 转为该 target 的
    /// `SyncResult::error(...)` 后继续，只有 `list_projects` 失败才整体 `Err`。
    pub(crate) fn perform_full_sync_with_backend(
        &self,
        backend: &dyn crate::sync::SyncBackend,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
        force_sync: bool,
    ) -> crate::error::Result<crate::sync::types::FullSyncResult> {
        use crate::sync::types::{SyncTarget, TargetSyncResult};

        // 无法建立 target 列表才整体 Err —— 此时连 App target 都无法有序执行。
        // #630 评论 5308040939 Part 1：list_projects 失败也要先持久化提前失败状态
        // （failed_target="global"）再返回 Err，不能留下上一次绿灯。
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
            backend,
            &self.app_data_root,
            config,
            secrets,
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
                backend,
                &self.project_root(&project.id),
                config,
                secrets,
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
            3 => SyncStatus::DirtyRepoBlocked,
            2 => SyncStatus::PartialConflict,
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
        let backend_type = crate::sync::resolved_backend_type(config);
        let backend = if let Some(transport) = self.sync_transport.as_ref() {
            match transport() {
                Ok(t) => crate::sync::create_sync_backend_with_transport(&backend_type, t),
                Err(e) => {
                    // 同 perform_full_sync：用类型化 Error，避免 Io → Retryable 错位
                    // （Issue #630 评论 5308439467 Part 2）。
                    return Err(transport_init_failure_error(&e.category, &e.message));
                }
            }
        } else {
            crate::sync::create_sync_backend(&backend_type)
        };
        backend.diagnose(config, secrets)
    }
}

/// #644 评论 5473105049 第3/4节：逐 target 判断 transfer 结果，只对成功终态的 target
/// 做 staging commit；失败 target 直接丢弃 staging，绝不能写 live。
///
/// #644 评论 5474166587 问题2：引入 `TargetCommitMode`，Conflict/PartialConflict
/// 不再整体丢弃 staging，而是 `ConflictMetadataOnly`——把冲突元数据
/// （state.local.json 的 conflicted_files/conflicts、conflicts.json、冲突副本）
/// 落到 live，PartialConflict 中已安全完成的非冲突文件也继续提交。
///
/// #644 评论 5473105049 第4节：commit IO 失败通过 `TargetCommitResult` 向上传播，
/// 不能只 `log::warn!` 吞掉。
///
/// #644 评论 5473401065 第4节：冲突按 target 保留 `StagingConflict`（含三方哈希），
/// 不再只做全局路径列表。
///
/// `staging_runs` 与 `transfer_targets` 按索引对应。
/// 返回每个 target 的 commit 结果 + 每个 target 的冲突列表。
#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
fn apply_staging_commits_for_targets(
    staging_runs: &[crate::sync::staging::StagingRun],
    transfer_targets: &[crate::sync::types::TargetSyncResult],
) -> StagingCommitOutcome {
    let mut target_conflicts: Vec<Vec<crate::sync::staging::StagingConflict>> = Vec::new();
    let mut target_results: Vec<TargetCommitResult> = Vec::new();

    for (idx, run) in staging_runs.iter().enumerate() {
        // 检查对应 target 的 transfer 结果，决定 commit 模式。
        let mode = if let Some(target) = transfer_targets.get(idx) {
            target_commit_mode(&target.result.status)
        } else {
            TargetCommitMode::Skip
        };

        match mode {
            TargetCommitMode::Skip => {
                log::warn!(
                    "Staging commit: skipping target {} (run_id={}) — transfer not in committable state",
                    idx,
                    run.run_id()
                );
                target_results.push(TargetCommitResult::Skipped);
                target_conflicts.push(Vec::new());
                run.cleanup();
                continue;
            }
            TargetCommitMode::Full => {
                let live_root = run.target_live_root();
                let plan = match run.compute_commit_plan(live_root) {
                    Ok(plan) => plan,
                    Err(e) => {
                        let msg = format!("compute_commit_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };
                // #644 评论 5475110422 第3节：Git backend 时启用 backup_mode，
                // 使 SaveTransaction commit 后能在 Git finalize 失败时 rollback。
                let needs_git_finalize = run.git_seed_state().is_some();

                // #644 评论 5476546134 第1节：prepare_git_finalize 必须在
                // apply_commit_plan_to_live 之前。snapshot 准备失败时，
                // live 一字节都不能改。
                let git_snapshot = if needs_git_finalize {
                    let seed_state = match run.git_seed_state() {
                        Some(s) => s,
                        None => {
                            let msg = "git_seed_state missing despite needs_git_finalize=true";
                            log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                            target_results.push(TargetCommitResult::Failed(msg.to_string()));
                            target_conflicts.push(Vec::new());
                            run.cleanup();
                            continue;
                        }
                    };
                    match crate::sync::git_commit::prepare_git_finalize(
                        live_root,
                        seed_state,
                        &run.staging_root(),
                    ) {
                        Ok(snap) => Some(snap),
                        Err(e) => {
                            let msg = format!("prepare_git_finalize failed: {}", e);
                            log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                            target_results.push(TargetCommitResult::Failed(msg));
                            target_conflicts.push(Vec::new());
                            run.cleanup();
                            continue;
                        }
                    }
                } else {
                    None
                };

                let mut tx = match apply_commit_plan_to_live(
                    live_root,
                    &plan.content_actions,
                    &plan.engine_state_actions,
                    needs_git_finalize,
                ) {
                    Ok(tx) => tx,
                    Err(e) => {
                        let msg = format!("apply_commit_plan_to_live failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };

                // #644 评论 5476546134 第2节：设置 git_finalize recovery record，
                // 写入 manifest 供崩溃恢复使用。
                if let (Some(snap), Some(seed_state)) = (&git_snapshot, run.git_seed_state()) {
                    tx.set_git_finalize_recovery(
                        crate::sync::git_commit::GitFinalizeRecoveryRecord {
                            seed_state:
                                crate::sync::git_commit::SerializableGitSeedState::from_seed_state(
                                    seed_state,
                                ),
                            metadata_snapshot: snap.clone(),
                        },
                    );
                }

                // #644 评论 5475805198 第2节：Git finalize 使用 git_commit 模块。
                // 失败时自动 rollback Git metadata + SaveTransaction rollback。
                if let Err(e) = crate::sync::git_commit::try_commit_git_finalize(
                    live_root,
                    &run.staging_root(),
                    run.git_seed_state(),
                    git_snapshot.as_ref(),
                ) {
                    // Git finalize 失败 → rollback 文件。
                    if let Err(rb_err) = tx.rollback() {
                        log::warn!(
                            "Staging commit: git finalize failed AND rollback failed: {} / {}",
                            e,
                            rb_err
                        );
                    }
                    let msg = format!("git repo-metadata finalize failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    target_results.push(TargetCommitResult::Failed(msg));
                    target_conflicts.push(Vec::new());
                    run.cleanup();
                    continue;
                }
                // Git finalize 成功，清理事务目录。
                tx.finish();
                target_conflicts.push(plan.conflict);
                target_results.push(TargetCommitResult::Ok);
                run.cleanup();
            }
            TargetCommitMode::ConflictMetadataOnly => {
                let live_root = run.target_live_root();
                let plan = match run.compute_commit_plan(live_root) {
                    Ok(plan) => plan,
                    Err(e) => {
                        let msg = format!("compute_commit_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };

                // #644 评论 5474166587 问题2：ConflictMetadataOnly 必须把冲突元数据
                // 落到 live。engine_state_actions 包含 state.local.json、conflicts.json
                // （若 staging 里写了），必须写回。
                // PartialConflict 中已安全完成的非冲突文件可继续提交，但
                // target.result.conflicts 里的路径必须从 content commit 排除。
                let transfer_conflict_paths: std::collections::HashSet<String> = transfer_targets
                    .get(idx)
                    .map(|t| {
                        t.result
                            .conflicts
                            .iter()
                            .map(|c| c.local_path.clone())
                            .collect()
                    })
                    .unwrap_or_default();

                let safe_content_actions: Vec<_> = plan
                    .content_actions
                    .iter()
                    .filter(|action| {
                        let rel = match action {
                            crate::sync::staging::CommitAction::Apply { rel_path, .. } => {
                                rel_path.to_string_lossy().to_string()
                            }
                            crate::sync::staging::CommitAction::Delete { rel_path } => {
                                rel_path.to_string_lossy().to_string()
                            }
                        };
                        !transfer_conflict_paths.contains(&rel)
                    })
                    .cloned()
                    .collect();

                if let Err(e) = apply_commit_plan_to_live(
                    live_root,
                    &safe_content_actions,
                    &plan.engine_state_actions,
                    false,
                )
                .map(|_tx| ())
                {
                    let msg = format!("apply_commit_plan_to_live failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    target_results.push(TargetCommitResult::Failed(msg));
                    target_conflicts.push(Vec::new());
                    run.cleanup();
                    continue;
                }
                // 两种冲突（Transfer 判定的 + 外层 StagingConflict）汇入同一份
                // live conflict state。外层 StagingConflict 通过 plan.conflict 返回，
                // 由 commit_full_sync 的 record_staging_conflicts 写入；
                // Transfer 判定的冲突已在 target.result.conflicts 里，由聚合逻辑保留。
                target_conflicts.push(plan.conflict);
                target_results.push(TargetCommitResult::Ok);
                run.cleanup();
            }
        }
    }

    StagingCommitOutcome {
        target_results,
        target_conflicts,
    }
}

/// 单个 target 的 staging commit 结果。
enum TargetCommitResult {
    /// commit 成功。
    Ok,
    /// transfer 失败，跳过 commit。
    Skipped,
    /// commit 过程中 IO 失败。
    Failed(String),
}

/// staging commit 阶段的汇总结果。
struct StagingCommitOutcome {
    target_results: Vec<TargetCommitResult>,
    /// #644 评论 5473401065 第4节：冲突按 target 保留，不再只做全局路径列表。
    target_conflicts: Vec<Vec<crate::sync::staging::StagingConflict>>,
}

/// #644 评论 5474166587 问题2：单个 target 的 staging commit 模式。
///
/// - `Full`：成功类终态，content + engine_state 全部写回 live。
/// - `ConflictMetadataOnly`：Conflict/PartialConflict，冲突元数据 + 已安全完成的
///   非冲突文件写回 live，冲突路径本身不写回。
/// - `Skip`：Fatal/Recoverable/Dirty/Error，整体丢弃 staging。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetCommitMode {
    /// 成功类终态：content + engine_state 全部写回 live。
    Full,
    /// Conflict/PartialConflict：冲突元数据 + 已安全完成的非冲突文件写回 live。
    ConflictMetadataOnly,
    /// Fatal/Recoverable/Dirty/Error：整体丢弃 staging。
    Skip,
}

/// #644 评论 5474166587 问题2：根据 transfer 结果状态决定 staging commit 模式。
///
/// 规则：
/// - 成功类终态（Success、NoChanges、LatestWinsApplied、BranchMissingRecovered）→ `Full`
/// - Conflict/PartialConflict → `ConflictMetadataOnly`
/// - Fatal/Error/Recoverable/Dirty → `Skip`
fn target_commit_mode(status: &crate::sync::SyncStatus) -> TargetCommitMode {
    use crate::sync::SyncStatus;
    match status {
        SyncStatus::Success
        | SyncStatus::NoChanges
        | SyncStatus::LatestWinsApplied
        | SyncStatus::BranchMissingRecovered => TargetCommitMode::Full,
        SyncStatus::Conflict | SyncStatus::PartialConflict => {
            TargetCommitMode::ConflictMetadataOnly
        }
        // 其余（FatalError/Error/RecoverableError/DirtyRepoBlocked/Syncing/Idle/ConfiguredNotTested）
        // 全部 Skip。
        _ => TargetCommitMode::Skip,
    }
}

/// #644 评论 5473105049 第4节：把 commit plan 中的 Apply/Delete 变更通过 SaveTransaction
/// 写回 live root。返回 `Result<()>`，任何 IO 失败都向上传播。
///
/// #644 评论 5474166587 问题1：content_actions + engine_state_actions 用同一个
/// `SaveTransaction` 一次写回，不另起第二套保存路径。
///
/// #644 评论 5475110422 第3节：`backup_mode` 为 true 时，SaveTransaction 在 commit 前
/// 备份被覆盖/删除的旧文件，使调用方能在 Git finalize 失败时 rollback。
fn apply_commit_plan_to_live(
    live_root: &std::path::Path,
    content_actions: &[crate::sync::staging::CommitAction],
    engine_state_actions: &[crate::sync::staging::CommitAction],
    backup_mode: bool,
) -> crate::error::Result<crate::storage::transaction::SaveTransaction> {
    if content_actions.is_empty() && engine_state_actions.is_empty() {
        return Ok(crate::storage::transaction::SaveTransaction::new(live_root));
    }
    let mut tx = crate::storage::transaction::SaveTransaction::new(live_root);
    if backup_mode {
        tx.enable_backup_mode();
    }
    // engine_state 先入事务（state/manifest 先就绪，再写用户内容）。
    for action in engine_state_actions {
        match action {
            crate::sync::staging::CommitAction::Apply { rel_path, content } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_bytes(&rel_str, content)?;
            }
            crate::sync::staging::CommitAction::Delete { rel_path } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_delete(&rel_str);
            }
        }
    }
    for action in content_actions {
        match action {
            crate::sync::staging::CommitAction::Apply { rel_path, content } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_bytes(&rel_str, content)?;
            }
            crate::sync::staging::CommitAction::Delete { rel_path } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_delete(&rel_str);
            }
        }
    }
    tx.commit()?;
    Ok(tx)
}

/// 当前 Unix 秒 — 全量同步持久状态统一时间源。
/// 系统时钟异常时回退 0（极端情况下 attempt 时间戳为 0，不阻断同步）。
fn now_epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// transport 初始化失败的类型化 Error 转换（Issue #630 评论 5308439467 Part 2）。
///
/// 唯一一份转换，同时用于持久化 FullSyncState 状态和返回给调用方，避免
/// "磁盘写 FatalError 但返回 Io → Android 视为 Retryable"的错位。
///
/// - token/auth/permission/repo-permission 类 → `Error::SyncAuthFailed`（不可恢复）
/// - network/dns/tls/临时 IO 类 → `Error::SyncNetworkUnavailable`（可恢复）
/// - rate limit → `Error::SyncRateLimited`（可恢复）
/// - 其它未知项 → `Error::SyncAuthFailed`（保守起视为不可恢复，不落 Io 后自动变可重试）
fn transport_init_failure_error(category: &str, message: &str) -> crate::Error {
    use crate::sync::types::SyncErrorCategory;
    let reason = format!("Transport init failed: {} - {}", category, message);
    match SyncErrorCategory::from_code(category, "") {
        SyncErrorCategory::TokenMissing
        | SyncErrorCategory::TokenInvalid
        | SyncErrorCategory::TokenPermissionDenied
        | SyncErrorCategory::AuthError
        | SyncErrorCategory::GithubUnauthorized
        | SyncErrorCategory::GithubForbidden
        | SyncErrorCategory::RepoNotFoundOrNoPermission => crate::Error::SyncAuthFailed { reason },
        SyncErrorCategory::GithubNetworkFailed
        | SyncErrorCategory::DnsFailed
        | SyncErrorCategory::TlsFailed
        | SyncErrorCategory::NetworkProbeFailed => crate::Error::SyncNetworkUnavailable { reason },
        SyncErrorCategory::ApiRateLimited => crate::Error::SyncRateLimited {
            retry_after_secs: 0,
        },
        // 其它未知项保守起视为不可恢复，不落 Io 后自动变可重试
        _ => crate::Error::SyncAuthFailed { reason },
    }
}

/// 把 Error 转为持久化用的 SyncStatus：recoverable → RecoverableError，否则 FatalError。
fn error_to_persist_status(err: &crate::Error) -> crate::sync::SyncStatus {
    let msg = err.to_string();
    if err.recoverable() {
        crate::sync::SyncStatus::RecoverableError(msg)
    } else {
        crate::sync::SyncStatus::FatalError(msg)
    }
}

/// 判断 target 状态是否为协议错误（不应出现在 target 结果里的非终态/未测试状态）。
fn is_protocol_error_status(status: &crate::sync::SyncStatus) -> bool {
    matches!(
        status,
        crate::sync::SyncStatus::Syncing
            | crate::sync::SyncStatus::Idle
            | crate::sync::SyncStatus::ConfiguredNotTested
    )
}

/// 协议错误聚合字段：(overall_status, error, error_category, message_key)。
type ProtocolErrorFields = (
    crate::sync::SyncStatus,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// 协议错误聚合字段构造（Issue #630 评论 5308439467 Part 3）。
///
/// 任何 target 返回 Syncing/Idle/ConfiguredNotTested 时，返回
/// (FatalError("invalid_target_status_for_aggregation"), error, error_category, message_key)，
/// 从第一个协议错误 target 取 error/error_category/message_key。无协议错误时返回 None。
fn build_protocol_error_fields(
    targets: &[crate::sync::types::TargetSyncResult],
) -> Option<ProtocolErrorFields> {
    if !targets
        .iter()
        .any(|t| is_protocol_error_status(&t.result.status))
    {
        return None;
    }
    let overall_status =
        crate::sync::SyncStatus::FatalError("invalid_target_status_for_aggregation".to_string());
    let dominant = targets
        .iter()
        .find(|t| is_protocol_error_status(&t.result.status));
    let error = dominant.and_then(|t| t.result.error.clone());
    let error_category = dominant.and_then(|t| t.result.error_category.clone());
    let message_key = dominant
        .and_then(|t| t.result.message_key.clone())
        .or_else(|| {
            error_category
                .as_deref()
                .map(sync_error_category_to_message_key_string)
        });
    Some((overall_status, error, error_category, message_key))
}

/// SyncErrorCategory code → message_key 字符串（供 protocol-error 聚合复用）。
fn sync_error_category_to_message_key_string(code: &str) -> String {
    crate::sync::types::SyncErrorCategory::from_code(code, "")
        .to_message_key()
        .to_string()
}

/// 聚合成功类终态（Issue #630 评论 5311102143）。
///
/// 失败优先级为 0 时调用。用语义判断而非数字优先级：
/// - `BranchMissingRecovered` 存在 → `BranchMissingRecovered`（最高）
/// - `LatestWinsApplied` 存在 → `LatestWinsApplied`
/// - 全部 `NoChanges` → `NoChanges`
/// - 其余情况 → `Success`
///
/// 关键语义：`Success + NoChanges → Success`（有 target 实际上传/下载了）。
/// 协议错误状态不应到达此处（已由 `build_protocol_error_fields` 拦截）。
fn aggregate_success_status(
    targets: &[crate::sync::types::TargetSyncResult],
) -> crate::sync::SyncStatus {
    use crate::sync::SyncStatus;

    if targets
        .iter()
        .any(|t| matches!(t.result.status, SyncStatus::BranchMissingRecovered))
    {
        return SyncStatus::BranchMissingRecovered;
    }
    if targets
        .iter()
        .any(|t| matches!(t.result.status, SyncStatus::LatestWinsApplied))
    {
        return SyncStatus::LatestWinsApplied;
    }
    if !targets.is_empty()
        && targets
            .iter()
            .all(|t| matches!(t.result.status, SyncStatus::NoChanges))
    {
        return SyncStatus::NoChanges;
    }
    SyncStatus::Success
}

/// 单个 target 状态在聚合中的优先级（数字越大越需要用户处理）：
/// 4=Fatal/Error，3=Dirty，2=Conflict/PartialConflict，1=Recoverable，0=其余（成功类）。
fn full_sync_status_priority(status: &crate::sync::SyncStatus) -> u8 {
    match status {
        crate::sync::SyncStatus::FatalError(_) | crate::sync::SyncStatus::Error(_) => 4,
        crate::sync::SyncStatus::DirtyRepoBlocked => 3,
        crate::sync::SyncStatus::Conflict | crate::sync::SyncStatus::PartialConflict => 2,
        crate::sync::SyncStatus::RecoverableError(_) => 1,
        _ => 0,
    }
}

/// 执行单个 target 的同步，把 `Err` 转为该 target 的 `SyncResult::error(...)`。
///
/// `perform_full_sync` 中 App target 和每个 Project target 都通过此 helper 调用，
/// 避免单 target 的 `Err` 用 `?` 提前打断整个全量同步。`Err` 的 `recoverable()`
/// 决定 `SyncStatus::RecoverableError` / `FatalError`，`sync_category()` 决定
/// `error_category`（空字符串视为无分类）。
pub(super) fn run_full_sync_target(
    backend: &dyn crate::sync::SyncBackend,
    local_root: &std::path::Path,
    config: &crate::sync::SyncConfig,
    secrets: &crate::sync::SyncSecrets,
    target: &crate::sync::types::SyncTarget,
    force_sync: bool,
) -> crate::sync::types::SyncResult {
    match backend.sync(local_root, config, secrets, target, force_sync) {
        Ok(result) => result,
        Err(err) => {
            let msg = err.to_string();
            let category = err.sync_category();
            let error_category = if category.is_empty() {
                None
            } else {
                Some(category.to_string())
            };
            let status = if err.recoverable() {
                crate::sync::SyncStatus::RecoverableError(msg.clone())
            } else {
                crate::sync::SyncStatus::FatalError(msg.clone())
            };
            crate::sync::types::SyncResult::error(
                status,
                crate::sync::types::FirstSyncMode::NotAttempted,
                msg,
                error_category,
            )
        }
    }
}
