//! full sync 的 staging run — 只承接远端变化对象与一次提交（Issue #1066）。
//!
//! 新的三段式同步里 staging 不再是 live 的整目录克隆：
//!
//! - **Prepare**：创建隔离 run 目录（`base/`、`staging/`），不复制 live；
//! - **Transfer**：只把远端索引里与本机基线/内容不同的 blob 下载到 `staging/`；
//! - **Commit**：`compute_commit_plan` 做唯一一次对象比较（共同基线 = 持久
//!   对象索引里的已确认版本），再用同一个 `SaveTransaction` 写回内容、对象
//!   索引与已确认基线。
//!
//! 本模块不持 Core 锁、不做网络、不写 live 文件（commit 由调用方落盘）。

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;
use uuid::Uuid;

use super::commit_plan::*;

const BASE_SUBDIR: &str = "base";
const STAGING_SUBDIR: &str = "staging";

/// 一次 full sync 的隔离 staging run。
///
/// `run_root` = `<parent>/full-sync-staging/<run_id>`，下含 `base/`（章节事务
/// 需要的历史基线视图，可为空）与 `staging/`（本轮下载的远端变化对象）。
/// run 结束调 [`StagingRun::cleanup`] 整体删除。
pub struct StagingRun {
    run_root: PathBuf,
    run_id: String,
    /// 应用数据根。提交阶段与 MCP 写事务共用这把跨进程工作区锁；
    /// 即使 target_live_root 是作品目录，锁也必须落在应用数据根。
    app_data_root: PathBuf,
    /// Commit 阶段要写回的 live root。
    target_live_root: PathBuf,
    /// 该 run 对应的 App 数据组；`None` 表示作品 target。
    app_group: Option<crate::sync::types::AppDataGroup>,
    /// 同步范围，决定本地扫描/白名单口径。
    scope: crate::sync::types::SyncScope,
    /// 本轮提交决议里需要上传 / 传播删除的本地对象（提交阶段取走）。
    publish_lists: std::sync::Mutex<(Vec<PathBuf>, Vec<PathBuf>)>,
    /// Prepare 阶段在传输之前记录的目标级失败（例如本机统计事件库迁移失败）。
    prepare_error: Option<String>,
}

impl StagingRun {
    /// 创建隔离 run 目录。`parent` 通常在 app-data 下（不进 live project root），
    /// 避免被 scanner 当成作品文件。
    pub fn create(parent: &Path, target_live_root: PathBuf) -> Result<Self> {
        let run_id = Uuid::new_v4().to_string();
        let run_root = parent.join("full-sync-staging").join(&run_id);
        fs::create_dir_all(run_root.join(BASE_SUBDIR))?;
        fs::create_dir_all(run_root.join(STAGING_SUBDIR))?;
        Ok(Self {
            run_root,
            run_id,
            app_data_root: parent.to_path_buf(),
            target_live_root,
            app_group: None,
            scope: crate::sync::types::SyncScope::Project,
            publish_lists: std::sync::Mutex::new((Vec::new(), Vec::new())),
            prepare_error: None,
        })
    }

    /// 创建 App 数据组的 staging run。
    pub(crate) fn create_for_app_group(
        parent: &Path,
        target_live_root: PathBuf,
        app_group: crate::sync::types::AppDataGroup,
    ) -> Result<Self> {
        let mut run = Self::create(parent, target_live_root)?;
        run.app_group = Some(app_group);
        run.scope = crate::sync::types::SyncScope::App;
        Ok(run)
    }

    pub fn run_root(&self) -> &Path {
        &self.run_root
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Commit 阶段要写回的 live root。
    pub fn target_live_root(&self) -> &Path {
        &self.target_live_root
    }

    /// 应用数据根；同步提交与 MCP 写事务共享的工作区锁在这里。
    pub fn app_data_root(&self) -> &Path {
        &self.app_data_root
    }

    /// 该 run 对应的 App 数据组。
    pub fn app_group(&self) -> Option<&crate::sync::types::AppDataGroup> {
        self.app_group.as_ref()
    }

    /// 记录该 run 的同步范围。
    pub(crate) fn set_scope(&mut self, scope: crate::sync::types::SyncScope) {
        self.scope = scope;
    }

    /// 该 run 的同步范围。
    pub(crate) fn scope(&self) -> crate::sync::types::SyncScope {
        self.scope
    }

    /// 记录目标级失败原因。
    pub fn record_prepare_error(&mut self, message: String) {
        self.prepare_error = Some(message);
    }

    /// 目标级失败原因。
    pub fn prepare_error(&self) -> Option<&str> {
        self.prepare_error.as_deref()
    }

    /// staging 子目录（Transfer 阶段写入远端变化对象）。
    pub fn staging_root(&self) -> PathBuf {
        self.run_root.join(STAGING_SUBDIR)
    }

    /// base 子目录（章节事务的历史基线视图；不再复制 live 整棵树）。
    pub fn base_root(&self) -> PathBuf {
        self.run_root.join(BASE_SUBDIR)
    }

    /// 记录本轮提交决议中的上传/删除清单。
    pub(crate) fn record_publish_lists(
        &self,
        uploads: Vec<PathBuf>,
        publish_deletes: Vec<PathBuf>,
    ) {
        if let Ok(mut lists) = self.publish_lists.lock() {
            *lists = (uploads, publish_deletes);
        }
    }

    /// 取走本轮提交决议中的上传/删除清单（只取一次）。
    pub(crate) fn take_publish_lists(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        self.publish_lists
            .lock()
            .map(|mut lists| std::mem::take(&mut *lists))
            .unwrap_or_default()
    }

    /// 统一对象比较入口（Issue #1066）：见 [`super::plan::compute`]。
    pub fn compute_commit_plan(&self, live_root: &Path) -> Result<CommitPlan> {
        super::plan::compute(self, live_root)
    }

    /// 清理整个 run 目录。
    pub fn cleanup(&self) {
        let _ = fs::remove_dir_all(&self.run_root);
    }
}

/// 创建本轮所有 staging run（无锁状态下调用）。
///
/// 不再 seed live、不再建 base 快照：staging 只承接远端变化的 blob，共同基线
/// 来自持久对象索引（`state.local.json` 的 `known_files`）。
pub fn prepare_staging_runs(
    plan: &mut crate::sync::full_sync::FullSyncPlan,
) -> crate::error::Result<Vec<StagingRun>> {
    let mut staging_runs: Vec<StagingRun> = Vec::new();

    for planned in &mut plan.targets {
        let mut run = match &planned.target.app_group {
            Some(group) => StagingRun::create_for_app_group(
                &plan.app_data_root,
                planned.target_live_root.clone(),
                group.clone(),
            )?,
            None => StagingRun::create(&plan.app_data_root, planned.target_live_root.clone())?,
        };
        run.set_scope(planned.target.scope);
        // #955：本机统计设备组必须在传输之前完成旧事件库迁移（写 live）。
        // 迁移失败只记录到该 run：本组本次跳过 Transfer/Commit，其它组继续同步。
        if let Some(error) = migrate_local_stats_group_before_seed(planned) {
            run.record_prepare_error(error);
        }
        planned.staging_root = Some(run.staging_root());
        staging_runs.push(run);
    }

    Ok(staging_runs)
}

/// 本机统计设备组的旧事件库迁移（#955）。
///
/// 对 `AppDataGroup::StatsDevice(device_id)` 且 `device_id` 等于 live 根
/// `device/current_device.json` 中设备 ID 的 target，在传输之前调用幂等迁移，
/// 把 `app-meta/stats/events.local/` 写入 live 的 `stats/events/<device_id>/`。
///
/// 返回 `Some(error)` 表示迁移失败；调用方只让该组失败，不扩大为整轮同步失败。
fn migrate_local_stats_group_before_seed(
    planned: &crate::sync::full_sync::PlannedTarget,
) -> Option<String> {
    let crate::sync::types::AppDataGroup::StatsDevice(device_id) =
        planned.target.app_group.as_ref()?
    else {
        return None;
    };
    let local_device_id = crate::settings::load_device_info(&planned.target_live_root)
        .ok()
        .map(|info| info.device_id)
        .filter(|id| !id.is_empty());
    if local_device_id.as_deref() != Some(device_id.as_str()) {
        return None;
    }
    match crate::writing_stats::migration::migrate_stats_event_store(&planned.target_live_root) {
        Ok(()) => None,
        Err(error) => Some(format!("migration: {error}")),
    }
}

/// 递归遍历 `dir`，跳过同步引擎内部目录，把文件相对 `root` 的路径推入 `out`。
///
/// 规则统一到 [`crate::storage::workspace_paths`]，本函数不持有规则副本。
pub(crate) fn walk_commit_candidates(
    root: &Path,
    dir: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    use crate::storage::workspace_paths::is_workspace_internal_path;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Ok(rel) = path.strip_prefix(root) {
            if is_workspace_internal_path(rel) {
                continue;
            }
        }
        if path.is_dir() {
            walk_commit_candidates(root, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_path_buf());
        }
    }
    Ok(())
}
