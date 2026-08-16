//! 同步状态查询 facade — per-target 本地状态 + 全量同步持久状态（Issue #630）。
//!
//! 分层：
//! - per-target `state.local.json`（`<root>/app-meta/sync/state.local.json`）记录每个
//!   target 自己的 manifest/LWW 状态；
//! - `full_state.local.json`（`<app_data_root>/app-meta/sync/full_state.local.json`）
//!   只记录"这一次全量事务整体是什么结果"，由 [`super::sync_ops`] 在事务开始 /
//!   提前失败 / 聚合完成时写入，本模块只负责读取与原子落盘。
//!
//! 这两层不混用：全量同步的总体状态不允许拿某个 target 的 per-target state 冒充。

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

    // ── 全量同步持久状态（Issue #630 评论 5307423953 Part B） ──
    // 路径：<app_data_root>/app-meta/sync/full_state.local.json
    // 与 per-target state.local.json 分层：full_state 只记录"这一次全量事务整体是什么结果"。

    /// 加载全量同步持久状态。文件不存在或 JSON 损坏时返回 None，不 panic。
    pub fn load_full_sync_state(
        &self,
    ) -> crate::error::Result<Option<crate::sync::full_sync_state::FullSyncState>> {
        let path = self
            .app_data_root
            .join("app-meta/sync/full_state.local.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(crate::Error::Io(e)),
        };
        match serde_json::from_str::<crate::sync::full_sync_state::FullSyncState>(&content) {
            Ok(state) => Ok(Some(state)),
            Err(_) => {
                // 损坏 JSON：记录警告并返回 None，不 panic、不阻断同步。
                log::warn!("full_state.local.json corrupted — returning None");
                Ok(None)
            }
        }
    }

    /// 原子写全量同步持久状态。写临时文件后 rename，保证读端不会看到部分写入。
    pub fn save_full_sync_state(
        &self,
        state: &crate::sync::full_sync_state::FullSyncState,
    ) -> crate::error::Result<()> {
        let path = self
            .app_data_root
            .join("app-meta/sync/full_state.local.json");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(state)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        crate::storage::atomic_write_string(&path, &content)
    }
}
