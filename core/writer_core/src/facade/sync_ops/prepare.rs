impl crate::facade::WriterCore {
    /// 锁外构建 full sync plan。
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
        remote_project_ids: &[String],
    ) -> crate::error::Result<crate::sync::full_sync::FullSyncPlan> {
        use crate::sync::full_sync::FullSyncPlan;

        let projects = crate::project::list_projects(projects_root)?;

        let pending_deleted =
            crate::sync::pending_deleted::load_pending_deleted_targets(app_data_root)?;

        let sync_policy = crate::sync::types::SyncPolicy::from_config(config);

        let targets = crate::sync::full_sync::build_full_sync_target_plan(
            app_data_root,
            projects_root,
            &projects,
            remote_project_ids,
            &pending_deleted,
        )?;

        Ok(FullSyncPlan {
            sync_policy,
            force_sync,
            targets,
            app_data_root: app_data_root.to_path_buf(),
        })
    }

    /// 三段式全量同步 — 创建 provider（Prepare 阶段、写锁内）。
    ///
    /// transport 初始化失败时返回 Err（已持久化失败状态）。
    /// 根据 `config.active_provider` 选择对应的 Provider 实现。
    ///
    /// 这是**用户同步路径**的 provider factory：transport 初始化失败会通过
    /// `persist_full_sync_early_failure` 把失败状态持久化到磁盘
    /// `FullSyncState`（`"preflight"` 标记），符合用户同步语义。
    ///
    /// maintenance / GC 等不应触碰用户同步终态的路径应改用
    /// [`create_sync_provider_for_maintenance`]。
    pub fn create_sync_provider_for_plan(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<Box<dyn crate::sync::provider::SyncProvider>> {
        self.create_sync_provider_inner(config, secrets)
    }

    /// provider 构造内部实现。
    ///
    /// transport 初始化失败时调 `persist_full_sync_early_failure(status, "preflight")`
    /// 持久化失败状态。
    pub(crate) fn create_sync_provider_inner(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<Box<dyn crate::sync::provider::SyncProvider>> {
        let create_github_api = |provider_config: &crate::sync::provider::ProviderConfig| {
            #[cfg(feature = "github-api")]
            {
                let transport =
                    self.init_sync_transport_with_persist()?;
                let crate::sync::provider::ProviderConfig::GitHub(github_config) = provider_config
                else {
                    return Err(crate::Error::Other(
                        "GitHub API fallback requires github provider config".into(),
                    ));
                };
                let runtime =
                    crate::sync::provider::github::config::GitHubRuntimeConfig::from_persisted(
                        github_config,
                        secrets.provider_secrets.as_ref(),
                    )
                    .map_err(crate::Error::from)?;
                Ok(Box::new(crate::sync::provider::github::GitHubProvider::new(
                    runtime, transport,
                ))
                    as Box<dyn crate::sync::provider::SyncProvider>)
            }
            #[cfg(not(feature = "github-api"))]
            {
                let _ = provider_config;
                Err(crate::Error::NotImplemented)
            }
        };

        crate::sync::provider::router::create_provider(
            config,
            secrets,
            &self.app_data_root,
            create_github_api,
        )
    }

    /// 锁外创建 provider。
    ///
    /// 调用方在短读锁内 clone `app_data_root` 与 `sync_transport` 后释放锁，再调用
    /// 本函数；transport 初始化、Git 探测和 GitHub API 构造都不会占用 Core 锁。
    pub(crate) fn create_sync_provider_unlocked(
        app_data_root: &std::path::Path,
        sync_transport: Option<&writer_platform_api::SyncTransportFactory>,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<Box<dyn crate::sync::provider::SyncProvider>> {
        #[cfg(not(feature = "github-api"))]
        let _ = sync_transport;
        let create_github_api = |provider_config: &crate::sync::provider::ProviderConfig| {
            #[cfg(feature = "github-api")]
            {
                let transport = Self::init_sync_transport_from_factory(sync_transport)?;
                let crate::sync::provider::ProviderConfig::GitHub(github_config) = provider_config
                else {
                    return Err(crate::Error::Other(
                        "GitHub API fallback requires github provider config".into(),
                    ));
                };
                let runtime =
                    crate::sync::provider::github::config::GitHubRuntimeConfig::from_persisted(
                        github_config,
                        secrets.provider_secrets.as_ref(),
                    )
                    .map_err(crate::Error::from)?;
                Ok(Box::new(crate::sync::provider::github::GitHubProvider::new(
                    runtime, transport,
                ))
                    as Box<dyn crate::sync::provider::SyncProvider>)
            }
            #[cfg(not(feature = "github-api"))]
            {
                let _ = provider_config;
                Err(crate::Error::NotImplemented)
            }
        };

        crate::sync::provider::router::create_provider(
            config,
            secrets,
            app_data_root,
            create_github_api,
        )
    }

    /// 初始化 sync transport；失败时持久化 preflight 失败状态（用户同步路径）。
    #[cfg(feature = "github-api")]
    fn init_sync_transport_with_persist(
        &self,
    ) -> crate::error::Result<std::sync::Arc<dyn writer_platform_api::SyncTransport>> {
        self.init_sync_transport().inspect_err(|err| {
            let status = crate::sync::full_sync::error_to_persist_status(err);
            let category = {
                let category = err.sync_category();
                if category.is_empty() {
                    "other"
                } else {
                    category
                }
            };
            self.persist_full_sync_early_failure_typed(
                status,
                "github_api_fallback_init",
                category,
                "github_api_fallback_init",
                &err.to_string(),
            );
        })
    }
}
