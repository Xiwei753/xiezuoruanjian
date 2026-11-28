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

    /// 全量同步 dry-run — 与正式入口共用同一个 target 枚举函数。
    ///
    /// 远端事实只有"哪些作品还有普通对象"；dry-run 不创建、不修改任何远端对象。
    pub fn perform_full_sync_dry_run(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<crate::sync::types::FullSyncDryRunResult> {
        let remote_project_ids = if config.enabled {
            let provider = self.create_sync_provider_for_plan(config, secrets)?;
            crate::sync::full_sync::discovery::discover_remote_project_ids(provider.as_ref())?
        } else {
            Vec::new()
        };
        Self::build_full_sync_dry_run_unlocked(
            &self.app_data_root,
            &self.projects_root,
            config,
            &remote_project_ids,
        )
    }

    /// 锁外构建 dry-run plan：所有磁盘读取（list_projects / pending / scan）在锁外执行。
    /// `app_data_root` / `projects_root` 由调用方在短锁内 snapshot 后传入。
    pub(crate) fn build_full_sync_dry_run_unlocked(
        app_data_root: &std::path::Path,
        projects_root: &std::path::Path,
        config: &crate::sync::SyncConfig,
        remote_project_ids: &[String],
    ) -> crate::error::Result<crate::sync::types::FullSyncDryRunResult> {
        use crate::sync::types::{FullSyncDryRunResult, SyncPlan, TargetSyncPlan};

        let projects = crate::project::list_projects(projects_root)?;
        let pending_deleted =
            crate::sync::pending_deleted::load_pending_deleted_targets(app_data_root)?;
        let planned_targets = crate::sync::full_sync::build_full_sync_target_plan(
            app_data_root,
            projects_root,
            &projects,
            remote_project_ids,
            &pending_deleted,
        )?;

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
}
