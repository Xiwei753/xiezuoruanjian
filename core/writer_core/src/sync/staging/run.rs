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
    /// 本 target 是"用户已删除作品"的远端删除意图：所有远端对象都按删除传播。
    delete_intent: bool,
    /// 删除前作品目录的回收区快照根：提供最后的已确认对象基线。
    baseline_root: Option<PathBuf>,
    /// Prepare 阶段在传输之前记录的目标级失败（例如本机统计事件库迁移失败）。
    prepare_error: Option<String>,
    /// 本轮开始时读取一次的持久 mutation 账本快照；多个 App 组共享同一份，
    /// 不在每组 plan 里重复读整份账本。
    ledger_snapshot:
        Option<std::sync::Arc<std::collections::HashMap<String, crate::sync::local_mutations::LocalMutationRecord>>>,
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
            delete_intent: false,
            baseline_root: None,
            prepare_error: None,
            ledger_snapshot: None,
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

    /// 标记该 run 为"删除远端作品"意图。
    pub(crate) fn set_delete_intent(&mut self, delete: bool) {
        self.delete_intent = delete;
    }

    /// 是否把所有远端对象按删除传播。
    pub(crate) fn delete_intent(&self) -> bool {
        self.delete_intent
    }

    /// 删除前作品目录的回收区快照根。
    pub(crate) fn set_baseline_root(&mut self, root: Option<PathBuf>) {
        self.baseline_root = root;
    }

    pub(crate) fn baseline_root(&self) -> Option<&Path> {
        self.baseline_root.as_deref()
    }

    /// 记录目标级失败原因。
    pub fn record_prepare_error(&mut self, message: String) {
        self.prepare_error = Some(message);
    }

    /// 目标级失败原因。
    pub fn prepare_error(&self) -> Option<&str> {
        self.prepare_error.as_deref()
    }

    pub(crate) fn set_ledger_snapshot(
        &mut self,
        ledger: std::sync::Arc<
            std::collections::HashMap<String, crate::sync::local_mutations::LocalMutationRecord>,
        >,
    ) {
        self.ledger_snapshot = Some(ledger);
    }

    pub(crate) fn ledger_snapshot(
        &self,
    ) -> Option<
        &std::sync::Arc<
            std::collections::HashMap<String, crate::sync::local_mutations::LocalMutationRecord>,
        >,
    > {
        self.ledger_snapshot.as_ref()
    }

    /// staging 子目录（Transfer 阶段写入远端变化对象）。
    pub fn staging_root(&self) -> PathBuf {
        self.run_root.join(STAGING_SUBDIR)
    }

    /// base 子目录（章节事务的历史基线视图；不再复制 live 整棵树）。
    pub fn base_root(&self) -> PathBuf {
        self.run_root.join(BASE_SUBDIR)
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
        run.set_delete_intent(matches!(
            planned.target_kind,
            crate::sync::types::PlannedTargetKind::DeleteRemoteProject
        ));
        run.set_baseline_root(planned.baseline_root.clone());
        // 作品删除：只在第一次处理时把删除前回收区里已确认的对象版本写成
        // live 的初始对象索引；之后冲突选择、恢复都读写 live 权威状态，
        // 不再每轮回到回收区读旧基线。
        if let Err(error) = seed_delete_baseline_state(planned) {
            run.record_prepare_error(error);
        }
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

/// 第一次处理作品删除时，用删除前回收区的已确认版本初始化 live 对象索引。
///
/// 只写一次：live 已有 `state.local.json` 就说明本轮之后的所有选择都有权威
/// 状态，不能再拿回收区旧基线覆盖。
fn seed_delete_baseline_state(
    planned: &crate::sync::full_sync::PlannedTarget,
) -> std::result::Result<(), String> {
    if !matches!(
        planned.target_kind,
        crate::sync::types::PlannedTargetKind::DeleteRemoteProject
    ) {
        return Ok(());
    }
    let Some(baseline_root) = planned.baseline_root.as_deref() else {
        return Ok(());
    };
    let live_state = planned
        .target_live_root
        .join("app-meta/sync/state.local.json");
    if live_state.exists() {
        return Ok(());
    }
    let baseline_state = baseline_root.join("app-meta/sync/state.local.json");
    let bytes = match std::fs::read(&baseline_state) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("seed_delete_baseline_state: {error}")),
    };
    if let Some(parent) = live_state.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("seed_delete_baseline_state: {error}"))?;
    }
    crate::storage::transaction::atomic_write_bytes(&live_state, &bytes)
        .map_err(|error| format!("seed_delete_baseline_state: {error}"))?;
    Ok(())
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
