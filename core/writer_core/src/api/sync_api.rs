use super::service::{ApiResult, WriterCoreApi};
use super::types::*;

/// 同步 API — 跨平台同步配置、执行和冲突解决契约。
///
/// 同步配置（config）和密钥（secrets）分开存储：config 可同步，secrets 仅存本地。
/// 冲突解决提供三种策略：保留本地、采用远端、标记已合并。
impl WriterCoreApi {
    /// 加载同步配置（含 remote_url、backend_type、auto_sync 等）。
    pub fn load_sync_config(&self) -> ApiResult<SyncConfigDto> {
        self.core()
            .load_sync_config()
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 保存同步配置。成功返回 true。
    pub fn save_sync_config(&self, config: SyncConfigDto) -> ApiResult<bool> {
        self.core()
            .save_sync_config(&config.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    /// 加载同步密钥（token 等）。密钥不同步到远端，仅存本地。
    pub fn load_sync_secrets(&self) -> ApiResult<SyncSecretsDto> {
        self.core()
            .load_sync_secrets()
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 保存同步密钥。成功返回 true。
    pub fn save_sync_secrets(&self, secrets: SyncSecretsDto) -> ApiResult<bool> {
        self.core()
            .save_sync_secrets(&secrets.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    /// #592 五：设置进程级 secrets override — 一次同步操作只使用同一份 snapshot 凭据。
    pub fn set_sync_secrets_override(&self, secrets: SyncSecretsDto) -> ApiResult<()> {
        self.core().set_secrets_override(Some(secrets.into()));
        Ok(())
    }

    /// #595 十：清除进程级 secrets override — 同步操作结束后调用，
    /// 陈旧凭据不得泄漏到后续操作（refresh_secrets_override 在已有 override 时
    /// 不会重新读取磁盘）。
    pub fn clear_sync_secrets_override(&self) -> ApiResult<()> {
        self.core().set_secrets_override(None);
        Ok(())
    }

    /// #592 五：按 generation 保存凭据到安全存储。
    pub fn save_sync_secrets_for_generation(
        &self,
        generation: u64,
        secrets: SyncSecretsDto,
    ) -> ApiResult<bool> {
        self.core()
            .save_sync_secrets_for_generation(generation, &secrets.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    /// #592 五：读取指定 generation 的安全存储凭据；缺失返回 None。
    pub fn load_sync_secrets_for_generation(
        &self,
        generation: u64,
    ) -> ApiResult<Option<SyncSecretsDto>> {
        self.core()
            .load_sync_secrets_for_generation(generation)
            .map(|opt| opt.map(Into::into))
            .map_err(Into::into)
    }

    /// #595 五：删除指定 generation 的安全存储凭据（旧版本清理）。
    pub fn delete_sync_secrets_for_generation(&self, generation: u64) -> ApiResult<()> {
        self.core()
            .delete_sync_secrets_for_generation(generation)
            .map_err(Into::into)
    }

    /// 加载同步状态（上次同步时间、远端 commit 等）。
    /// sync state 存储在 project_root/sync/ 下，每个作品独立维护同步状态。
    pub fn load_sync_state(&self, project_id: &str) -> ApiResult<SyncStateDto> {
        self.core()
            .load_sync_state(project_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 执行同步诊断——检查网络、认证、仓库、分支可达性。
    pub fn perform_sync_diagnostics(
        &self,
        config: SyncConfigDto,
    ) -> ApiResult<SyncDiagnosticsResultDto> {
        self.core()
            .perform_sync_diagnostics(&config.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 干运行——计算同步计划但不实际执行文件传输。
    /// 每个作品目录是独立的 Git 仓库，sync 针对单个作品执行。
    pub fn perform_sync_dry_run(
        &self,
        project_id: &str,
        config: SyncConfigDto,
    ) -> ApiResult<SyncPlanDto> {
        self.core()
            .perform_sync_dry_run(project_id, &config.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 执行同步。`force_sync=true` 跳过部分安全检查（如脏仓库保护）。
    /// 每个作品目录是独立的 Git 仓库，sync 针对单个作品执行。
    pub fn perform_sync(
        &self,
        project_id: &str,
        config: SyncConfigDto,
        force_sync: bool,
    ) -> ApiResult<SyncResultDto> {
        self.core()
            .perform_sync(project_id, &config.into(), force_sync)
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 冲突解决：保留本地版本，丢弃远端变更。
    /// 冲突状态存储在 project_root/sync/ 下，需指定作品。
    pub fn resolve_conflict_keep_local(&self, project_id: &str, path: &str) -> ApiResult<bool> {
        self.core()
            .resolve_conflict_keep_local(project_id, path)
            .map(|_| true)
            .map_err(Into::into)
    }

    /// 冲突解决：采用远端版本，覆盖本地。
    /// 冲突状态存储在 project_root/sync/ 下，需指定作品。
    pub fn resolve_conflict_take_remote(&self, project_id: &str, path: &str) -> ApiResult<bool> {
        self.core()
            .resolve_conflict_take_remote(project_id, path)
            .map(|_| true)
            .map_err(Into::into)
    }

    /// 冲突解决：标记为已合并（用户已手动处理）。
    /// 冲突状态存储在 project_root/sync/ 下，需指定作品。
    pub fn resolve_conflict_mark_merged(&self, project_id: &str, path: &str) -> ApiResult<bool> {
        self.core()
            .resolve_conflict_mark_merged(project_id, path)
            .map(|_| true)
            .map_err(Into::into)
    }

    /// 检查同步能力——综合 config 和 secrets 判断是否可执行同步。
    /// 返回 `can_run`、`block_reason_code`（如 "DISABLED"/"TOKEN_MISSING"）和 i18n key。
    #[allow(clippy::unwrap_used)]
    pub fn get_sync_capability(&self) -> ApiResult<SyncCapabilityDto> {
        let config = self.load_sync_config()?;
        let secrets = self.load_sync_secrets()?;

        let mut block_reason_code = None;
        let mut block_message_key = None;
        let message_args = std::collections::HashMap::new();
        let mut can_run = true;

        if !config.enabled {
            can_run = false;
            block_reason_code = Some("DISABLED".to_string());
            block_message_key = Some("sync.block.disabled".to_string());
        } else if self.secure_storage.is_none() {
            can_run = false;
            block_reason_code = Some("SECURE_STORAGE_UNAVAILABLE".to_string());
            block_message_key = Some("sync.block.secure_storage_unavailable".to_string());
        } else if config.remote_url.is_empty() {
            can_run = false;
            block_reason_code = Some("REMOTE_URL_MISSING".to_string());
            block_message_key = Some("sync.block.remote_url_missing".to_string());
        } else if secrets.token.is_none() || secrets.token.as_ref().unwrap().is_empty() {
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
    fn test_load_sync_secrets() {
        let temp_dir = tempdir().unwrap();
        let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));

        // Test loading when no secrets exist (should return default/empty struct)
        let loaded_empty = api.load_sync_secrets().unwrap();
        assert_eq!(loaded_empty.token, None);

        // Save some dummy secrets
        let dummy_secrets = SyncSecretsDto {
            token: Some("ghp_dummy123".to_string()),
        };
        api.save_sync_secrets(dummy_secrets.clone()).unwrap();

        // Test loading the saved secrets
        let loaded_secrets = api.load_sync_secrets().unwrap();
        assert_eq!(loaded_secrets.token, dummy_secrets.token);
    }

    #[test]
    fn save_sync_config_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
        let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));

        let config = SyncConfigDto {
            enabled: true,
            backend_type: "github_api".to_string(),
            remote_url: "https://github.com/test/repo.git".to_string(),
            transport: "https_token".to_string(),
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            username: "".to_string(),
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let result = api.save_sync_config(config);
        assert!(result.unwrap());
    }
}
