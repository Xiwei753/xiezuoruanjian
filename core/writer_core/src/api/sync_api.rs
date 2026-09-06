use super::service::{ApiResult, WriterCoreApi};
use super::types::*;

/// 同步 API — 全量同步统一入口（Issue #630）。
///
/// 一个全局 `SyncConfig` + 一份全局凭据，`perform_full_sync` 内部按 `SyncTarget`
/// 把不同本地根映射到同一个远端仓库的不同前缀。
/// 旧的"作品同步 + 应用数据同步"两套用户配置 API 已删除。
impl WriterCoreApi {
    /// 旧→新同步 profile 一次性迁移（Issue #630 评论第 4 点 / D）。
    ///
    /// 详见 `crate::storage::migration`。失败时返回 `WriterError`；
    /// 冲突时返回 `NeedsReconfigure`（非 Err），由 UI 引导用户重选全局仓库。
    pub fn migrate_legacy_sync_profile(&self) -> ApiResult<LegacyMigrationOutcomeDto> {
        self.core_write()
            .migrate_legacy_sync_profile()
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 旧→新同步 profile 一次性迁移，接受精确 generation metadata（Issue #630 评论第 5 点 Part C）。
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
        // #645 评论 5504296097 问题4：sync config 是同步引擎运行状态
        // （app-meta/sync/config.local.json），不进入本地用户版本历史。
        // is_workspace_history_path 已把 SyncEngineState 排除，这里不再调
        // record_workspace_history。
        Ok(true)
    }

    /// 加载全局同步密钥（token 等）。
    /// #644 评论 5462823517 第1节：先查 API 层 override snapshot，
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
    /// #645 评论 5504296097 Blocker 1：凭据写入根本不是历史内容，
    /// 不调用 `record_workspace_history`。凭据路径由
    /// [`crate::storage::workspace_paths::is_workspace_secret_path`]
    /// 在底层统一排除，永不进入 history change set。
    pub fn save_sync_secrets(&self, secrets: SyncSecretsDto) -> ApiResult<bool> {
        self.core_write()
            .save_sync_secrets(&secrets.into())
            .map(|_| true)
            .map_err(crate::api::error::WriterError::from)
    }

    /// #592 五 / #644 评论 5462823517 第1节：设置进程级 secrets override。
    /// 直接写 API 层 Mutex，不再透传到 facade::WriterCore。
    pub fn set_sync_secrets_override(&self, secrets: SyncSecretsDto) -> ApiResult<()> {
        self.set_secrets_override(Some(secrets.into()));
        Ok(())
    }

    /// #595 十 / #644 评论 5462823517 第1节：清除进程级 secrets override。
    pub fn clear_sync_secrets_override(&self) -> ApiResult<()> {
        self.set_secrets_override(None);
        Ok(())
    }

    /// #592 五：按 generation 保存凭据到安全存储。
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

    /// #592 五：读取指定 generation 的安全存储凭据；缺失返回 None。
    pub fn load_sync_secrets_for_generation(
        &self,
        generation: u64,
    ) -> ApiResult<Option<SyncSecretsDto>> {
        self.core_read()
            .load_sync_secrets_for_generation(generation)
            .map(|opt| opt.map(Into::into))
            .map_err(Into::into)
    }

    /// #595 五：删除指定 generation 的安全存储凭据。
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
        // #645 评论 5504296097 问题4：App target 同步状态是同步引擎运行状态
        // （app-meta/sync/state.local.json），不进入本地用户版本历史。
        // is_workspace_history_path 已把 SyncEngineState 排除，这里不再调
        // record_workspace_history。
        Ok(())
    }

    /// 全量同步持久状态（Issue #630 评论 5307423953 Part B）。
    ///
    /// 读取 `<app_data_root>/app-meta/sync/full_state.local.json`。
    /// 文件不存在或 JSON 损坏时返回 None，不报错。
    pub fn load_full_sync_state(&self) -> ApiResult<Option<FullSyncStateDto>> {
        self.core_read()
            .load_full_sync_state()
            .map(|opt| opt.map(Into::into))
            .map_err(Into::into)
    }

    /// 冷启动恢复中断的 Syncing 状态（Issue #630 评论 5308439467 Part 1）。
    ///
    /// 读取 `full_state.local.json`，只有旧状态是 `Syncing` 才原子改成
    /// `RecoverableError("previous_full_sync_interrupted")`；其它终态不动。
    /// 只能在新 Core/WriterAppService 实例启动时执行一次。
    pub fn recover_interrupted_full_sync_state(&self) -> ApiResult<bool> {
        self.core_write()
            .recover_interrupted_full_sync_state()
            .map_err(Into::into)
    }

    /// #630 评论 5308040939 Part 1：平台预处理失败写同一份 Core FullSyncState 的窄接口。
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
    /// #645 评论 5504296097 问题6：dry-run 网络 IO（读远端 catalog）在 core_write()
    /// 锁外执行，拆三段短锁，避免阻塞正文/作品读取。
    /// #645 评论 5504296097 问题5：dry-run 用 core_read() + read-only state loader，
    /// 绝不写本地文件，绝不写远端（discover_legacy_remote_catalog 只读）。
    pub fn perform_full_sync_dry_run(
        &self,
        config: SyncConfigDto,
    ) -> ApiResult<FullSyncDryRunResultDto> {
        let sync_config: crate::sync::SyncConfig = config.into();
        let secrets = self.secrets_override_snapshot().unwrap_or_default();

        // #645 评论 5504296097 问题6：三段短锁 — 网络 IO 不持 core 写锁。
        // 1a. 短锁 A：创建 provider（transport 初始化可能涉及本地 IO）。
        let provider = if sync_config.enabled {
            let core = self.core_write();
            match core.create_sync_provider_for_plan(&sync_config, &secrets) {
                Ok(p) => p,
                Err(e) => {
                    // #645 评论 5504296097 问题6：provider 创建失败 → 返回错误，
                    // 不降级为空 catalog（dry-run 是预览，但不能返回假的远端事实）。
                    log::warn!("[sync] dry-run: create_sync_provider_for_plan failed: {e}");
                    return Err(crate::api::error::WriterError::from(e));
                }
            }
        } else {
            // sync disabled → 不做网络 IO，用空 catalog。
            // #645 评论 5504296097 回退问题：短锁 snapshot paths，锁外扫描。
            let (app_data_root, projects_root) = {
                let core = self.core_read();
                (core.app_data_root.clone(), core.projects_root.clone())
            };
            return crate::facade::WriterCore::build_full_sync_dry_run_unlocked(
                &app_data_root,
                &projects_root,
                &sync_config,
                &crate::sync::types::TargetLifecycleCatalog::default(),
            )
            .map(Into::into)
            .map_err(Into::into);
        };
        // 写锁已释放。
        // 1b. 无锁：读 remote catalog（网络 IO，只读一个文件）。
        // #645 评论 5504296097 问题5：用 discover_legacy_remote_catalog（只读，不写远端）。
        // dry-run 绝不在远端创建 targets.sync.json。
        let remote_catalog_snapshot =
            crate::sync::target_lifecycle::discover_legacy_remote_catalog(provider.as_ref())
                .map_err(|e| {
                    log::warn!("[sync] dry-run: discover_legacy_remote_catalog failed: {e}");
                    crate::api::error::WriterError::from(e)
                })?;

        // 1c. 短锁 B（read）：snapshot paths，锁外扫描。
        // #645 评论 5504296097 回退问题：恢复短锁+锁外扫描。
        let (app_data_root, projects_root) = {
            let core = self.core_read();
            (core.app_data_root.clone(), core.projects_root.clone())
        };
        crate::facade::WriterCore::build_full_sync_dry_run_unlocked(
            &app_data_root,
            &projects_root,
            &sync_config,
            &remote_catalog_snapshot.catalog,
        )
        .map(Into::into)
        .map_err(Into::into)
    }

    /// 全量同步 — 四段式：Prepare（短写锁）→ Seed staging（不持锁）→ Transfer（不持锁）→ Commit（短写锁）。
    ///
    /// #644 评论 5467821839 第7节：网络阶段完全不持 Core 锁，
    /// 避免全量同步期间阻塞所有读操作。
    ///
    /// #644 评论 5473401065 第1节：staging seed（磁盘扫描/复制）也移出写锁，
    /// 避免冷启动读取卷章被同步 Prepare 卡住。
    ///
    /// #645 评论 5504296097 第2点：通用 full-sync 入口不再 `#[cfg(feature = "github-api")]`
    /// 门控。具体 Provider 能否创建由 [`crate::facade::WriterCore::create_sync_provider_for_plan`]
    /// 决定（未启用 github-api feature 时 `github_api` 分支返回 `NotImplemented`）。
    #[allow(clippy::too_many_lines)]
    pub fn perform_full_sync(
        &self,
        config: SyncConfigDto,
        force_sync: bool,
    ) -> ApiResult<FullSyncResultDto> {
        let sync_config: crate::sync::SyncConfig = config.into();

        // #645 评论 5504296097 问题3 修复：sync disabled → 直接返回 no-op，
        // 不创建 provider、不读 catalog、不建 plan、不进入 run_transfer。
        // 防止 disabled 状态下仍写远端（LiveProject 会发布只有 generation.meta.json
        // 没有 正文/manifest 的空 active generation）。
        if !sync_config.enabled {
            log::debug!("[sync] perform_full_sync: sync disabled — returning no-op");
            let noop = crate::sync::types::FullSyncResult {
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
            };
            return Ok(noop.into());
        }

        // Snapshot secrets before acquiring core_write（避免持锁期间回调 override）。
        let secrets = self.secrets_override_snapshot().unwrap_or_default();

        // Phase 1: Prepare — 拆成三段短锁，网络 IO 不持 core 写锁。
        // #645 评论 5504296097 问题6：load_remote_catalog 是网络 IO，必须在
        // core_write() 作用域外执行，不阻塞正文/作品读取（#644 拆锁路线）。
        //
        // 1a. 短锁 A：创建 provider（transport 初始化可能涉及本地 IO）。
        let provider = {
            let core = self.core_write();
            core.create_sync_provider_for_plan(&sync_config, &secrets)?
        };
        // 写锁已释放。
        // 1b. 无锁：读 remote catalog（网络 IO，只读一个文件）。
        // #645 评论 5504296097 问题4：catalog 读取失败直接结束本次 full sync，
        // 返回 RecoverableError，不构造空 catalog 继续 plan（空 catalog 会让
        // planner 误判"远端无记录"做破坏性删除/复活决策）。
        // #645 评论 5504296097 问题6：用 discover_legacy_remote_catalog（真做 legacy 枚举），
        // 不再用 load_remote_catalog（只读 catalog 文件，不做 legacy 发现）。
        let remote_catalog_snapshot =
            match crate::sync::target_lifecycle::discover_legacy_remote_catalog(provider.as_ref()) {
                Ok(snapshot) => snapshot,
                Err(e) => {
                    let msg = format!("discover_legacy_remote_catalog failed: {e}");
                    log::warn!("[sync] prepare: {msg}");
                    let _ = self.record_full_sync_preflight_failure(
                        "recoverable_error".to_string(),
                        "target_catalog".to_string(),
                    );
                    return Err(crate::api::error::WriterError::from(e));
                }
            };

        // #645 评论 5504296097 问题6：catalog 文件不存在于远端（version == __nonexistent__）
        // → 正式 sync 需要把 discover 合成的 bootstrap catalog 落盘，后续 CAS 写入才有 base version。
        // dry-run 不走本路径（dry-run 用 perform_full_sync_dry_run_with_catalog，不 persist）。
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
                        let _ = self.record_full_sync_preflight_failure(
                            "recoverable_error".to_string(),
                            "target_catalog".to_string(),
                        );
                        return Err(crate::api::error::WriterError::from(e));
                    }
                }
            } else {
                remote_catalog_snapshot
            };
        // 1c. 短锁 B：只 persist Syncing + snapshot app_data_root/projects_root。
        // #645 评论 5504296097 回退问题：恢复短锁+锁外扫描。
        // 短锁只拿 app_data_root/projects_root/sync_policy/remote snapshot + persist Syncing，
        // 释放锁后 list_projects/pending/device/planner/scan 全部锁外执行，
        // 避免阻塞正文/作品读取。
        let (app_data_root, projects_root) = {
            let core = self.core_write();
            core.persist_full_sync_started();
            (core.app_data_root.clone(), core.projects_root.clone())
        };
        // 写锁已释放。锁外构建 plan（list_projects / pending / device / planner / scan）。
        let mut plan = match crate::facade::WriterCore::build_full_sync_plan_unlocked(
            &app_data_root,
            &projects_root,
            &sync_config,
            force_sync,
            &remote_catalog_snapshot.catalog,
            remote_catalog_snapshot.clone(),
        ) {
            Ok(plan) => plan,
            Err(err) => {
                let msg = err.to_string();
                log::warn!("[sync] prepare: build_full_sync_plan_unlocked failed: {msg}");
                let _ = self.record_full_sync_preflight_failure(
                    "recoverable_error".to_string(),
                    "global".to_string(),
                );
                return Err(crate::api::error::WriterError::from(err));
            }
        };

        // Phase 2: Seed staging（不持锁）— 磁盘扫描/复制，创建隔离 staging 目录。
        // #644 评论 5473401065 第2节：seed 失败直接终止本次同步，不继续拿半成品。
        // prepare_staging_runs 是纯函数，不依赖 WriterCore，无需持锁。
        //
        // #644 评论 5473551127 第1节：seed 失败时必须把 FullSyncState 从 Syncing
        // 改为失败终态，否则下次启动/同步会永久看到上一次遗留的 Syncing。
        //
        // #645 评论 5504296097 第2点：staging 不再按 active_provider 分 Git/GithubApi
        // backend 走不同 seed 路径；统一调 `seed_from_live`（文件级复制）。
        // workspace 级别的 Git layout 迁移仍由 `prepare_staging_runs` 内部完成，
        // 但不作为某个 remote provider 的 staging 模式。
        let staging_runs = match crate::sync::staging::prepare_staging_runs(&mut plan) {
            Ok(runs) => runs,
            Err(err) => {
                let status = crate::sync::full_sync::error_to_persist_status(&err);
                let status_str = match &status {
                    crate::sync::SyncStatus::FatalError(_) => "fatal_error".to_string(),
                    crate::sync::SyncStatus::RecoverableError(_) => "recoverable_error".to_string(),
                    _ => "fatal_error".to_string(),
                };
                // record_full_sync_preflight_failure 是 pub API，
                // persist_full_sync_early_failure 是 pub(super) 不可从 api 层调用。
                let _ =
                    self.record_full_sync_preflight_failure(status_str, "staging_seed".to_string());
                return Err(err.into());
            }
        };

        // Phase 3: Transfer（不持锁）— 网络 + 本地文件读写。
        let transfer_result = crate::sync::full_sync::run_transfer(provider.as_ref(), &plan);

        // Phase 4: Commit（短写锁）— 聚合结果、原子写终态、重建搜索索引、清理 staging。
        let (result, committed_paths, lifecycle_receipts) = {
            let core = self.core_write();
            core.commit_full_sync(transfer_result, staging_runs)
        };

        // #645 评论 5504296097 Blocker 2：用 commit 阶段返回的 committed_paths
        // 精确 stage，替代全量 &[] 扫描。committed_paths 是 workspace-relative paths。
        // 问题1：空 committed_paths 不触发全量扫描（record_workspace_paths_history
        // 空 paths 直接返回空结果）。
        // #645 评论 5504296097 问题4 修复：committed_paths 不再包含 RemoteLifecycle 删除
        // 的 paths（apply_local_lifecycle_deletes 已改为走 receipt.change_set 单一路径），
        // 避免同一删除记两次 history。
        self.record_workspace_paths_history(&committed_paths, "full_sync_commit");

        // #645 评论 5504296097 问题2修复：处理 RemoteLifecycle 删除事务的 receipts。
        // #645 评论 5504296097 问题4 修复：恢复单一 durable 路线 —
        // 对每个 receipt：用 change_set 调 record_workspace_change_set_history 记本地 history，
        // 成功后才调 ack_project_delete_history 推进 journal。
        // history 失败 → 不 ack → journal 保留 StarMapsUnbound → bootstrap/recover 下次补记。
        for receipt in &lifecycle_receipts {
            self.process_lifecycle_receipt(receipt);
        }

        Ok(result.into())
    }

    /// #645 评论 5504296097 问题4：处理单个 lifecycle receipt — history + ack。
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
    pub fn resolve_conflict_keep_local(&self, project_id: &str, path: &str) -> ApiResult<bool> {
        self.core_write()
            .resolve_conflict_keep_local(project_id, path)
            .map(|_| true)
            .map_err(Into::into)
    }

    /// 冲突解决：采用远端版本。
    pub fn resolve_conflict_take_remote(&self, project_id: &str, path: &str) -> ApiResult<bool> {
        self.core_write()
            .resolve_conflict_take_remote(project_id, path)
            .map(|_| true)
            .map_err(Into::into)
    }

    /// 冲突解决：标记为已合并。
    pub fn resolve_conflict_mark_merged(&self, project_id: &str, path: &str) -> ApiResult<bool> {
        self.core_write()
            .resolve_conflict_mark_merged(project_id, path)
            .map(|_| true)
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

        // 从 provider_config 读 remote_url（Issue #645 评论第 2 点）。
        let remote_url = config
            .provider_config
            .as_ref()
            .map(|pc| match pc {
                #[cfg(feature = "github-api")]
                crate::api::ProviderConfigDto::GitHub { remote_url, .. } => remote_url.clone(),
                #[cfg(not(feature = "github-api"))]
                _ => String::new(),
            })
            .unwrap_or_default();
        // 从 provider_secrets 读 token。
        let token = secrets
            .provider_secrets
            .as_ref()
            .map(|ps| match ps {
                #[cfg(feature = "github-api")]
                crate::api::ProviderSecretsDto::GitHub { token } => token.clone(),
                #[cfg(not(feature = "github-api"))]
                _ => String::new(),
            })
            .unwrap_or_default();

        if !config.enabled {
            can_run = false;
            block_reason_code = Some("DISABLED".to_string());
            block_message_key = Some("sync.block.disabled".to_string());
        } else if self.secure_storage.is_none() {
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
}
