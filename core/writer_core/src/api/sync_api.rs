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
            .map_err(Into::into)
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
    pub fn save_sync_secrets(&self, secrets: SyncSecretsDto) -> ApiResult<bool> {
        self.core_write()
            .save_sync_secrets(&secrets.into())
            .map(|_| true)
            .map_err(Into::into)
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
            .map_err(Into::into)
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
    pub fn perform_full_sync_dry_run(
        &self,
        config: SyncConfigDto,
    ) -> ApiResult<FullSyncDryRunResultDto> {
        let secrets = self.secrets_override_snapshot().unwrap_or_default();
        self.core_write()
            .perform_full_sync_dry_run(&config.into(), &secrets)
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
    #[cfg(feature = "github-api")]
    pub fn perform_full_sync(
        &self,
        config: SyncConfigDto,
        force_sync: bool,
    ) -> ApiResult<FullSyncResultDto> {
        let sync_config: crate::sync::SyncConfig = config.into();

        // Snapshot secrets before acquiring core_write（避免持锁期间回调 override）。
        let secrets = self.secrets_override_snapshot().unwrap_or_default();

        // Phase 1: Prepare（短写锁）— 写 Syncing、枚举 targets、创建 provider。
        // 不创建/seed staging runs（#644 评论 5473401065 第1节）。
        let (mut plan, provider) = {
            let core = self.core_write();
            let plan = core.prepare_full_sync(&sync_config, force_sync, secrets.clone())?;
            let provider = core.create_sync_provider_for_plan(&sync_config, &secrets)?;
            (plan, provider)
        };
        // 写锁已释放。

        // Phase 2: Seed staging（不持锁）— 磁盘扫描/复制，创建隔离 staging 目录。
        // #644 评论 5473401065 第2节：seed 失败直接终止本次同步，不继续拿半成品。
        // prepare_staging_runs 是纯函数，不依赖 WriterCore，无需持锁。
        //
        // #644 评论 5473551127 第1节：seed 失败时必须把 FullSyncState 从 Syncing
        // 改为失败终态，否则下次启动/同步会永久看到上一次遗留的 Syncing。
        //
        // #644 评论 5473551127 第2节：按 backend 类型选择对应 staging 方式，
        // Git 后端需要保留仓库身份（.git/HEAD/remote），不能共用 GithubApi 的文件复制。
        let resolved_active_provider = crate::sync::url::resolved_active_provider(&sync_config);
        let staging_runs = match crate::sync::staging::prepare_staging_runs(
            &mut plan,
            &resolved_active_provider,
        ) {
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
        let result = {
            let core = self.core_write();
            core.commit_full_sync(transfer_result, staging_runs)
        };

        Ok(result.into())
    }

    /// `perform_full_sync` 的非 github-api fallback — 无 LWW engine 可用。
    #[cfg(not(feature = "github-api"))]
    pub fn perform_full_sync(
        &self,
        _config: SyncConfigDto,
        _force_sync: bool,
    ) -> ApiResult<FullSyncResultDto> {
        Err(crate::Error::NotImplemented.into())
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
    use super::*;
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
