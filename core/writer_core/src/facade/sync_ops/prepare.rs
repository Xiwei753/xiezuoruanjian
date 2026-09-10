impl crate::facade::WriterCore {
    /// 三段式全量同步 — Prepare 阶段（短写锁内调用）。
    ///
    /// 写 `Syncing` 状态、枚举 targets、算出每个 target 的 `local_root`，
    /// 产出 [`crate::sync::full_sync::FullSyncPlan`]（owned，不依赖 core）。
    ///
    /// **不在**写锁内创建或 seed `StagingRun`。
    /// seed 涉及磁盘扫描/复制，会把"短写锁"变成"磁盘长锁"，阻塞冷启动卷章读取。
    /// staging 的创建和 seed 积到 `prepare_staging_runs`，在无锁状态下执行。
    ///
    /// `secrets` 由调用方传入（API 层已 snapshot override），不再内部加载。
    ///
    ///   `remote_catalog` 由调用方传入（在创建 provider 后
    /// 读取），planner 真正使用它做 target-level LWW 决策。`device_id` 来自真实
    /// `DeviceInfo.device_id`。
    ///
    /// transport 初始化失败时返回 Err（已持久化失败状态）。
    /// `list_projects` 失败时返回 Err（已持久化失败状态）。
    pub fn prepare_full_sync(
        &self,
        config: &crate::sync::SyncConfig,
        force_sync: bool,
        _secrets: crate::sync::SyncSecrets,
        remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
        remote_catalog_snapshot: crate::sync::types::RemoteTargetCatalogSnapshot,
    ) -> crate::error::Result<crate::sync::full_sync::FullSyncPlan> {
        use crate::sync::full_sync::FullSyncPlan;

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

        //   加载 pending deleted targets，
        // 让 prepare_full_sync 为已删除作品生成 target，run_transfer 走
        // target-delete 计划清理远端 projects/<id>/ 下所有对象。
        let pending_deleted =
            match crate::sync::pending_deleted::load_pending_deleted_targets(&self.app_data_root) {
                Ok(targets) => targets,
                Err(err) => {
                    let msg = err.to_string();
                    self.persist_full_sync_early_failure(
                        crate::sync::SyncStatus::RecoverableError(msg),
                        "global",
                    );
                    return Err(err);
                }
            };

        let sync_policy = crate::sync::types::SyncPolicy::from_config(config);

        //   device_id 来自真实 DeviceInfo。
        let device_id = crate::settings::load_device_info(&self.app_data_root)
            .map(|info| info.device_id)
            .unwrap_or_default();

        //   调用共享 planner，传入真实 remote_catalog
        // 做 target-level LWW 决策。
        // 加载 pending_remote_cleanups。
        // 持久化错误向上传递，不再 unwrap_or_default。
        let pending_remote_cleanups =
            crate::sync::pending_remote_cleanup::load_pending_remote_cleanups(&self.app_data_root)?;
        let targets = crate::sync::full_sync::build_full_sync_target_plan(
            &self.app_data_root,
            &self.projects_root,
            &projects,
            &pending_deleted,
            remote_catalog,
            &sync_policy,
            force_sync,
            &device_id,
            &pending_remote_cleanups,
        );

        // 不再携带 workspace_git_layout。
        // 本地 Git 仓库由 bootstrap 阶段初始化，同步计划不负责 Git 生命周期。

        Ok(FullSyncPlan {
            sync_policy,
            force_sync,
            targets,
            app_data_root: self.app_data_root.clone(),
            remote_catalog_snapshot,
        })
    }

    ///   回退问题：锁外构建 full sync plan。
    ///
    /// 与 [`prepare_full_sync`] 的区别：本函数不调 `persist_full_sync_started`（调用方
    /// 已在短锁内完成），不持任何 Core 锁，所有磁盘读取（list_projects / pending /
    /// device / planner / scan）在锁外执行，避免阻塞正文/作品读取。
    ///
    /// `app_data_root` / `projects_root` 由调用方在短锁内 snapshot 后传入。
    pub(crate) fn build_full_sync_plan_unlocked(
        app_data_root: &std::path::Path,
        projects_root: &std::path::Path,
        config: &crate::sync::SyncConfig,
        force_sync: bool,
        remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
        remote_catalog_snapshot: crate::sync::types::RemoteTargetCatalogSnapshot,
    ) -> crate::error::Result<crate::sync::full_sync::FullSyncPlan> {
        use crate::sync::full_sync::FullSyncPlan;

        let projects = crate::project::list_projects(projects_root)?;

        let pending_deleted =
            crate::sync::pending_deleted::load_pending_deleted_targets(app_data_root)?;

        let sync_policy = crate::sync::types::SyncPolicy::from_config(config);

        let device_id = crate::settings::load_device_info(app_data_root)
            .map(|info| info.device_id)
            .unwrap_or_default();

        // 加载 pending_remote_cleanups。
        // 持久化错误向上传递，不再 unwrap_or_default。
        let pending_remote_cleanups =
            crate::sync::pending_remote_cleanup::load_pending_remote_cleanups(app_data_root)?;
        let targets = crate::sync::full_sync::build_full_sync_target_plan(
            app_data_root,
            projects_root,
            &projects,
            &pending_deleted,
            remote_catalog,
            &sync_policy,
            force_sync,
            &device_id,
            &pending_remote_cleanups,
        );

        Ok(FullSyncPlan {
            sync_policy,
            force_sync,
            targets,
            app_data_root: app_data_root.to_path_buf(),
            remote_catalog_snapshot,
        })
    }

    /// 三段式全量同步 — 创建 provider（Prepare 阶段、写锁内）。
    ///
    /// transport 初始化失败时返回 Err（已持久化失败状态）。
    /// 根据 `config.active_provider` 选择对应的 Provider 实现。
    pub fn create_sync_provider_for_plan(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<Box<dyn crate::sync::provider::SyncProvider>> {
        // secrets 仅在 github-api feature 下使用；非 github-api 时消费以避免 unused。
        #[cfg(not(feature = "github-api"))]
        let _ = secrets;
        match config.active_provider.as_str() {
            #[cfg(feature = "github-api")]
            "github_api" => {
                let transport = self.init_sync_transport().inspect_err(|err| {
                    let status = crate::sync::full_sync::error_to_persist_status(err);
                    self.persist_full_sync_early_failure(status, "preflight");
                })?;
                let github_config = config
                    .provider_config
                    .as_ref()
                    .map(|pc| match pc {
                        crate::sync::provider::ProviderConfig::GitHub(c) => c,
                    })
                    .ok_or_else(|| crate::Error::Other("missing github provider config".into()))?;
                let runtime =
                    crate::sync::provider::github::config::GitHubRuntimeConfig::from_persisted(
                        github_config,
                        secrets.provider_secrets.as_ref(),
                    )
                    .map_err(crate::Error::from)?;
                Ok(Box::new(
                    crate::sync::provider::github::GitHubProvider::new(runtime, transport),
                ))
            }
            #[cfg(not(feature = "github-api"))]
            "github_api" => Err(crate::Error::NotImplemented),
            _ => Err(crate::Error::NotImplemented),
        }
    }
}
