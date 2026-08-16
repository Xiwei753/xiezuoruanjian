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

    /// 旧→新同步 profile 一次性迁移（Issue #630 评论第 4 点 / D）。
    ///
    /// 新全局 profile 已存在时返回 `NotNeeded`；否则依次探测旧应用级 / 旧作品级
    /// profile，多项目一致迁一份，不一致返回 `NeedsReconfigure`。提交成功后清理
    /// 旧凭据；失败/冲突时不删旧凭据。
    pub fn migrate_legacy_sync_profile(
        &self,
    ) -> crate::error::Result<crate::sync::legacy_migration::LegacyMigrationOutcome> {
        let migrator = crate::sync::legacy_migration::LegacySyncProfileMigrator::new(
            &self.app_data_root,
            &self.projects_root,
            self.secure_storage.as_deref(),
        );
        migrator.migrate()
    }

    /// 旧→新同步 profile 一次性迁移，接受精确 generation metadata（Issue #630 评论第 5 点 Part C）。
    ///
    /// 详见 `crate::sync::legacy_migration::LegacySyncProfileMigrator::migrate_with_metadata`。
    /// metadata 中每个项描述一个旧 profile 的 source 和 committed generation，
    /// 使 Core 精确读取 `sync_token_<base>_g<N>` 而不猜测枚举上限。
    pub fn migrate_legacy_sync_profile_with_metadata(
        &self,
        metadata: &[crate::sync::legacy_migration::LegacyProfileMetadata],
    ) -> crate::error::Result<crate::sync::legacy_migration::LegacyMigrationOutcome> {
        let migrator = crate::sync::legacy_migration::LegacySyncProfileMigrator::new(
            &self.app_data_root,
            &self.projects_root,
            self.secure_storage.as_deref(),
        );
        migrator.migrate_with_metadata(metadata)
    }

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

        self.perform_full_sync_with_backend(backend.as_ref(), config, &secrets, force_sync)
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
        let projects = self.list_projects()?;

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

/// 执行单个 target 的同步，把 `Err` 转为该 target 的 `SyncResult::error(...)`。
///
/// `perform_full_sync` 中 App target 和每个 Project target 都通过此 helper 调用，
/// 避免单 target 的 `Err` 用 `?` 提前打断整个全量同步。`Err` 的 `recoverable()`
/// 决定 `SyncStatus::RecoverableError` / `FatalError`，`sync_category()` 决定
/// `error_category`（空字符串视为无分类）。
fn run_full_sync_target(
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

#[cfg(test)]
mod tests {
    use super::run_full_sync_target;
    use crate::sync::types::{
        FirstSyncMode, SyncConfig, SyncDiagnosticsResult, SyncResult, SyncStatus, SyncTarget,
    };
    use crate::sync::{SyncBackend, SyncSecrets};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;
    use tempfile::tempdir;

    /// 可 Clone 的 mock 输出 —— 避免 `Error` 不 `Clone` 的问题。
    ///
    /// `Ok` 变体 `Box<SyncResult>` 以避免 clippy::large_enum_variant（SyncResult
    /// 远大于其他两个变体）。
    #[derive(Clone)]
    enum MockOutcome {
        Ok(Box<SyncResult>),
        ErrOther(String),
        ErrSyncAuth(String),
    }

    impl MockOutcome {
        fn ok(result: SyncResult) -> Self {
            Self::Ok(Box::new(result))
        }
        fn to_result(&self) -> std::result::Result<SyncResult, crate::Error> {
            match self {
                MockOutcome::Ok(r) => Ok((**r).clone()),
                MockOutcome::ErrOther(msg) => Err(crate::Error::Other(msg.clone())),
                MockOutcome::ErrSyncAuth(msg) => Err(crate::Error::SyncAuthFailed {
                    reason: msg.clone(),
                }),
            }
        }
    }

    /// 按 `remote_prefix` 配置每个 target 返回值的 mock backend。
    struct MockBackend {
        behaviors: Mutex<HashMap<String, MockOutcome>>,
        default: MockOutcome,
    }

    impl MockBackend {
        fn new(default: MockOutcome) -> Self {
            Self {
                behaviors: Mutex::new(HashMap::new()),
                default,
            }
        }
        fn set(&self, remote_prefix: &str, outcome: MockOutcome) {
            self.behaviors
                .lock()
                .expect("behaviors mutex poisoned")
                .insert(remote_prefix.to_string(), outcome);
        }
    }

    impl SyncBackend for MockBackend {
        fn diagnose(
            &self,
            _: &SyncConfig,
            _: &SyncSecrets,
        ) -> crate::Result<SyncDiagnosticsResult> {
            Ok(SyncDiagnosticsResult::new())
        }
        fn pull(
            &self,
            _: &Path,
            _: &SyncConfig,
            _: &SyncSecrets,
            _: &SyncTarget,
            _: bool,
        ) -> crate::Result<SyncResult> {
            Ok(SyncResult::success())
        }
        fn push(
            &self,
            _: &Path,
            _: &SyncConfig,
            _: &SyncSecrets,
            _: &SyncTarget,
            _: bool,
        ) -> crate::Result<SyncResult> {
            Ok(SyncResult::success())
        }
        fn sync(
            &self,
            _: &Path,
            _: &SyncConfig,
            _: &SyncSecrets,
            target: &SyncTarget,
            _: bool,
        ) -> crate::Result<SyncResult> {
            let behaviors = self.behaviors.lock().expect("behaviors mutex poisoned");
            match behaviors.get(&target.remote_prefix) {
                Some(outcome) => outcome.to_result(),
                None => self.default.to_result(),
            }
        }
    }

    fn test_config() -> SyncConfig {
        SyncConfig {
            enabled: true,
            backend_type: crate::sync::BackendType::GithubApi,
            remote_url: "https://github.com/test/repo.git".to_string(),
            transport: crate::sync::SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        }
    }

    /// 单个 target 的 Err 不阻止后续 target 执行，所有 target 都出现在结果中。
    #[test]
    fn test_full_sync_single_target_err_does_not_block_others() {
        let temp_dir = tempdir().expect("tempdir");
        let core =
            crate::facade::WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).expect("create projects dir");

        let p1 = core.create_project("Project 1").expect("create project 1");
        let p2 = core.create_project("Project 2").expect("create project 2");

        // App target 返回 Err，两个 Project target 返回 Ok
        let backend = MockBackend::new(MockOutcome::ok(SyncResult::success()));
        backend.set(
            "app",
            MockOutcome::ErrOther("app root IO failed".to_string()),
        );

        let config = test_config();
        let secrets = SyncSecrets::default();
        let result = core
            .perform_full_sync_with_backend(&backend, &config, &secrets, false)
            .expect("single target failure must not make full sync return Err");

        // 3 个 target 都在结果中（1 app + 2 project）
        assert_eq!(result.targets.len(), 3, "all targets should be present");

        // App target 失败（Error::Other recoverable=true → RecoverableError）
        let app_target = &result.targets[0];
        assert_eq!(app_target.target_kind, "app");
        assert!(
            matches!(app_target.result.status, SyncStatus::RecoverableError(_)),
            "app target should be RecoverableError, got {:?}",
            app_target.result.status
        );

        // 两个 Project target 成功
        let project_results: Vec<_> = result
            .targets
            .iter()
            .filter(|t| t.target_kind == "project")
            .collect();
        assert_eq!(
            project_results.len(),
            2,
            "both project targets should succeed"
        );
        for pr in &project_results {
            assert_eq!(pr.result.status, SyncStatus::Success);
        }

        // overall_status 反映有 target 失败
        assert!(
            matches!(result.overall_status, SyncStatus::Error(_)),
            "overall_status should be Error, got {:?}",
            result.overall_status
        );

        // 两个 project 都在结果中
        let project_ids: std::collections::HashSet<_> = project_results
            .iter()
            .map(|t| t.project_id.clone().expect("project target has id"))
            .collect();
        assert!(project_ids.contains(&p1.id), "p1 should be in results");
        assert!(project_ids.contains(&p2.id), "p2 should be in results");
    }

    /// 混合：App Ok, Project1 Err(auth), Project2 Ok —— 全部 target 在结果中，overall Error。
    #[test]
    fn test_full_sync_mixed_outcomes_all_targets_present() {
        let temp_dir = tempdir().expect("tempdir");
        let core =
            crate::facade::WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).expect("create projects dir");

        let p1 = core.create_project("Project 1").expect("create project 1");
        let p2 = core.create_project("Project 2").expect("create project 2");

        let backend = MockBackend::new(MockOutcome::ok(SyncResult::success()));
        // p1 失败（auth），p2 用默认 Ok
        backend.set(
            &format!("projects/{}", p1.id),
            MockOutcome::ErrSyncAuth("token invalid".to_string()),
        );

        let config = test_config();
        let secrets = SyncSecrets::default();
        let result = core
            .perform_full_sync_with_backend(&backend, &config, &secrets, false)
            .expect("mixed outcomes must not make full sync return Err");

        assert_eq!(result.targets.len(), 3, "all targets present");
        // App 成功
        assert_eq!(result.targets[0].result.status, SyncStatus::Success);
        // overall Error（有一个 target 失败）
        assert!(
            matches!(result.overall_status, SyncStatus::Error(_)),
            "overall should be Error"
        );

        // p1 target：SyncAuthFailed recoverable=false → FatalError，category=auth_error
        let p1_target = result
            .targets
            .iter()
            .find(|t| t.project_id.as_deref() == Some(p1.id.as_str()))
            .expect("p1 target present");
        assert!(
            matches!(p1_target.result.status, SyncStatus::FatalError(_)),
            "auth error should be FatalError, got {:?}",
            p1_target.result.status
        );
        assert_eq!(
            p1_target.result.error_category.as_deref(),
            Some("auth_error"),
            "auth error category should be auth_error"
        );

        // p2 成功
        let p2_target = result
            .targets
            .iter()
            .find(|t| t.project_id.as_deref() == Some(p2.id.as_str()))
            .expect("p2 target present");
        assert_eq!(p2_target.result.status, SyncStatus::Success);
    }

    /// 全部 target 成功 → overall Success。
    #[test]
    fn test_full_sync_all_ok_overall_success() {
        let temp_dir = tempdir().expect("tempdir");
        let core =
            crate::facade::WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).expect("create projects dir");

        core.create_project("Project 1").expect("create project 1");

        let backend = MockBackend::new(MockOutcome::ok(SyncResult::success()));
        let config = test_config();
        let secrets = SyncSecrets::default();
        let result = core
            .perform_full_sync_with_backend(&backend, &config, &secrets, false)
            .expect("full sync ok");

        assert_eq!(result.targets.len(), 2);
        assert_eq!(result.overall_status, SyncStatus::Success);
    }

    /// list_projects 失败（projects_root 是文件不是目录）→ 整体 Err。
    #[test]
    fn test_full_sync_list_projects_failure_returns_err() {
        let temp_dir = tempdir().expect("tempdir");
        // 把 projects_root 设为一个文件，让 read_dir 失败
        let projects_path = temp_dir.path().join("projects_file");
        std::fs::write(&projects_path, "not a directory").expect("write file");

        let core = crate::facade::WriterCore::new(temp_dir.path(), &projects_path);
        let backend = MockBackend::new(MockOutcome::ok(SyncResult::success()));
        let config = test_config();
        let secrets = SyncSecrets::default();
        let result = core.perform_full_sync_with_backend(&backend, &config, &secrets, false);
        assert!(
            result.is_err(),
            "list_projects failure should make perform_full_sync return Err"
        );
    }

    /// run_full_sync_target：Err 转为 SyncResult::error，Error::Other → RecoverableError。
    #[test]
    fn test_run_full_sync_target_converts_err_to_error_result() {
        let backend = MockBackend::new(MockOutcome::ErrOther("boom".to_string()));
        let config = test_config();
        let secrets = SyncSecrets::default();
        let target = SyncTarget::app();
        let result = run_full_sync_target(
            &backend,
            Path::new("/tmp/nonexistent"),
            &config,
            &secrets,
            &target,
            false,
        );
        // Error::Other recoverable=true → RecoverableError
        assert!(
            matches!(result.status, SyncStatus::RecoverableError(ref msg) if msg.contains("boom")),
            "expected RecoverableError containing 'boom', got {:?}",
            result.status
        );
        assert!(result.error.is_some(), "error field should be set");
        // Error::Other sync_category() 返回空 → error_category None
        assert!(
            result.error_category.is_none(),
            "Error::Other has no sync_category, expected None"
        );
        assert_eq!(result.first_sync_mode, FirstSyncMode::NotAttempted);
    }

    /// run_full_sync_target：Ok 直接透传。
    #[test]
    fn test_run_full_sync_target_passes_through_ok() {
        let backend = MockBackend::new(MockOutcome::ok(SyncResult::success()));
        let config = test_config();
        let secrets = SyncSecrets::default();
        let target = SyncTarget::app();
        let result = run_full_sync_target(
            &backend,
            Path::new("/tmp/nonexistent"),
            &config,
            &secrets,
            &target,
            false,
        );
        assert_eq!(result.status, SyncStatus::Success);
    }

    /// run_full_sync_target：auth Err → FatalError + auth_error category。
    #[test]
    fn test_run_full_sync_target_auth_err_maps_category() {
        let backend = MockBackend::new(MockOutcome::ErrSyncAuth("bad token".to_string()));
        let config = test_config();
        let secrets = SyncSecrets::default();
        let target = SyncTarget::app();
        let result = run_full_sync_target(
            &backend,
            Path::new("/tmp/nonexistent"),
            &config,
            &secrets,
            &target,
            false,
        );
        assert!(
            matches!(result.status, SyncStatus::FatalError(_)),
            "auth error is not recoverable, expected FatalError"
        );
        assert_eq!(
            result.error_category.as_deref(),
            Some("auth_error"),
            "auth error category should map to auth_error"
        );
    }
}
