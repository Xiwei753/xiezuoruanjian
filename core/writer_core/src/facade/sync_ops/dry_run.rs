impl crate::facade::WriterCore {
    /// 全量同步诊断 — 只测一次仓库、分支、token。
    ///
    /// `secrets` 由调用方传入（API 层已 snapshot override），不再内部加载。
    pub fn perform_full_sync_diagnostics(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<crate::sync::types::FullSyncDiagnosticsResult> {
        let diagnostics = self.run_sync_diagnostics(config, secrets)?;
        Ok(crate::sync::types::FullSyncDiagnosticsResult { diagnostics })
    }

    ///   全量同步 dry-run — 枚举 App target 所有 Project target pending deleted targets，
    /// 构建每个 target 的计划。
    ///
    /// `secrets` 由调用方传入（API 层已 snapshot override），不再内部加载。
    ///
    ///   调用共享 `build_full_sync_target_plan` 枚举 targets，
    /// dry-run 也包含 pending deleted target（`target_kind="deleted_project"`），
    /// 不再只看 live Project targets。deleted target 的 `SyncPlan` 为空（dry-run 不读远端，
    /// 无法知道远端对象数；调用方据 `target_kind` 判断将删除/恢复）。
    ///
    ///   dry-run 读真实远端 catalog（read-only 网络 IO），
    /// 不再传空 catalog。catalog 读取失败时返回错误（dry-run 是预览，不能返回假的远端事实）。
    pub fn perform_full_sync_dry_run(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<crate::sync::types::FullSyncDryRunResult> {
        //   catalog 读取（网络 IO）由 API 层在
        // core_write() 锁外执行，传入已加载的 catalog。此处只做本地 plan 构建。
        // 保留 fallback：若 API 层未预加载 catalog（旧调用方），在此加载。
        let remote_catalog = if config.enabled {
            self.dry_run_load_remote_catalog(config, secrets)?
        } else {
            crate::sync::types::TargetLifecycleCatalog::default()
        };
        self.perform_full_sync_dry_run_with_catalog(config, &remote_catalog)
    }

    ///   dry-run plan 构建（纯本地 IO，不做网络 IO）。
    ///
    /// `remote_catalog` 由 API 层在 core_write() 锁外预加载后传入。
    pub(crate) fn perform_full_sync_dry_run_with_catalog(
        &self,
        config: &crate::sync::SyncConfig,
        remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    ) -> crate::error::Result<crate::sync::types::FullSyncDryRunResult> {
        use crate::sync::types::{FullSyncDryRunResult, SyncPlan, TargetSyncPlan};

        let projects = self.list_projects()?;

        //   加载 pending deleted targets，让 dry-run 也能看到 deleted target。
        let pending_deleted =
            crate::sync::pending_deleted::load_pending_deleted_targets(&self.app_data_root)?;

        let sync_policy = crate::sync::types::SyncPolicy::from_config(config);

        //   device_id 来自真实 DeviceInfo。
        let device_id = crate::settings::load_device_info(&self.app_data_root)
            .map(|info| info.device_id)
            .unwrap_or_default();

        //   调用共享 planner 枚举 targets，
        // 不复制一套 target 枚举逻辑。
        // 加载 pending_remote_cleanups，
        // 让上一轮 cleanup 失败的远端残留能在本轮重试。
        // 持久化错误向上传递，不再 unwrap_or_default。
        let pending_remote_cleanups =
            crate::sync::pending_remote_cleanup::load_pending_remote_cleanups(&self.app_data_root)?;
        let planned_targets = crate::sync::full_sync::build_full_sync_target_plan(
            &self.app_data_root,
            &self.projects_root,
            &projects,
            &pending_deleted,
            remote_catalog,
            &sync_policy,
            false,
            &device_id,
            &pending_remote_cleanups,
        );

        let mut targets: Vec<TargetSyncPlan> = Vec::new();
        for planned in &planned_targets {
            //   dry-run 用 read-only state loader,
            // build_sync_plan 内部已改用 load_sync_state_read_only.
            let plan = if !config.enabled || planned.is_deleted_target() {
                SyncPlan::new()
            } else {
                crate::sync::SyncService::build_sync_plan(
                    &planned.local_root,
                    planned.target.scope,
                )?
            };
            targets.push(TargetSyncPlan {
                target_kind: planned.target_kind.as_target_kind_str().to_string(),
                project_id: planned.project_id.clone(),
                remote_prefix: planned.target.remote_prefix.clone(),
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

    ///   回退问题：锁外构建 dry-run plan。
    ///
    /// 与 [`perform_full_sync_dry_run_with_catalog`] 的区别：本函数不持任何 Core 锁，
    /// 所有磁盘读取（list_projects / pending / device / planner / scan）在锁外执行。
    /// `app_data_root` / `projects_root` 由调用方在短锁内 snapshot 后传入。
    pub(crate) fn build_full_sync_dry_run_unlocked(
        app_data_root: &std::path::Path,
        projects_root: &std::path::Path,
        config: &crate::sync::SyncConfig,
        remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    ) -> crate::error::Result<crate::sync::types::FullSyncDryRunResult> {
        use crate::sync::types::{FullSyncDryRunResult, SyncPlan, TargetSyncPlan};

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
        let planned_targets = crate::sync::full_sync::build_full_sync_target_plan(
            app_data_root,
            projects_root,
            &projects,
            &pending_deleted,
            remote_catalog,
            &sync_policy,
            false,
            &device_id,
            &pending_remote_cleanups,
        );

        let mut targets: Vec<TargetSyncPlan> = Vec::new();
        for planned in &planned_targets {
            let plan = if !config.enabled || planned.is_deleted_target() {
                SyncPlan::new()
            } else {
                crate::sync::SyncService::build_sync_plan(
                    &planned.local_root,
                    planned.target.scope,
                )?
            };
            targets.push(TargetSyncPlan {
                target_kind: planned.target_kind.as_target_kind_str().to_string(),
                project_id: planned.project_id.clone(),
                remote_prefix: planned.target.remote_prefix.clone(),
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

    ///   dry-run 读真实远端 catalog 的 helper。
    ///
    /// 创建 provider + 读 catalog。任一步骤失败时返回错误
    /// （dry-run 是预览，不能返回假的远端事实）。
    fn dry_run_load_remote_catalog(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<crate::sync::types::TargetLifecycleCatalog> {
        let provider = self.create_sync_provider_for_plan(config, secrets)?;
        let snapshot = crate::sync::target_lifecycle::load_remote_catalog(provider.as_ref())?;
        Ok(snapshot.catalog)
    }
}
