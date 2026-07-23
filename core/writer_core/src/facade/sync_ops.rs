impl super::WriterCore {
    pub fn scan_sync_files(&self) -> crate::error::Result<Vec<crate::sync::SyncFileEntry>> {
        crate::sync::SyncService::scan_workspace_for_sync(&self.workspace_path)
    }

    pub fn build_sync_plan_from_workspace(&self) -> crate::error::Result<crate::sync::SyncPlan> {
        crate::sync::SyncService::build_sync_plan_from_workspace(&self.workspace_path)
    }

    pub fn load_sync_state(&self) -> crate::error::Result<crate::sync::SyncState> {
        crate::sync::SyncService::load_sync_state(&self.workspace_path)
    }

    pub fn save_sync_state(&self, state: &crate::sync::SyncState) -> crate::error::Result<()> {
        crate::sync::SyncService::save_sync_state(&self.workspace_path, state)
    }

    pub fn record_sync_conflict(
        &self,
        conflict: crate::sync::SyncConflict,
        local_content: Option<&str>,
    ) -> crate::error::Result<()> {
        crate::sync::SyncService::record_sync_conflict(
            &self.workspace_path,
            conflict,
            local_content,
        )
    }

    pub fn resolve_conflict_keep_local(&self, path: &str) -> crate::error::Result<()> {
        crate::sync::SyncService::resolve_conflict_keep_local(&self.workspace_path, path)
    }

    pub fn resolve_conflict_take_remote(&self, path: &str) -> crate::error::Result<()> {
        crate::sync::SyncService::resolve_conflict_take_remote(&self.workspace_path, path)
    }

    pub fn resolve_conflict_mark_merged(&self, path: &str) -> crate::error::Result<()> {
        crate::sync::SyncService::resolve_conflict_mark_merged(&self.workspace_path, path)
    }

    pub fn get_sync_ignored_paths(&self) -> crate::error::Result<Vec<String>> {
        crate::sync::SyncService::get_sync_ignored_paths(&self.workspace_path)
    }

    pub fn perform_sync_diagnostics(
        &self,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<crate::sync::SyncDiagnosticsResult> {
        let secrets = self.load_sync_secrets().unwrap_or_default();
        let backend_type = crate::sync::resolved_backend_type(config);
        let backend = crate::sync::create_sync_backend(&backend_type);
        backend.diagnose(config, &secrets)
    }

    pub fn perform_sync_dry_run(
        &self,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<crate::sync::SyncPlan> {
        crate::sync::SyncService::perform_sync_dry_run(&self.workspace_path, config)
    }

    pub fn perform_sync(
        &self,
        config: &crate::sync::SyncConfig,
        force_sync: bool,
    ) -> crate::error::Result<crate::sync::SyncResult> {
        let secrets = self.load_sync_secrets().unwrap_or_default();
        let backend_type = crate::sync::resolved_backend_type(config);
        let backend = crate::sync::create_sync_backend(&backend_type);
        backend.sync(&self.workspace_path, config, &secrets, force_sync)
    }

    pub fn load_sync_secrets(&self) -> crate::error::Result<crate::sync::SyncSecrets> {
        let secrets_path = self
            .workspace_path
            .join("app-meta/sync/sync_secrets.local.json");
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
        let secrets_path = self
            .workspace_path
            .join("app-meta/sync/sync_secrets.local.json");
        if let Some(parent) = secrets_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(secrets)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        let parent = secrets_path.parent().unwrap_or_else(|| std::path::Path::new(""));
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
        let config_path = self.workspace_path.join("app-meta/sync/sync_config.json");
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
                android_has_internet_permission: true,
                android_has_access_network_state_permission: true,
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
        let config_path = self.workspace_path.join("app-meta/sync/sync_config.json");
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
