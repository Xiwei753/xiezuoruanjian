//! 同步 facade — 全量同步统一入口（Issue #630）。
//!
//! 一个全局 `SyncConfig` + 一份全局凭据，`perform_full_sync` 内部按 `SyncTarget`
//! 把不同本地根映射到同一个远端仓库的不同前缀：
//! - App target：`<app_data_root>` → `app/`
//! - Project target：`<project_root>` → `projects/<project_id>/`
//!
//! 旧的"作品同步 + 应用数据同步"两套用户配置入口已删除。

impl super::WriterCore {
    pub fn scan_sync_files(
        &self,
        project_id: &str,
    ) -> crate::error::Result<Vec<crate::sync::SyncFileEntry>> {
        crate::sync::SyncService::scan_for_sync(
            &self.project_root(project_id),
            crate::sync::types::SyncScope::Project,
        )
    }

    pub fn build_sync_plan(&self, project_id: &str) -> crate::error::Result<crate::sync::SyncPlan> {
        crate::sync::SyncService::build_sync_plan(
            &self.project_root(project_id),
            crate::sync::types::SyncScope::Project,
        )
    }

    /// Project target 同步状态。路径：`<project_root>/app-meta/sync/state.local.json`。
    pub fn load_sync_state(
        &self,
        project_id: &str,
    ) -> crate::error::Result<crate::sync::SyncState> {
        crate::sync::SyncService::load_sync_state(&self.project_root(project_id))
    }

    pub fn save_sync_state(
        &self,
        project_id: &str,
        state: &crate::sync::SyncState,
    ) -> crate::error::Result<()> {
        crate::sync::SyncService::save_sync_state(&self.project_root(project_id), state)
    }

    pub fn record_sync_conflict(
        &self,
        project_id: &str,
        conflict: crate::sync::SyncConflict,
        local_content: Option<&str>,
    ) -> crate::error::Result<()> {
        crate::sync::SyncService::record_sync_conflict(
            &self.project_root(project_id),
            conflict,
            local_content,
        )
    }

    pub fn resolve_conflict_keep_local(
        &self,
        project_id: &str,
        path: &str,
    ) -> crate::error::Result<()> {
        crate::sync::SyncService::resolve_conflict_keep_local(&self.project_root(project_id), path)
    }

    pub fn resolve_conflict_take_remote(
        &self,
        project_id: &str,
        path: &str,
    ) -> crate::error::Result<()> {
        crate::sync::SyncService::resolve_conflict_take_remote(&self.project_root(project_id), path)
    }

    pub fn resolve_conflict_mark_merged(
        &self,
        project_id: &str,
        path: &str,
    ) -> crate::error::Result<()> {
        crate::sync::SyncService::resolve_conflict_mark_merged(&self.project_root(project_id), path)
    }

    pub fn get_sync_ignored_paths(&self, project_id: &str) -> crate::error::Result<Vec<String>> {
        crate::sync::SyncService::get_sync_ignored_paths(
            &self.project_root(project_id),
            crate::sync::types::SyncScope::Project,
        )
    }

    // ── App target 同步状态（per-target 状态查询，非配置入口） ──

    /// App target 同步状态。路径：`<app_data_root>/app-meta/sync/state.local.json`。
    pub fn load_app_sync_state(&self) -> crate::error::Result<crate::sync::SyncState> {
        crate::sync::SyncService::load_sync_state(&self.app_data_root)
    }

    pub fn save_app_sync_state(&self, state: &crate::sync::SyncState) -> crate::error::Result<()> {
        crate::sync::SyncService::save_sync_state(&self.app_data_root, state)
    }

    // ── 全局配置 + 全局凭据（Issue #630：唯一一份） ──

    /// 加载全局同步配置。路径：`<app_data_root>/app-meta/sync/config.local.json`。
    pub fn load_sync_config(&self) -> crate::error::Result<crate::sync::SyncConfig> {
        let config_path = self.app_data_root.join("app-meta/sync/config.local.json");
        if !config_path.exists() {
            return Ok(default_sync_config());
        }
        let content = std::fs::read_to_string(&config_path)?;
        let raw: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        let mut config: crate::sync::SyncConfig = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

        let backend_missing = raw
            .as_object()
            .map(|obj| !obj.contains_key("backend_type"))
            .unwrap_or(false);
        let should_migrate = crate::sync::is_github_https_remote(&config.remote_url)
            && (backend_missing || config.backend_type == crate::sync::BackendType::Git);
        if should_migrate {
            config.backend_type = crate::sync::BackendType::GithubApi;
            self.save_sync_config(&config)?;
        }
        Ok(config)
    }

    /// 保存全局同步配置。路径：`<app_data_root>/app-meta/sync/config.local.json`。
    pub fn save_sync_config(&self, config: &crate::sync::SyncConfig) -> crate::error::Result<()> {
        let config_path = self.app_data_root.join("app-meta/sync/config.local.json");
        save_config_atomic(&config_path, config)
    }

    pub fn validate_sync_config(
        &self,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<bool> {
        if config.enabled && config.remote_url.is_empty() {
            return Ok(false);
        }
        Ok(true)
    }

    /// 加载全局同步凭据。安全存储 key = `sync_token_global`。
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn load_sync_secrets(&self) -> crate::error::Result<crate::sync::SyncSecrets> {
        if let Some(ref override_secrets) = self.secrets_override {
            return Ok(override_secrets.clone());
        }
        const GLOBAL_KEY: &str = "sync_token_global";
        if let Some(ref storage) = self.secure_storage {
            if let Ok(Some(bytes)) = storage.get_secret(GLOBAL_KEY) {
                if let Ok(token) = String::from_utf8(bytes) {
                    if !token.is_empty() {
                        return Ok(crate::sync::SyncSecrets {
                            token: Some(token),
                            ssh_private_key: None,
                        });
                    }
                }
            }
            let file_secrets = self.load_sync_secrets_from_file()?;
            if let Some(token) = &file_secrets.token {
                if !token.is_empty() {
                    let _ = storage.set_secret(GLOBAL_KEY, token.as_bytes());
                    let secrets_path = self.app_data_root.join("app-meta/sync/secrets.local.json");
                    let _ = std::fs::remove_file(&secrets_path);
                }
            }
            return Ok(file_secrets);
        }
        self.load_sync_secrets_from_file()
    }

    /// #592 五：进程级 secrets override — 一次同步操作只使用同一份 snapshot 的凭据。
    pub fn set_secrets_override(&mut self, secrets: Option<crate::sync::SyncSecrets>) {
        self.secrets_override = secrets;
    }

    pub(crate) fn has_secrets_override(&self) -> bool {
        self.secrets_override.is_some()
    }

    /// #592 五：按 generation 保存凭据到安全存储（key: sync_token_global_g{N}）。
    pub fn save_sync_secrets_for_generation(
        &self,
        generation: u64,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        let key = format!("sync_token_global_g{}", generation);
        if let Some(ref storage) = self.secure_storage {
            if let Some(token) = &secrets.token {
                storage
                    .set_secret(&key, token.as_bytes())
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            } else {
                storage
                    .delete_secret(&key)
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            }
            return Ok(());
        }
        let secrets_path = self
            .app_data_root
            .join(format!("app-meta/sync/secrets_g{}.local.json", generation));
        write_secrets_atomic(
            &secrets_path,
            secrets,
            &format!("secrets_g{}.local.json", generation),
        )
    }

    /// #592 五：读取指定 generation 的安全存储凭据；缺失返回 None。
    pub fn load_sync_secrets_for_generation(
        &self,
        generation: u64,
    ) -> crate::error::Result<Option<crate::sync::SyncSecrets>> {
        let key = format!("sync_token_global_g{}", generation);
        if let Some(ref storage) = self.secure_storage {
            let token = storage
                .get_secret(&key)
                .ok()
                .flatten()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .filter(|t| !t.is_empty());
            return Ok(token.map(|t| crate::sync::SyncSecrets {
                token: Some(t),
                ssh_private_key: None,
            }));
        }
        let secrets_path = self
            .app_data_root
            .join(format!("app-meta/sync/secrets_g{}.local.json", generation));
        if !secrets_path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&secrets_path)?;
        let secrets: crate::sync::SyncSecrets = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(Some(secrets))
    }

    /// #595 五：删除指定 generation 的安全存储凭据。
    pub fn delete_sync_secrets_for_generation(&self, generation: u64) -> crate::error::Result<()> {
        let key = format!("sync_token_global_g{}", generation);
        if let Some(ref storage) = self.secure_storage {
            storage
                .delete_secret(&key)
                .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            return Ok(());
        }
        let secrets_path = self
            .app_data_root
            .join(format!("app-meta/sync/secrets_g{}.local.json", generation));
        if secrets_path.exists() {
            std::fs::remove_file(&secrets_path)?;
        }
        Ok(())
    }

    fn load_sync_secrets_from_file(&self) -> crate::error::Result<crate::sync::SyncSecrets> {
        let secrets_path = self.app_data_root.join("app-meta/sync/secrets.local.json");
        if !secrets_path.exists() {
            return Ok(crate::sync::SyncSecrets::default());
        }
        let content = std::fs::read_to_string(&secrets_path)?;
        let secrets: crate::sync::SyncSecrets = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(secrets)
    }

    pub fn save_sync_secrets(
        &self,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        const GLOBAL_KEY: &str = "sync_token_global";
        if let Some(ref storage) = self.secure_storage {
            if let Some(token) = &secrets.token {
                storage
                    .set_secret(GLOBAL_KEY, token.as_bytes())
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            } else {
                storage
                    .delete_secret(GLOBAL_KEY)
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            }
            return Ok(());
        }
        let secrets_path = self.app_data_root.join("app-meta/sync/secrets.local.json");
        write_secrets_atomic(&secrets_path, secrets, "sync_secrets")
    }

    // ── 全量同步统一入口（Issue #630） ──

    /// 全量同步诊断 — 只测一次仓库、分支、token。
    pub fn perform_full_sync_diagnostics(
        &self,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<crate::sync::types::FullSyncDiagnosticsResult> {
        let secrets = self.load_sync_secrets().unwrap_or_default();
        let diagnostics = self.run_sync_diagnostics(config, &secrets)?;
        Ok(crate::sync::types::FullSyncDiagnosticsResult { diagnostics })
    }

    /// 全量同步 dry-run — 枚举 App target + 所有 Project target，构建每个 target 的计划。
    pub fn perform_full_sync_dry_run(
        &self,
        config: &crate::sync::SyncConfig,
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
    pub fn perform_full_sync(
        &self,
        config: &crate::sync::SyncConfig,
        force_sync: bool,
    ) -> crate::error::Result<crate::sync::types::FullSyncResult> {
        use crate::sync::types::{SyncTarget, TargetSyncResult};

        let secrets = self.load_sync_secrets().unwrap_or_default();
        let backend_type = crate::sync::resolved_backend_type(config);
        let backend = if let Some(transport) = self.sync_transport.as_ref() {
            match transport() {
                Ok(t) => crate::sync::create_sync_backend_with_transport(&backend_type, t),
                Err(e) => {
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "Transport init failed: {} - {}",
                        e.category, e.message
                    ))))
                }
            }
        } else {
            crate::sync::create_sync_backend(&backend_type)
        };

        let mut targets: Vec<TargetSyncResult> = Vec::new();

        // App target
        let app_target = SyncTarget::app();
        let app_result = backend.sync(
            &self.app_data_root,
            config,
            &secrets,
            &app_target,
            force_sync,
        )?;
        targets.push(TargetSyncResult {
            target_kind: "app".to_string(),
            project_id: None,
            remote_prefix: app_target.remote_prefix.clone(),
            result: app_result,
        });

        // Project targets
        let projects = self.list_projects()?;
        for project in &projects {
            let target = SyncTarget::project(&project.id);
            let result = backend.sync(
                &self.project_root(&project.id),
                config,
                &secrets,
                &target,
                force_sync,
            )?;
            targets.push(TargetSyncResult {
                target_kind: "project".to_string(),
                project_id: Some(project.id.clone()),
                remote_prefix: target.remote_prefix.clone(),
                result,
            });
        }

        let result = Self::aggregate_full_sync_result(targets);

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
    /// 按任一错误 → Error、任一冲突 → PartialConflict、否则 Success 决定总体状态。
    fn aggregate_full_sync_result(
        targets: Vec<crate::sync::types::TargetSyncResult>,
    ) -> crate::sync::types::FullSyncResult {
        use crate::sync::types::FullSyncResult;

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

        let any_error = targets.iter().any(|t| {
            matches!(
                t.result.status,
                crate::sync::SyncStatus::FatalError(_)
                    | crate::sync::SyncStatus::Error(_)
                    | crate::sync::SyncStatus::RecoverableError(_)
                    | crate::sync::SyncStatus::DirtyRepoBlocked
            )
        });
        let any_conflict = targets.iter().any(|t| {
            matches!(
                t.result.status,
                crate::sync::SyncStatus::Conflict | crate::sync::SyncStatus::PartialConflict
            )
        });
        let overall_status = if any_error {
            crate::sync::SyncStatus::Error("one_or_more_targets_failed".to_string())
        } else if any_conflict {
            crate::sync::SyncStatus::PartialConflict
        } else {
            crate::sync::SyncStatus::Success
        };

        let error = targets.iter().find_map(|t| t.result.error.clone());
        let error_category = targets.iter().find_map(|t| t.result.error_category.clone());
        let message_key = error_category.as_deref().map(|c| {
            crate::sync::types::SyncErrorCategory::from_code(c, "")
                .to_message_key()
                .to_string()
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
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "Transport init failed: {} - {}",
                        e.category, e.message
                    ))))
                }
            }
        } else {
            crate::sync::create_sync_backend(&backend_type)
        };
        backend.diagnose(config, secrets)
    }
}

/// 写 secrets 到文件的原子操作。
fn write_secrets_atomic(
    secrets_path: &std::path::Path,
    secrets: &crate::sync::SyncSecrets,
    tmp_prefix: &str,
) -> crate::error::Result<()> {
    if let Some(parent) = secrets_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(secrets)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let parent = secrets_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new(""));
    let mut tmp_file = tempfile::Builder::new()
        .prefix(tmp_prefix)
        .suffix(".tmp")
        .tempfile_in(parent)?;

    use std::io::Write;
    tmp_file.write_all(content.as_bytes())?;
    tmp_file.persist(secrets_path).map_err(|e| e.error)?;

    Ok(())
}

/// 原子写入 sync config。
fn save_config_atomic(
    config_path: &std::path::Path,
    config: &crate::sync::SyncConfig,
) -> crate::error::Result<()> {
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let tmp_path = config_path.with_extension("tmp");
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(tmp_path, config_path)?;
    Ok(())
}

/// 默认同步配置（未配置时返回）。
fn default_sync_config() -> crate::sync::SyncConfig {
    crate::sync::SyncConfig {
        enabled: false,
        backend_type: crate::sync::BackendType::GithubApi,
        remote_url: String::new(),
        transport: crate::sync::SyncProtocol::HttpsToken,
        branch: "main".to_string(),
        auto_sync: false,
        sync_interval_seconds: 300,
        username: String::new(),
        has_network_permission: true,
        has_network_state_permission: true,
    }
}
