impl super::WriterCore {
    pub fn scan_sync_files(
        &self,
        project_id: &str,
    ) -> crate::error::Result<Vec<crate::sync::SyncFileEntry>> {
        crate::sync::SyncService::scan_for_sync(&self.project_root(project_id))
    }

    pub fn build_sync_plan(
        &self,
        project_id: &str,
    ) -> crate::error::Result<crate::sync::SyncPlan> {
        crate::sync::SyncService::build_sync_plan(&self.project_root(project_id))
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

    pub fn get_sync_ignored_paths(
        &self,
        project_id: &str,
    ) -> crate::error::Result<Vec<String>> {
        crate::sync::SyncService::get_sync_ignored_paths(&self.project_root(project_id))
    }

    pub fn perform_sync_diagnostics(
        &self,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<crate::sync::SyncDiagnosticsResult> {
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
        backend.diagnose(config, &secrets)
    }

    pub fn perform_sync_dry_run(
        &self,
        project_id: &str,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<crate::sync::SyncPlan> {
        crate::sync::SyncService::perform_sync_dry_run(&self.project_root(project_id), config)
    }

    pub fn perform_sync(
        &self,
        project_id: &str,
        config: &crate::sync::SyncConfig,
        force_sync: bool,
    ) -> crate::error::Result<crate::sync::SyncResult> {
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
        if let Some(ref storage) = self.secure_storage {
            if let Ok(Some(bytes)) = storage.get_secret("sync_token") {
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
                    let _ = storage.set_secret("sync_token", token.as_bytes());
                    let secrets_path = self
                        .app_data_root
                        .join("sync/sync_secrets.local.json");
                    let _ = std::fs::remove_file(&secrets_path);
                }
            }
            return Ok(file_secrets);
        }
        self.load_sync_secrets_from_file()
    }

    /// #592 五：进程级 secrets override — 一次同步操作只使用同一份 snapshot 的凭据，
    /// 不再从磁盘二次读取。由 app_service 在同步启动前设置。
    pub fn set_secrets_override(&mut self, secrets: Option<crate::sync::SyncSecrets>) {
        self.secrets_override = secrets;
    }

    pub(crate) fn has_secrets_override(&self) -> bool {
        self.secrets_override.is_some()
    }

    /// #592 五：按 generation 保存凭据到安全存储（key: sync_token_g{N}）。
    /// 凭据按 generation 保存，activeGeneration 提交前旧 generation 的凭据仍可读取。
    pub fn save_sync_secrets_for_generation(
        &self,
        generation: u64,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        let key = format!("sync_token_g{}", generation);
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
        // 无 secure storage（测试/桌面直连）：按 generation 落文件，与 live 槽同构。
        let secrets_path = self.app_data_root.join(format!(
            "sync/sync_secrets_g{}.local.json",
            generation
        ));
        if let Some(parent) = secrets_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(secrets)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        let parent = secrets_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let tmp = parent.join(format!("sync_secrets_g{}.local.json.tmp", generation));
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, &secrets_path)?;
        Ok(())
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
        generation: u64,
    ) -> crate::error::Result<Option<crate::sync::SyncSecrets>> {
        let key = format!("sync_token_g{}", generation);
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
        let secrets_path = self.app_data_root.join(format!(
            "sync/sync_secrets_g{}.local.json",
            generation
        ));
        if !secrets_path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&secrets_path)?;
        let secrets: crate::sync::SyncSecrets = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(Some(secrets))
    }

    /// #595 五：删除指定 generation 的安全存储凭据（key: sync_token_g{N}）。
    /// 用于 generation 提交成功后的旧版本清理：保留 current + previous 一个
    /// 可回滚版本，删除更旧 generation 的凭据；缺失/已删除视为成功。
    pub fn delete_sync_secrets_for_generation(&self, generation: u64) -> crate::error::Result<()> {
        let key = format!("sync_token_g{}", generation);
        if let Some(ref storage) = self.secure_storage {
            storage
                .delete_secret(&key)
                .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            return Ok(());
        }
        // 无 secure storage（测试/桌面直连）：删除按 generation 落的文件。
        let secrets_path = self.app_data_root.join(format!(
            "sync/sync_secrets_g{}.local.json",
            generation
        ));
        if secrets_path.exists() {
            std::fs::remove_file(&secrets_path)?;
        }
        Ok(())
    }

    fn load_sync_secrets_from_file(&self) -> crate::error::Result<crate::sync::SyncSecrets> {
        let secrets_path = self
            .app_data_root
            .join("sync/sync_secrets.local.json");
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
        if let Some(ref storage) = self.secure_storage {
            if let Some(token) = &secrets.token {
                storage
                    .set_secret("sync_token", token.as_bytes())
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            } else {
                storage
                    .delete_secret("sync_token")
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            }
            return Ok(());
        }
        self.save_sync_secrets_to_file(secrets)
    }

    fn save_sync_secrets_to_file(
        &self,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        let secrets_path = self
            .app_data_root
            .join("sync/sync_secrets.local.json");
        if let Some(parent) = secrets_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(secrets)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        let parent = secrets_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let mut tmp_file = tempfile::Builder::new()
            .prefix("sync_secrets")
            .suffix(".tmp")
            .tempfile_in(parent)?;

        use std::io::Write;
        tmp_file.write_all(content.as_bytes())?;
        tmp_file.persist(secrets_path).map_err(|e| e.error)?;

        Ok(())
    }

    pub fn load_sync_config(&self) -> crate::error::Result<crate::sync::SyncConfig> {
        let config_path = self.app_data_root.join("sync/sync_config.json");
        if !config_path.exists() {
            return Ok(crate::sync::SyncConfig {
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
            });
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

    pub fn save_sync_config(&self, config: &crate::sync::SyncConfig) -> crate::error::Result<()> {
        let config_path = self.app_data_root.join("sync/sync_config.json");
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

    pub fn validate_sync_config(
        &self,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<bool> {
        if config.enabled && config.remote_url.is_empty() {
            return Ok(false);
        }
        Ok(true)
    }
}
