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
            crate::sync::SyncScope::Project,
        )
    }

    /// 作品级同步诊断。`project_id` 指定要诊断的作品，凭据从该作品的
    /// 安全存储/文件读取（或使用进程级 override）。
    pub fn perform_sync_diagnostics(
        &self,
        project_id: &str,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<crate::sync::SyncDiagnosticsResult> {
        let secrets = self.load_sync_secrets(project_id).unwrap_or_default();
        self.run_sync_diagnostics(config, &secrets)
    }

    pub fn perform_sync_dry_run(
        &self,
        project_id: &str,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<crate::sync::SyncPlan> {
        crate::sync::SyncService::perform_sync_dry_run(&self.project_root(project_id), config)
    }

    /// 作品级同步。同步根 = `projects_root/<project_id>`，
    /// 凭据从该作品的安全存储/文件读取（或使用进程级 override）。
    pub fn perform_sync(
        &self,
        project_id: &str,
        config: &crate::sync::SyncConfig,
        force_sync: bool,
    ) -> crate::error::Result<crate::sync::SyncResult> {
        let secrets = self.load_sync_secrets(project_id).unwrap_or_default();
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
        let result = backend.sync(&self.project_root(project_id), config, &secrets, force_sync)?;
        let mut result = result;
        if matches!(
            result.status,
            crate::sync::SyncStatus::Success
                | crate::sync::SyncStatus::LatestWinsApplied
                | crate::sync::SyncStatus::BranchMissingRecovered
        ) {
            if let Err(e) = self.rebuild_search_index(None) {
                log::warn!("Failed to rebuild search index after sync: {e}");
                result.search_index_rebuild_error = Some(format!("{e}"));
            }
        }
        Ok(result)
    }

    // ── 作品级 secrets ──

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn load_sync_secrets(
        &self,
        project_id: &str,
    ) -> crate::error::Result<crate::sync::SyncSecrets> {
        if let Some(ref override_secrets) = self.secrets_override {
            return Ok(override_secrets.clone());
        }
        let storage_key = format!("sync_token_{}", project_id);
        if let Some(ref storage) = self.secure_storage {
            if let Ok(Some(bytes)) = storage.get_secret(&storage_key) {
                if let Ok(token) = String::from_utf8(bytes) {
                    if !token.is_empty() {
                        return Ok(crate::sync::SyncSecrets {
                            token: Some(token),
                            ssh_private_key: None,
                        });
                    }
                }
            }
            let file_secrets = self.load_sync_secrets_from_file(project_id)?;
            if let Some(token) = &file_secrets.token {
                if !token.is_empty() {
                    let _ = storage.set_secret(&storage_key, token.as_bytes());
                    let secrets_path = self
                        .project_root(project_id)
                        .join("app-meta/sync/secrets.local.json");
                    let _ = std::fs::remove_file(&secrets_path);
                }
            }
            return Ok(file_secrets);
        }
        self.load_sync_secrets_from_file(project_id)
    }

    /// #592 五：进程级 secrets override — 一次同步操作只使用同一份 snapshot 的凭据，
    /// 不再从磁盘二次读取。由 app_service 在同步启动前设置。
    pub fn set_secrets_override(&mut self, secrets: Option<crate::sync::SyncSecrets>) {
        self.secrets_override = secrets;
    }

    pub(crate) fn has_secrets_override(&self) -> bool {
        self.secrets_override.is_some()
    }

    /// #592 五：按 generation 保存凭据到安全存储（key: sync_token_<project_id>_g{N}）。
    pub fn save_sync_secrets_for_generation(
        &self,
        project_id: &str,
        generation: u64,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        let key = format!("sync_token_{}_g{}", project_id, generation);
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
            .project_root(project_id)
            .join(format!("app-meta/sync/secrets_g{}.local.json", generation));
        write_secrets_atomic(
            &secrets_path,
            secrets,
            &format!("secrets_g{}.local.json", generation),
        )
    }

    /// #592 五：读取指定 generation 的安全存储凭据；缺失返回 None。
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn load_sync_secrets_for_generation(
        &self,
        project_id: &str,
        generation: u64,
    ) -> crate::error::Result<Option<crate::sync::SyncSecrets>> {
        let key = format!("sync_token_{}_g{}", project_id, generation);
        if let Some(ref storage) = self.secure_storage {
            if let Ok(Some(bytes)) = storage.get_secret(&key) {
                if let Ok(token) = String::from_utf8(bytes) {
                    if !token.is_empty() {
                        return Ok(Some(crate::sync::SyncSecrets {
                            token: Some(token),
                            ssh_private_key: None,
                        }));
                    }
                }
            }
            return Ok(None);
        }
        let secrets_path = self
            .project_root(project_id)
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
    pub fn delete_sync_secrets_for_generation(
        &self,
        project_id: &str,
        generation: u64,
    ) -> crate::error::Result<()> {
        let key = format!("sync_token_{}_g{}", project_id, generation);
        if let Some(ref storage) = self.secure_storage {
            storage
                .delete_secret(&key)
                .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            return Ok(());
        }
        let secrets_path = self
            .project_root(project_id)
            .join(format!("app-meta/sync/secrets_g{}.local.json", generation));
        if secrets_path.exists() {
            std::fs::remove_file(&secrets_path)?;
        }
        Ok(())
    }

    fn load_sync_secrets_from_file(
        &self,
        project_id: &str,
    ) -> crate::error::Result<crate::sync::SyncSecrets> {
        let secrets_path = self
            .project_root(project_id)
            .join("app-meta/sync/secrets.local.json");
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
        project_id: &str,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        let storage_key = format!("sync_token_{}", project_id);
        if let Some(ref storage) = self.secure_storage {
            if let Some(token) = &secrets.token {
                storage
                    .set_secret(&storage_key, token.as_bytes())
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            } else {
                storage
                    .delete_secret(&storage_key)
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            }
            return Ok(());
        }
        self.save_sync_secrets_to_file(project_id, secrets)
    }

    fn save_sync_secrets_to_file(
        &self,
        project_id: &str,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        let secrets_path = self
            .project_root(project_id)
            .join("app-meta/sync/secrets.local.json");
        write_secrets_atomic(&secrets_path, secrets, "sync_secrets")
    }

    // ── 作品级 sync config ──

    /// 加载作品级同步配置。路径：`<project_root>/app-meta/sync/config.local.json`。
    pub fn load_sync_config(
        &self,
        project_id: &str,
    ) -> crate::error::Result<crate::sync::SyncConfig> {
        let config_path = self
            .project_root(project_id)
            .join("app-meta/sync/config.local.json");
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
            self.save_sync_config(project_id, &config)?;
        }
        Ok(config)
    }

    /// 保存作品级同步配置。路径：`<project_root>/app-meta/sync/config.local.json`。
    pub fn save_sync_config(
        &self,
        project_id: &str,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<()> {
        let config_path = self
            .project_root(project_id)
            .join("app-meta/sync/config.local.json");
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

    // ── 应用级同步通道（Issue #600 评论 #3 问题四） ──

    /// 加载应用级同步配置。路径：`<app_data_root>/app-meta/sync/config.local.json`。
    pub fn load_app_sync_config(&self) -> crate::error::Result<crate::sync::SyncConfig> {
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
            self.save_app_sync_config(&config)?;
        }
        Ok(config)
    }

    /// 保存应用级同步配置。路径：`<app_data_root>/app-meta/sync/config.local.json`。
    pub fn save_app_sync_config(
        &self,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<()> {
        let config_path = self.app_data_root.join("app-meta/sync/config.local.json");
        save_config_atomic(&config_path, config)
    }

    /// 加载应用级同步凭据。安全存储 key = `sync_token_app`。
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn load_app_sync_secrets(&self) -> crate::error::Result<crate::sync::SyncSecrets> {
        if let Some(ref override_secrets) = self.secrets_override {
            return Ok(override_secrets.clone());
        }
        const APP_KEY: &str = "sync_token_app";
        if let Some(ref storage) = self.secure_storage {
            if let Ok(Some(bytes)) = storage.get_secret(APP_KEY) {
                if let Ok(token) = String::from_utf8(bytes) {
                    if !token.is_empty() {
                        return Ok(crate::sync::SyncSecrets {
                            token: Some(token),
                            ssh_private_key: None,
                        });
                    }
                }
            }
            let file_secrets = self.load_app_sync_secrets_from_file()?;
            if let Some(token) = &file_secrets.token {
                if !token.is_empty() {
                    let _ = storage.set_secret(APP_KEY, token.as_bytes());
                    let secrets_path = self.app_data_root.join("app-meta/sync/secrets.local.json");
                    let _ = std::fs::remove_file(&secrets_path);
                }
            }
            return Ok(file_secrets);
        }
        self.load_app_sync_secrets_from_file()
    }

    /// 保存应用级同步凭据。
    pub fn save_app_sync_secrets(
        &self,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        const APP_KEY: &str = "sync_token_app";
        if let Some(ref storage) = self.secure_storage {
            if let Some(token) = &secrets.token {
                storage
                    .set_secret(APP_KEY, token.as_bytes())
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            } else {
                storage
                    .delete_secret(APP_KEY)
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            }
            return Ok(());
        }
        let secrets_path = self.app_data_root.join("app-meta/sync/secrets.local.json");
        write_secrets_atomic(&secrets_path, secrets, "sync_secrets")
    }

    fn load_app_sync_secrets_from_file(&self) -> crate::error::Result<crate::sync::SyncSecrets> {
        let secrets_path = self.app_data_root.join("app-meta/sync/secrets.local.json");
        if !secrets_path.exists() {
            return Ok(crate::sync::SyncSecrets::default());
        }
        let content = std::fs::read_to_string(&secrets_path)?;
        let secrets: crate::sync::SyncSecrets = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(secrets)
    }

    /// 应用级：按 generation 保存凭据（key: sync_token_app_g{N}）。
    pub fn save_app_sync_secrets_for_generation(
        &self,
        generation: u64,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        let key = format!("sync_token_app_g{}", generation);
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

    /// 应用级：读取指定 generation 的凭据；缺失返回 None。
    pub fn load_app_sync_secrets_for_generation(
        &self,
        generation: u64,
    ) -> crate::error::Result<Option<crate::sync::SyncSecrets>> {
        let key = format!("sync_token_app_g{}", generation);
        if let Some(ref storage) = self.secure_storage {
            let result = storage
                .get_secret(&key)
                .ok()
                .flatten()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .filter(|token| !token.is_empty())
                .map(|token| crate::sync::SyncSecrets {
                    token: Some(token),
                    ssh_private_key: None,
                });
            return Ok(result);
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

    /// 应用级：删除指定 generation 的凭据。
    pub fn delete_app_sync_secrets_for_generation(
        &self,
        generation: u64,
    ) -> crate::error::Result<()> {
        let key = format!("sync_token_app_g{}", generation);
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

    /// 应用级同步诊断。同步根 = `app_data_root`。
    pub fn perform_app_sync_diagnostics(
        &self,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<crate::sync::SyncDiagnosticsResult> {
        let secrets = self.load_app_sync_secrets().unwrap_or_default();
        self.run_sync_diagnostics(config, &secrets)
    }

    /// 应用级同步干运行。同步根 = `app_data_root`，使用应用级白名单/黑名单。
    pub fn perform_app_sync_dry_run(
        &self,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<crate::sync::SyncPlan> {
        if !config.enabled {
            return Ok(crate::sync::SyncPlan::new());
        }
        crate::sync::SyncService::build_sync_plan(
            &self.app_data_root,
            crate::sync::types::SyncScope::App,
        )
    }

    /// 应用级同步。同步根 = `app_data_root`，使用应用级白名单/黑名单。
    /// `config.scope` 被强制设为 `App`，确保后端使用应用级路径过滤。
    pub fn perform_app_sync(
        &self,
        config: &crate::sync::SyncConfig,
        force_sync: bool,
    ) -> crate::error::Result<crate::sync::SyncResult> {
        let secrets = self.load_app_sync_secrets().unwrap_or_default();
        let mut app_config = config.clone();
        app_config.scope = crate::sync::types::SyncScope::App;
        let backend_type = crate::sync::resolved_backend_type(&app_config);
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
        backend.sync(&self.app_data_root, &app_config, &secrets, force_sync)
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

/// 写 secrets 到文件的原子操作（作品级和应用级共用）。
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
        scope: crate::sync::types::SyncScope::Project,
    }
}
