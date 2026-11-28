//! 同步 facade — 全量同步统一入口。
//!
//! 一个全局 `SyncConfig` + 一份全局凭据，`perform_full_sync` 内部按 `SyncTarget`
//! 把不同本地根映射到同一个远端仓库的不同前缀：
//! - App target：`<app_data_root>` → `app/`
//! - Project target：`<project_root>` → `projects/<project_id>/`
//!
//! 三段式：Prepare（短写锁内规划）→ Transfer（无锁下载远端对象变化）→
//! Commit（持锁统一裁决 + 一次 SaveTransaction）+ 一次远端发布。

pub(crate) mod commit;
mod dry_run;
mod prepare;

impl crate::facade::WriterCore {
    /// 同步诊断 — 只测一次仓库、分支、token。
    fn run_sync_diagnostics(
        &self,
        config: &crate::sync::SyncConfig,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<crate::sync::SyncDiagnosticsResult> {
        let provider = self.create_sync_provider_inner(config, secrets)?;
        provider.diagnose().map_err(crate::Error::from)
    }

    fn remove_pending_remote_cleanup_by_prefix(&self, remote_prefix: &str) {
        if let Err(error) = crate::sync::pending_remote_cleanup::remove_pending_remote_cleanup(
            &self.app_data_root,
            remote_prefix,
        ) {
            log::warn!(
                "finalize_full_sync: remove_pending_remote_cleanup failed for {remote_prefix}: \
                 {error} — pending will be retried next sync"
            );
        }
    }

    fn remove_pending_deleted_by_prefix(&self, remote_prefix: &str) {
        let pending = match crate::sync::pending_deleted::load_pending_deleted_targets(
            &self.app_data_root,
        ) {
            Ok(pending) => pending,
            Err(error) => {
                log::warn!(
                    "finalize_full_sync: load_pending_deleted_targets failed: {error} — \
                     pending will be retried next sync"
                );
                return;
            }
        };
        let Some(matched) = pending
            .iter()
            .find(|entry| entry.target.remote_prefix == remote_prefix)
        else {
            return;
        };
        if let Err(error) = crate::sync::pending_deleted::remove_pending_deleted_target(
            &self.app_data_root,
            &matched.journal_token,
        ) {
            log::warn!(
                "finalize_full_sync: remove_pending_deleted_target failed for {remote_prefix}: \
                 {error} — pending will be retried next sync"
            );
        }
    }
}
