use super::service::{ApiResult, WriterCoreApi};
use super::types::*;
use crate::sync::cancellation_token::{SyncCancellationToken, SyncProgressSink};

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
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    pub fn perform_full_sync(
        &self,
        config: SyncConfigDto,
        force_sync: bool,
        cancellation_token: Option<SyncCancellationToken>,
        progress_sink: Option<SyncProgressSink>,
        target_progress: Option<&crate::sync::full_sync::SyncProgressCallback>,
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
        let provider = {
            let core = self.core_write();
            core.create_sync_provider_for_plan(&sync_config, &secrets)?
        };
        // 写锁已释放。
        // 1b. 无锁：读 remote catalog（网络 IO，只读一个文件）。
        //   catalog 读取失败直接结束本次 full sync，
        // 返回 RecoverableError，不构造空 catalog 继续 plan（空 catalog 会让
        // planner 误判"远端无记录"做破坏性删除/复活决策）。
        //   用 discover_legacy_remote_catalog（真做 legacy 枚举），
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
        self.perform_full_sync_with_provider(
            provider.as_ref(),
            &plan,
            staging_runs,
            cancellation_token,
            progress_sink,
            target_progress,
        )
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
            let Some((target_sync_result, _resolution, _action)) =
                crate::sync::full_sync::run_single_target_transfer(
                    provider,
                    plan,
                    target_index,
                    &mut catalog_snapshot,
                    cancellation_token.as_ref(),
                )
            else {
                // target 未执行（取消或索引越界），跳过。
                continue;
            };

            // 3. sink.set_target_phase("commit") — 标记该 target 进入 Commit 阶段。
            if let Some(ref sink) = progress_sink {
                sink.set_target_phase(
                    &planned.target.remote_prefix,
                    planned.project_id.as_deref(),
                    "commit",
                );
            }

            // 4. Commit（短写锁）— 该 target 的 state/conflicts 成为 live 终态。
            let (target_with_conflicts, target_committed_paths) =
                match staging_runs.get(target_index) {
                    Some(run) => self.commit_target_after_transfer(run, &target_sync_result),
                    None => {
                        // 没有 staging run（不应发生），直接使用 transfer result。
                        (target_sync_result.clone(), Vec::new())
                    }
                };

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

            // 3. cleanup staging run — 之后整轮收口不再碰这个 target 的 staging。
            if let Some(run) = staging_runs.get(target_index) {
                run.cleanup();
            }

            // 6. target_progress callback — 此时该 target 的 state/conflicts 已经成为
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

            all_targets.push(target_with_conflicts);
            all_committed_paths.extend(target_committed_paths);
        }

        // generation GC — 清理未引用 generation（整轮结束后统一执行）。
        // Issue #763 评论 5831610228：generation GC 循环里每处理一个 planned target
        // 时用 set_target_phase 更新 sink，避免残留最后一个 Transfer target 指错作品。
        let generation_gc_result = Self::run_full_sync_generation_gc(
            provider,
            plan,
            &catalog_snapshot,
            cancellation_token.as_ref(),
            progress_sink.as_ref(),
        );

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
        let (result, committed_paths, lifecycle_receipts) = {
            let core = self.core_write();
            core.finalize_full_sync(all_targets, generation_gc_result)
        };

        // 合并 per-target committed_paths 与 finalize 阶段返回的 committed_paths。
        let mut all_paths = all_committed_paths;
        all_paths.extend(committed_paths);

        // 用 commit 阶段返回的 committed_paths
        // 精确 stage，替代全量 &[] 扫描。committed_paths 是 workspace-relative paths。
        // 空 committed_paths 不触发全量扫描（record_workspace_paths_history
        // 空 paths 直接返回空结果）。
        self.record_workspace_paths_history(&all_paths, "full_sync_commit");

        // 处理 RemoteLifecycle 删除事务的 receipts。
        // 恢复单一 durable 路线 —
        // 对每个 receipt：用 change_set 调 record_workspace_change_set_history 记本地 history，
        // 成功后才调 ack_project_delete_history 推进 journal。
        // history 失败 → 不 ack → journal 保留 StarMapsUnbound → bootstrap/recover 下次补记。
        for receipt in &lifecycle_receipts {
            self.process_lifecycle_receipt(receipt);
        }

        Ok(result.into())
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
        match crate::sync::conflict::record_staging_conflicts(
            run.target_live_root(),
            &target_sync_result.remote_prefix,
            conflicts,
            &existing_conflicts,
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

    fn commit_target_after_transfer(
        &self,
        run: &crate::sync::staging::StagingRun,
        target_sync_result: &crate::sync::types::TargetSyncResult,
    ) -> (
        crate::sync::types::TargetSyncResult,
        Vec<std::path::PathBuf>,
    ) {
        // Issue #762 评论 5834136935：跨 WriterCoreApi 实例串行化同一 sync root 的
        // state/conflict mutation。整段 Commit（三方 merge + SaveTransaction 写回 +
        // record_staging_conflicts）持有该 project root 的共享锁，防止 UI resolve
        // 在 Commit 过程中写入后被旧 merged state 覆盖。
        crate::sync::state_lock::with_sync_state_lock(run.target_live_root(), || {
            let _core = self.core_write();
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
                target_with_conflicts.result.status = crate::sync::SyncStatus::RecoverableError(
                    format!("staging_commit_failed: {}", msg),
                );
                target_with_conflicts.result.error =
                    Some(format!("staging commit failed: {}", msg));
            }

            (target_with_conflicts, committed_paths)
        })
    }

    /// 整轮结束后的 generation GC — 清理未引用 generation。
    ///
    /// 成功返回 `None`；任一 target GC 失败返回 `Some(Err(msg))`，由调用方聚合进
    /// `FullSyncResult`。取消令牌已取消时跳过剩余 target 的 GC。
    ///
    /// `progress_sink`：Issue #763 评论 5831610228 — generation GC 循环里每处理一个
    /// planned target 时用 `set_target_phase` 更新 sink 的 target 信息，避免残留
    /// 最后一个 Transfer target 导致诊断包指错作品。
    fn run_full_sync_generation_gc(
        provider: &dyn crate::sync::provider::SyncProvider,
        plan: &crate::sync::full_sync::FullSyncPlan,
        catalog_snapshot: &crate::sync::types::RemoteTargetCatalogSnapshot,
        cancellation_token: Option<&SyncCancellationToken>,
        progress_sink: Option<&SyncProgressSink>,
    ) -> Option<Result<(), String>> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut generation_gc_result: Option<Result<(), String>> = None;
        for planned in &plan.targets {
            if Self::sync_cancelled(cancellation_token) {
                break;
            }
            // Issue #763 评论 5831610228：generation GC 循环里每处理一个 planned，
            // 都更新 sink 的 target 信息，避免残留最后一个 Transfer target 指错作品。
            if let Some(sink) = progress_sink {
                sink.set_target_phase(
                    &planned.target.remote_prefix,
                    planned.project_id.as_deref(),
                    "generation_gc",
                );
            }
            if !planned.target.remote_prefix.starts_with("projects/") {
                continue;
            }
            let active_generation = crate::sync::target_lifecycle::find_record(
                &catalog_snapshot.catalog,
                &planned.target.remote_prefix,
            )
            .and_then(|r| r.active_generation.as_deref());
            match crate::sync::generation_gc::run_generation_gc(
                provider,
                &planned.target.remote_prefix,
                active_generation,
                now_ms,
                crate::sync::generation_gc::GENERATION_RETENTION_MS,
                cancellation_token,
            ) {
                Ok(()) => {}
                Err(e) => {
                    log::warn!(
                        "[sync] perform_full_sync: generation GC failed for {}: {e}",
                        planned.target.remote_prefix
                    );
                    generation_gc_result = Some(Err(e.to_string()));
                }
            }
        }
        generation_gc_result
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
    pub fn resolve_conflict_keep_local(&self, project_id: &str, path: &str) -> ApiResult<bool> {
        // Issue #762 评论 5834136935：跨 WriterCoreApi 实例串行化同一 project root 的
        // state/conflict mutation。resolve 拿同一把 root lock，与 Commit 互斥：
        // resolve 先拿锁 → Commit 后读 live 时能看到已解决的状态；
        // Commit 先拿锁 → resolve 等 Commit 完成，再基于最终 live 状态解决。
        let project_root = self.projects_root.join(project_id);
        crate::sync::state_lock::with_sync_state_lock(&project_root, || {
            self.core_write()
                .resolve_conflict_keep_local(project_id, path)
                .map(|_| true)
                .map_err(Into::into)
        })
    }

    /// 冲突解决：采用远端版本。
    ///
    /// 返回 `applied_live` bool：`true` 表示已立即修改 live 正文（snapshot 替换/移入 trash），
    /// 平台层据此触发编辑器重载；`false` 表示仅排队 pending_take_remote（老数据兼容）。
    pub fn resolve_conflict_take_remote(&self, project_id: &str, path: &str) -> ApiResult<bool> {
        // Issue #762 评论 5834136935：跨 WriterCoreApi 实例串行化同一 project root 的
        // state/conflict mutation。resolve 拿同一把 root lock，与 Commit 互斥：
        // resolve 先拿锁 → Commit 后读 live 时能看到已解决的状态；
        // Commit 先拿锁 → resolve 等 Commit 完成，再基于最终 live 状态解决。
        let project_root = self.projects_root.join(project_id);
        crate::sync::state_lock::with_sync_state_lock(&project_root, || {
            self.core_write()
                .resolve_conflict_take_remote(project_id, path)
                .map_err(Into::into)
        })
    }

    /// 冲突解决：标记为已合并。
    pub fn resolve_conflict_mark_merged(&self, project_id: &str, path: &str) -> ApiResult<bool> {
        // Issue #762 评论 5834136935：跨 WriterCoreApi 实例串行化同一 project root 的
        // state/conflict mutation。resolve 拿同一把 root lock，与 Commit 互斥：
        // resolve 先拿锁 → Commit 后读 live 时能看到已解决的状态；
        // Commit 先拿锁 → resolve 等 Commit 完成，再基于最终 live 状态解决。
        let project_root = self.projects_root.join(project_id);
        crate::sync::state_lock::with_sync_state_lock(&project_root, || {
            self.core_write()
                .resolve_conflict_mark_merged(project_id, path)
                .map(|_| true)
                .map_err(Into::into)
        })
    }

    /// 加载冲突预览 — 返回本地/远端内容供平台层展示。
    ///
    /// 平台层只拿此 DTO，不直接读 `conflicts.json`，QML 更不能自己拼磁盘路径。
    pub fn load_sync_conflict_preview(
        &self,
        project_id: &str,
        path: &str,
    ) -> ApiResult<SyncConflictPreviewDto> {
        let preview = self
            .core_read()
            .load_sync_conflict_preview(project_id, path)
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

    /// 列出当前项目的所有冲突记录。
    pub fn list_sync_conflicts(&self, project_id: &str) -> ApiResult<Vec<SyncConflictDto>> {
        self.core_read()
            .list_sync_conflicts(project_id)
            .map(|conflicts| conflicts.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    /// 列出所有作品的所有未解决冲突。
    ///
    /// 跨作品全局冲突查询，返回扁平的 `ProjectSyncConflictDto`。
    /// 平台层按 projectId 分组展示。单个 project 读取失败时跳过，不阻断整体查询。
    pub fn list_all_sync_conflicts(&self) -> ApiResult<Vec<ProjectSyncConflictDto>> {
        self.core_read()
            .list_all_sync_conflicts()
            .map(|all| all.into_iter().map(Into::into).collect())
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
