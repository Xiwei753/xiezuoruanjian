//! 同步状态查询 facade — per-target 本地状态 + 全量同步持久状态。
//!
//! 分层：
//! - Project target 的 `state.local.json` 位于 `<root>/app-meta/sync/`；App v2
//!   则将 manifest/LWW/conflict 状态分别保存在各自的 group 目录中；
//! - `full_state.local.json`（`<app_data_root>/app-meta/sync/full_state.local.json`）
//!   只记录"这一次全量事务整体是什么结果"，由 [`super::sync_ops`] 在事务开始 /
//!   提前失败 / 聚合完成时写入，本模块只负责读取与原子落盘。
//!
//! 这两层不混用：全量同步的总体状态不允许拿某个 target 的 per-target state 冒充。

use std::path::PathBuf;

impl super::WriterCore {
    /// 根据 target kind 解析冲突操作的 sync root。
    ///
    /// 与 `sync/full_sync/plan.rs` 中 `SyncTarget` 的 `target_live_root` 一致：
    /// - `"app"` → `app_data_root`（星图等应用级数据，`project_id` 不使用）；
    /// - `"project"` → `projects_root/project_id`（作品正文，`project_id` 必填）。
    ///
    /// 统一入口避免冲突 API 把锁根 / sync root 硬编码到 `projects_root/project_id`，
    /// 让 App target 的星图冲突也能从平台接口正常到达（Issue #902 第十九轮问题1）。
    fn conflict_target_root(
        &self,
        target_kind: &str,
        project_id: Option<&str>,
    ) -> crate::error::Result<PathBuf> {
        match target_kind {
            "app" => Ok(self.app_data_root.clone()),
            "project" => {
                let pid = project_id.ok_or_else(|| {
                    crate::Error::Other("conflict target 'project' requires project_id".into())
                })?;
                Ok(self.project_root(pid))
            }
            _ => Err(crate::Error::Other(format!(
                "unknown conflict target kind: {}",
                target_kind
            ))),
        }
    }

    fn app_conflict_groups(&self) -> crate::error::Result<Vec<crate::sync::types::AppDataGroup>> {
        crate::sync::app_data_group_catalog::discover_local_app_data_groups(&self.app_data_root)
    }

    fn app_conflict_group_for_path(
        &self,
        path: &str,
    ) -> crate::error::Result<crate::sync::types::AppDataGroup> {
        let mut owners = self
            .app_conflict_groups()?
            .into_iter()
            .filter(|group| group.owns_path(path));
        let group = owners.next().ok_or_else(|| {
            crate::Error::Other(format!(
                "App conflict path '{}' does not belong to a known data group",
                path
            ))
        })?;
        if owners.next().is_some() {
            return Err(crate::Error::Other(format!(
                "App conflict path '{}' belongs to multiple data groups",
                path
            )));
        }
        Ok(group)
    }

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

    /// 冲突解决：保留本地版本。
    ///
    /// `target_kind` 标识冲突所属的同步 target（`"app"` / `"project"`），
    /// `project_id` 在 `"project"` target 下必填，`"app"` target 下不使用。
    pub fn resolve_conflict_keep_local(
        &self,
        target_kind: &str,
        project_id: Option<&str>,
        path: &str,
    ) -> crate::error::Result<()> {
        let root = self.conflict_target_root(target_kind, project_id)?;
        if target_kind == "app" {
            let group = self.app_conflict_group_for_path(path)?;
            crate::sync::SyncService::resolve_conflict_keep_local_for_group(&root, path, &group)
        } else {
            crate::sync::SyncService::resolve_conflict_keep_local(&root, path)
        }
    }

    /// 冲突解决：采用远端版本。
    ///
    /// `target_kind` 标识冲突所属的同步 target（`"app"` / `"project"`），
    /// `project_id` 在 `"project"` target 下必填，`"app"` target 下不使用。
    pub fn resolve_conflict_take_remote(
        &self,
        target_kind: &str,
        project_id: Option<&str>,
        path: &str,
    ) -> crate::error::Result<bool> {
        let root = self.conflict_target_root(target_kind, project_id)?;
        if target_kind == "app" {
            let group = self.app_conflict_group_for_path(path)?;
            crate::sync::SyncService::resolve_conflict_take_remote_for_group(&root, path, &group)
        } else {
            crate::sync::SyncService::resolve_conflict_take_remote(&root, path)
        }
    }

    /// 冲突解决：标记为已合并。
    ///
    /// `target_kind` 标识冲突所属的同步 target（`"app"` / `"project"`），
    /// `project_id` 在 `"project"` target 下必填，`"app"` target 下不使用。
    pub fn resolve_conflict_mark_merged(
        &self,
        target_kind: &str,
        project_id: Option<&str>,
        path: &str,
    ) -> crate::error::Result<()> {
        let root = self.conflict_target_root(target_kind, project_id)?;
        if target_kind == "app" {
            let group = self.app_conflict_group_for_path(path)?;
            crate::sync::SyncService::resolve_conflict_mark_merged_for_group(&root, path, &group)
        } else {
            crate::sync::SyncService::resolve_conflict_mark_merged(&root, path)
        }
    }

    /// 加载冲突预览 — 返回本地/远端内容供平台层展示。
    ///
    /// `target_kind` 标识冲突所属的同步 target（`"app"` / `"project"`），
    /// `project_id` 在 `"project"` target 下必填，`"app"` target 下不使用。
    pub fn load_sync_conflict_preview(
        &self,
        target_kind: &str,
        project_id: Option<&str>,
        path: &str,
    ) -> crate::error::Result<crate::sync::types::SyncConflictPreview> {
        let root = self.conflict_target_root(target_kind, project_id)?;
        if target_kind == "app" {
            let group = self.app_conflict_group_for_path(path)?;
            crate::sync::SyncService::load_conflict_preview_for_group(&root, path, &group)
        } else {
            crate::sync::SyncService::load_conflict_preview(&root, path)
        }
    }

    /// 列出指定 target 的所有冲突记录。
    ///
    /// `target_kind` 标识同步 target（`"app"` / `"project"`），
    /// `project_id` 在 `"project"` target 下必填，`"app"` target 下不使用。
    pub fn list_sync_conflicts(
        &self,
        target_kind: &str,
        project_id: Option<&str>,
    ) -> crate::error::Result<Vec<crate::sync::SyncConflict>> {
        let root = self.conflict_target_root(target_kind, project_id)?;
        if target_kind == "app" {
            let mut conflicts = Vec::new();
            for group in self.app_conflict_groups()? {
                conflicts.extend(crate::sync::SyncService::list_conflicts_for_group(
                    &root, &group,
                )?);
            }
            Ok(conflicts)
        } else {
            crate::sync::SyncService::list_conflicts(&root)
        }
    }

    /// 列出所有 target 的所有冲突（聚合），只返回摘要条目，不含正文/快照。
    ///
    /// 诊断包用（Issue #763）。覆盖 App target（星图等应用级数据）和所有 Project
    /// target，单 target 读取失败不阻断全局聚合，跳过该 target 继续。
    /// 每个条目带 `target_kind` 标识来源（`"app"` / `"project"`），平台层据此
    /// 路由到对应 target 的冲突解决接口（Issue #902 第十九轮问题1）。
    pub fn list_all_sync_conflicts(
        &self,
    ) -> crate::error::Result<Vec<crate::api::AllSyncConflictEntryDto>> {
        let mut entries = Vec::new();

        // App target（星图等应用级数据）。读取失败不阻断全局聚合。
        match self.list_sync_conflicts("app", None) {
            Ok(conflicts) => {
                for c in conflicts {
                    let kind = match c.kind {
                        crate::sync::types::SyncConflictKind::BothChanged => "both_changed",
                        crate::sync::types::SyncConflictKind::RemoteDeleted => "remote_deleted",
                    };
                    entries.push(crate::api::AllSyncConflictEntryDto {
                        project_id: String::new(),
                        project_title: String::new(),
                        path: c.local_path,
                        kind: kind.to_string(),
                        created_at: c.created_at,
                        target_kind: "app".to_string(),
                    });
                }
            }
            Err(e) => {
                log::warn!("list_all_sync_conflicts: skip app target — {e}");
            }
        }

        // Project targets。
        let projects = self.list_projects()?;
        for project in &projects {
            let conflicts = match self.list_sync_conflicts("project", Some(&project.id)) {
                Ok(c) => c,
                Err(e) => {
                    // 单项目失败不阻断全局聚合，记录警告后继续下一项目。
                    log::warn!(
                        "list_all_sync_conflicts: skip project {} ({}) — {e}",
                        project.id,
                        project.title
                    );
                    continue;
                }
            };
            for c in conflicts {
                let kind = match c.kind {
                    crate::sync::types::SyncConflictKind::BothChanged => "both_changed",
                    crate::sync::types::SyncConflictKind::RemoteDeleted => "remote_deleted",
                };
                entries.push(crate::api::AllSyncConflictEntryDto {
                    project_id: project.id.clone(),
                    project_title: project.title.clone(),
                    path: c.local_path,
                    kind: kind.to_string(),
                    created_at: c.created_at,
                    target_kind: "project".to_string(),
                });
            }
        }
        Ok(entries)
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

    // ── 全量同步持久状态 ──
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

    /// 加载最近一次完整同步报告。报告损坏时返回 None，不阻断同步恢复。
    pub fn load_latest_full_sync_report(
        &self,
    ) -> crate::error::Result<Option<crate::sync::full_sync_state::FullSyncReport>> {
        let path = self
            .app_data_root
            .join("app-meta/sync/last_report.local.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(crate::Error::Io(e)),
        };
        match serde_json::from_str::<crate::sync::full_sync_state::FullSyncReport>(&content) {
            Ok(report) => Ok(Some(report)),
            Err(_) => {
                log::warn!("last_report.local.json corrupted — returning None");
                Ok(None)
            }
        }
    }

    /// 原子保存最近一次全量同步报告。
    pub fn save_latest_full_sync_report(
        &self,
        report: &crate::sync::full_sync_state::FullSyncReport,
    ) -> crate::error::Result<()> {
        let path = self
            .app_data_root
            .join("app-meta/sync/last_report.local.json");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(report)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        crate::storage::atomic_write_string(&path, &content)
    }

    /// 从本轮 target 结果生成并原子保存完整同步报告。
    pub fn record_latest_full_sync_report(
        &self,
        result: &crate::sync::types::FullSyncResult,
        now_epoch_seconds: i64,
    ) -> crate::error::Result<()> {
        let report = crate::sync::full_sync_state::FullSyncReport::from_result(
            result,
            now_epoch_seconds,
            &self.app_data_root,
            &self.projects_root,
        );
        self.save_latest_full_sync_report(&report)
    }

    /// 为全局初始化或平台预处理失败保留可诊断报告。
    pub fn record_full_sync_early_failure_report(
        &self,
        status: &crate::sync::SyncStatus,
        failed_target: &str,
        now_epoch_seconds: i64,
    ) -> crate::error::Result<()> {
        let report = crate::sync::full_sync_state::FullSyncReport::early_failure(
            status,
            failed_target,
            now_epoch_seconds,
            &self.app_data_root,
            &self.projects_root,
        );
        self.save_latest_full_sync_report(&report)
    }

    /// 保留 Core 已确定的错误分类与失败阶段，不按 Recoverable/Fatal 猜测。
    pub fn record_full_sync_early_failure_report_typed(
        &self,
        status: &crate::sync::SyncStatus,
        failed_target: &str,
        error_category: &str,
        phase: &str,
        now_epoch_seconds: i64,
    ) -> crate::error::Result<()> {
        let report = crate::sync::full_sync_state::FullSyncReport::early_failure_typed(
            status,
            failed_target,
            error_category,
            phase,
            now_epoch_seconds,
            &self.app_data_root,
            &self.projects_root,
        );
        self.save_latest_full_sync_report(&report)
    }

    /// 冷启动恢复中断的 `Syncing` 状态。
    ///
    /// 读取 `full_state.local.json`，只有旧状态是 `Syncing` 才原子改成
    /// `RecoverableError("previous_full_sync_interrupted")` + `failed_targets=["global"]`，
    /// 保留旧 `last_attempt_time` / `last_success_time`。其它终态（Success /
    /// FatalError / RecoverableError / None 等）不动。
    ///
    /// 该动作只能在新 Core/WriterAppService 实例启动时执行一次，不能塞进
    /// `load_full_sync_state()`：后者是纯读探针，每次状态查询都会调用，若在其中
    /// 落盘会反复改写磁盘并掩盖真正的事务进度。
    ///
    /// 返回值：是否实际发生了恢复落盘（true 表示旧状态是 Syncing 并已原子改写）。
    pub fn recover_interrupted_full_sync_state(&self) -> crate::error::Result<bool> {
        let previous = self.load_full_sync_state()?;
        let recovered =
            crate::sync::full_sync_state::FullSyncState::recover_interrupted(previous.as_ref());
        let Some(new_state) = recovered else {
            return Ok(false);
        };
        self.save_full_sync_state(&new_state)?;
        Ok(true)
    }
}
