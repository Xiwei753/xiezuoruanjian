use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::Result;
use uuid::Uuid;

use super::commit_plan::*;
use super::resolve::*;

/// 星图 graph.json 和对象文件路径分组。
///
/// 在 `compute_commit_plan` 中收集每个星图的 graph.json 和对象文件路径，
/// 确保 graph.json 成员表合并与对象文件决策一致（issue #902 第九轮复核）。
#[derive(Default)]
pub(crate) struct StarmapFilePaths {
    pub graph_meta_path: Option<PathBuf>,
    /// 星图 meta.json 路径 `starmaps/<id>.meta.json`（issue #902 第十六轮复核）。
    /// meta 服从整图生命周期决策，不再独立走 LWW。
    pub meta_path: Option<PathBuf>,
    pub object_paths: Vec<PathBuf>,
}

const BASE_SUBDIR: &str = "base";
const STAGING_SUBDIR: &str = "staging";

/// 一次 full sync 的隔离 staging run。
///
/// `run_root` = `<parent>/<run_id>`，下含 `base/`（Prepare 时 live 快照）与
/// `staging/`（Transfer 后远端内容）。run 结束调 [StagingRun::cleanup] 整体删除。
///
/// `StagingRun` 不再携带 `active_provider` /
/// `git_seed_state` / `git_layout` 字段。staging 统一走文件级 `seed_from_live`，
/// 不再按 Git/GithubApi backend 走不同 seed 路径。workspace Git 由 bootstrap 阶段
/// 初始化，staging 不负责 Git 生命周期。
pub struct StagingRun {
    run_root: PathBuf,
    run_id: String,
    /// Commit 阶段需要知道这个 staging run 对应的 live root，
    /// 才能调 `compute_commit_plan(live_root)` 做三方比较并把变更写回 live。
    target_live_root: PathBuf,
    /// App group whose files and engine state this staging run may commit.
    /// `None` preserves the legacy App / Project staging behavior.
    app_group: Option<crate::sync::types::AppDataGroup>,
    /// Prepare 阶段在 seed 之前记录的目标级失败（例如本机统计事件库迁移失败）。
    /// 这类失败只属于当前 target：Transfer/Commit 跳过该组，其余独立组继续同步。
    prepare_error: Option<String>,
}

impl StagingRun {
    /// 创建隔离 run 目录。`parent` 通常在 app-data 下（不进 live project root），
    /// 避免被 scanner 当成作品文件。`target_live_root` 记录 Commit 阶段要写回的 live 根。
    pub fn create(parent: &Path, target_live_root: PathBuf) -> Result<Self> {
        let run_id = Uuid::new_v4().to_string();
        let run_root = parent.join("full-sync-staging").join(&run_id);
        fs::create_dir_all(run_root.join(BASE_SUBDIR))?;
        fs::create_dir_all(run_root.join(STAGING_SUBDIR))?;
        Ok(Self {
            run_root,
            run_id,
            target_live_root,
            app_group: None,
            prepare_error: None,
        })
    }

    /// Create a staging run scoped to one versioned App data group.
    pub(crate) fn create_for_app_group(
        parent: &Path,
        target_live_root: PathBuf,
        app_group: crate::sync::types::AppDataGroup,
    ) -> Result<Self> {
        let mut run = Self::create(parent, target_live_root)?;
        run.app_group = Some(app_group);
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

    /// App group associated with this run, if it is a grouped App target.
    pub fn app_group(&self) -> Option<&crate::sync::types::AppDataGroup> {
        self.app_group.as_ref()
    }

    /// Prepare 阶段在 seed 之前记录的目标级失败。
    pub fn record_prepare_error(&mut self, message: String) {
        self.prepare_error = Some(message);
    }

    /// Prepare 阶段记录的目标级失败原因。
    pub fn prepare_error(&self) -> Option<&str> {
        self.prepare_error.as_deref()
    }

    /// staging 子目录（Transfer 阶段写入远端内容的目标）。
    pub fn staging_root(&self) -> PathBuf {
        self.run_root.join(STAGING_SUBDIR)
    }

    /// base 子目录（Prepare 阶段从 live 建 snapshot）。
    pub fn base_root(&self) -> PathBuf {
        self.run_root.join(BASE_SUBDIR)
    }

    /// 从 live 建 base snapshot：仅复制传入的 `rel_paths`，不扫描或复制整个数据目录。
    /// App group 使用独立 inode，保证 base 是 Prepare 时冻结的快照；Project 保留
    /// hard-link 优化。live 文件不存在时跳过（base 也不留，表示该文件 Prepare 时不存在）。
    pub fn build_base_snapshot_from_live(
        &self,
        live_root: &Path,
        rel_paths: &[PathBuf],
    ) -> Result<()> {
        for rel in rel_paths {
            let live_path = live_root.join(rel);
            if !live_path.exists() {
                continue;
            }
            let base_path = self.base_root().join(rel);
            if let Some(parent) = base_path.parent() {
                fs::create_dir_all(parent)?;
            }
            self.copy_live_snapshot_file(&live_path, &base_path)?;
        }
        Ok(())
    }

    /// 从 live 完整初始化 staging run。
    ///
    /// 1. 递归扫描 `live_root` 下所有文件（跳过 `.git/`、`full-sync-staging/`、
    ///    `app-meta/transactions/`；App group 进一步按组筛选），把每个文件写入 `base/`；
    /// 2. 把同一份文件也写入 `staging/`（Transfer 在这份克隆上执行）；
    /// 3. 把 live 的 sync state 和 manifest 复制到 `staging/`，让 LWW 在 staging 里
    ///    能读到 live 的 known_files / device_id / pending / conflict 状态，
    ///    不会重新生成设备身份。
    pub fn seed_from_live(&self, live_root: &Path) -> Result<()> {
        let mut rel_paths = list_live_file_paths(live_root)?;
        if let Some(group) = &self.app_group {
            rel_paths.retain(|rel| group.owns_path(&rel.to_string_lossy().replace('\\', "/")));
            // The aggregate App manifest remains a read-only migration input for the
            // group-scoped LWW snapshot path. It is deliberately excluded from this
            // run's commit candidate set below.
            let legacy_manifest = PathBuf::from("app-meta/sync/manifest.sync.json");
            if live_root.join(&legacy_manifest).is_file() {
                rel_paths.push(legacy_manifest);
            }
            for metadata_path in [
                group.local_manifest_path(),
                group.local_state_path(),
                group.local_conflicts_path(),
            ] {
                let path = PathBuf::from(metadata_path);
                if live_root.join(&path).is_file() {
                    rel_paths.push(path);
                }
            }
            rel_paths.sort();
            rel_paths.dedup();
        }
        // 1. base snapshot
        self.build_base_snapshot_from_live(live_root, &rel_paths)?;
        // 2. staging clone（Transfer 在这份上执行）
        for rel in &rel_paths {
            let live_path = live_root.join(rel);
            let staging_path = self.staging_root().join(rel);
            if let Some(parent) = staging_path.parent() {
                fs::create_dir_all(parent)?;
            }
            self.copy_live_snapshot_file(&live_path, &staging_path)?;
        }
        if let Some(group) = &self.app_group {
            self.seed_legacy_group_state(live_root, group)?;
        }
        Ok(())
    }

    /// Materialize a filtered view of legacy App state in this run only. This keeps
    /// group transfer's existing migration reads available without copying or
    /// committing the aggregate state into another group's staging tree.
    fn seed_legacy_group_state(
        &self,
        live_root: &Path,
        group: &crate::sync::types::AppDataGroup,
    ) -> Result<()> {
        let group_state_path = PathBuf::from(group.local_state_path());
        if let Some(state) =
            super::sync_state_merge::load_app_group_state_with_legacy(live_root, group)?
        {
            self.write_seed_file(
                &group_state_path,
                &serde_json::to_vec_pretty(&state)
                    .map_err(|error| crate::Error::Io(std::io::Error::other(error.to_string())))?,
            )?;
        }

        let legacy_conflicts_path = PathBuf::from("app-meta/sync/conflicts.json");
        let group_conflicts_path = PathBuf::from(group.local_conflicts_path());
        if live_root.join(&legacy_conflicts_path).is_file()
            && !live_root.join(&group_conflicts_path).is_file()
        {
            let conflicts = crate::sync::conflict::load_conflicts_json_for_group(live_root, group)?;
            self.write_seed_file(
                &group_conflicts_path,
                &serde_json::to_vec_pretty(&conflicts)
                    .map_err(|error| crate::Error::Io(std::io::Error::other(error.to_string())))?,
            )?;
        }
        Ok(())
    }

    fn write_seed_file(&self, rel: &Path, bytes: &[u8]) -> Result<()> {
        for root in [self.base_root(), self.staging_root()] {
            let path = root.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            // Replace the run-local directory entry instead of truncating in place.
            // This keeps live state untouched even if a future seed path reuses a
            // hard-linked snapshot entry before applying the legacy-state migration.
            match fs::symlink_metadata(&path) {
                Ok(_) => fs::remove_file(&path)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            fs::write(path, bytes)?;
        }
        Ok(())
    }

    /// Copy selected live files into a run snapshot. App group base/staging files
    /// must never share live inodes: Transfer and legacy-state seeding can rewrite
    /// them before Commit. Project retains the previous hard-link fast path.
    fn copy_live_snapshot_file(&self, live_path: &Path, snapshot_path: &Path) -> Result<()> {
        if self.app_group.is_none() && fs::hard_link(live_path, snapshot_path).is_ok() {
            return Ok(());
        }
        fs::copy(live_path, snapshot_path)?;
        Ok(())
    }

    /// 三方比较生成 commit plan。
    ///
    /// 按 `base ∪ staging` 的路径全集做真正的三方比较，
    /// 值统一用 `Option<bytes>`。`incoming=None` 表示远端删除。
    ///
    /// - `base` = Prepare 时 live（[Self::base_root] 下）
    /// - `local` = 现在 live（`live_root` 下）
    /// - `incoming` = Transfer 后 staging（[Self::staging_root] 下）
    ///
    /// 按 [`ContentClass`] 分类决策，不再统一字节比较：
    /// - [`ContentClass::UserTextDocument`]：走 [`three_way_resolve`]，
    ///   `BothChanged` → [`StagingConflict`]；其余按 NoOp/KeepLocal/Apply。
    /// - [`ContentClass::Metadata`] / [`ContentClass::GeneratedCache`]：真正 LWW
    ///   （时间戳 device_id 决胜，不再固定 remote-wins）。
    /// - [`ContentClass::LocalOnly`]：按 [`StagingCommitClass`] 进一步细分——
    ///   EngineState 写回 live，PlatformConfig/Skip 不写回。
    ///
    /// CommitPlan 拆 `content_actions` `engine_state_actions`。
    /// `app-meta/sync/manifest.sync.json` 和 `app-meta/sync/state.local.json` 作为
    /// EngineState 写回 live；`.git/`、`full-sync-staging/`、`app-meta/transactions/`
    /// 永不进 commit；`config.local.json` / secrets 不从 staging 覆盖 live。
    ///
    /// `incoming=None`（远端删除）+ `local==base` → [CommitAction::Delete]。
    /// `incoming=None` + `local!=base`（UserTextDocument）→ Conflict（本地改了、远端删了）。
    #[allow(
        clippy::excessive_nesting,
        clippy::too_many_lines,
        clippy::cognitive_complexity
    )]
    pub fn compute_commit_plan(&self, live_root: &Path) -> Result<CommitPlan> {
        use crate::sync::content_class::{
            classify_content_path, is_document_content_path, resolve_lww, three_way_resolve,
            ContentClass, LwwWinner, ThreeWayResult,
        };
        use crate::sync::staging::starmap_merge::{
            is_starmap_graph_meta_path, is_starmap_index_path, is_starmap_meta_path,
            is_starmap_object_file_path, merge_starmap_graph_with_object_versions,
            parse_object_path, ObjectMergeDecision, StarmapMergeResult,
        };

        let staging_root = self.staging_root();
        let base_root = self.base_root();
        let mut plan = CommitPlan::default();

        // 读取 live 的 device_id（用于 LWW 决胜）。读取失败时回退空字符串，
        // 退化为纯时间戳比较（仍优于固定 remote-wins）。
        // 加载 live 的 SyncState，获取 tombstones。
        // delete 参与 LWW 时需要 tombstones 里的 deleted_at 时间戳。
        let live_sync_state = match &self.app_group {
            Some(group) => {
                super::sync_state_merge::load_app_group_state_with_legacy(live_root, group)?
            }
            None => read_live_sync_state(live_root),
        };
        let live_device_id = live_sync_state
            .as_ref()
            .map(|state| state.device_id.clone())
            .filter(|device_id| !device_id.is_empty())
            .or_else(|| read_live_device_id(live_root))
            .unwrap_or_default();
        let live_tombstones = live_sync_state
            .as_ref()
            .map(|s| s.tombstones.clone())
            .unwrap_or_default();

        // 读取 base/live 的持久 manifest，保留三方对象版本时间事实；
        // 不能用当前文件系统 mtime 覆盖已有记录。
        let base_manifest_result = match &self.app_group {
            Some(group) => read_live_manifest_for_group(&base_root, group),
            None => read_live_manifest(&base_root),
        };
        let base_manifest = match base_manifest_result {
            Ok(Some(map)) => map,
            Ok(None) => std::collections::HashMap::new(),
            Err(e) => {
                return Err(crate::Error::Other(format!(
                    "compute_commit_plan: base manifest unavailable: {e}"
                )));
            }
        };

        // live manifest 是本地对象的版本事实；
        let live_manifest_result = match &self.app_group {
            Some(group) => read_live_manifest_for_group(live_root, group),
            None => read_live_manifest(live_root),
        };
        let live_manifest = match live_manifest_result {
            Ok(Some(map)) => map,
            Ok(None) => {
                // 首次导入/旧数据迁移：尚无版本记录，允许按当前文件建立初始版本。
                std::collections::HashMap::new()
            }
            Err(e) => {
                // manifest 已存在但读取/解析失败：版本事实未知，不能用 mtime 猜胜负。
                return Err(crate::Error::Other(format!(
                    "compute_commit_plan: live manifest unavailable: {e}"
                )));
            }
        };

        // 读取 staging 的 manifest.sync.json，
        // 获取远端文件的真实 LWW 元数据（updated_at_ms、device_id、op）。
        // Transfer 阶段会写入 manifest；不存在时才允许按首次导入建立初始版本。
        // 存在但损坏时必须让本组 Commit 失败，不能退回 mtime 决策。
        let staging_manifest_result = match &self.app_group {
            Some(group) => read_staging_manifest_for_group(&staging_root, group),
            None => read_staging_manifest(&staging_root),
        };
        let mut staging_manifest = match staging_manifest_result {
            Ok(Some(map)) => map,
            Ok(None) => std::collections::HashMap::new(),
            Err(e) => {
                return Err(crate::Error::Other(format!(
                    "compute_commit_plan: staging manifest unavailable: {e}"
                )));
            }
        };

        // Issue #1005: 远端旧 settings.sync.json 可能仍带设备配色字段，在进入
        // 三方/LWW 比较前规范化隔离 staging 副本及其 manifest 哈希。
        // 本地 live/base 已由 perform_full_sync 在 seed 前统一迁移，这里不得写 live。
        if matches!(
            self.app_group.as_ref(),
            Some(crate::sync::types::AppDataGroup::Global)
        ) {
            normalize_syncable_settings_for_plan(&staging_root, &mut staging_manifest)?;
        }

        // 收集 base ∪ staging 的路径全集。
        // base 和 staging 都走 list_commit_candidate_paths，
        // 排除 `.git/`、`full-sync-staging/`、`app-meta/transactions/`，
        // 不让 Git 元数据被当成正文比较。
        // ReplaceProject 不再走 compute_commit_plan
        // （改走 build_replace_project_plan），普通三方 commit 不需要 live-only 文件
        // 做 Delete，不再把 live_paths 加入 all_paths。
        let base_paths = list_commit_candidate_paths(&base_root)?;
        let staging_paths = list_commit_candidate_paths(&staging_root)?;
        let mut all_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for p in &base_paths {
            all_paths.insert(p.clone());
        }
        for p in &staging_paths {
            all_paths.insert(p.clone());
        }

        // 收集 live-only 星图文件，确保语义合并能看到 local 完整状态。
        // compute_commit_plan 原先只枚举 base∪staging；StarMap 语义合并时
        // 必须同样看到 live-only 新增对象（Transfer 期间新建），否则成员表
        // 与对象文件仍可能错位（issue #902 第八轮复核）。
        let live_starmap_paths = list_starmap_content_paths(live_root)?;
        for p in &live_starmap_paths {
            all_paths.insert(p.clone());
        }

        if let Some(group) = &self.app_group {
            let manifest_path = group.local_manifest_path();
            let state_path = group.local_state_path();
            let conflicts_path = group.local_conflicts_path();
            all_paths.retain(|path| {
                let path = path.to_string_lossy().replace('\\', "/");
                group.owns_path(&path)
                    || path == manifest_path
                    || path == state_path
                    || path == conflicts_path
            });
        }

        // 收集星图 graph.json 和对象文件路径，按 starmap_id 分组。
        // 这些文件需要在循环后统一处理，确保 graph.json 成员表合并与
        // 对象文件决策一致（issue #902 第九轮复核）。
        let mut starmap_groups: std::collections::HashMap<String, StarmapFilePaths> =
            std::collections::HashMap::new();
        for rel in &all_paths {
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if is_starmap_graph_meta_path(&rel_str) {
                let parts: Vec<&str> = rel_str.split('/').collect();
                let starmap_id = parts[1].to_string();
                starmap_groups
                    .entry(starmap_id)
                    .or_default()
                    .graph_meta_path = Some(rel.clone());
            } else if is_starmap_meta_path(&rel_str) {
                // issue #902 第十六轮复核：收集 starmaps/<id>.meta.json，
                // 作为整图生命周期的一部分统一处理。
                let parts: Vec<&str> = rel_str.split('/').collect();
                if let Some(starmap_id) = parts[1].strip_suffix(".meta.json") {
                    let starmap_id = starmap_id.to_string();
                    starmap_groups.entry(starmap_id).or_default().meta_path = Some(rel.clone());
                }
            } else if is_starmap_object_file_path(&rel_str) {
                let parts: Vec<&str> = rel_str.split('/').collect();
                let starmap_id = parts[1].to_string();
                starmap_groups
                    .entry(starmap_id)
                    .or_default()
                    .object_paths
                    .push(rel.clone());
            }
        }

        for rel in all_paths {
            let rel_str = rel.to_string_lossy().to_string();

            //   按 StagingCommitClass 决定 staging commit 写回语义。
            // 远端同步语义（ContentClass）和 staging commit 写回语义（StagingCommitClass）
            // 是两个正交维度，不再复用 LocalOnly。
            let commit_class = match &self.app_group {
                Some(group) => classify_staging_commit_path_for_group(&rel_str, group),
                None => classify_staging_commit_path(&rel_str),
            };
            match commit_class {
                StagingCommitClass::Skip => {
                    // .git/、full-sync-staging/、app-meta/transactions/、
                    // config.local.json、secrets：永不进 commit。
                    continue;
                }
                StagingCommitClass::EngineState => {
                    let staging_path = staging_root.join(&rel);
                    let incoming: Option<Vec<u8>> = if let Some(group) = &self.app_group {
                        if rel_str == group.local_manifest_path() {
                            let base = read_opt_bytes(&base_root.join(&rel))?;
                            let live = read_opt_bytes(&live_root.join(&rel))?;
                            let incoming = read_opt_bytes(&staging_path)?;
                            super::sync_state_merge::merge_app_group_manifest_three_way(
                                base.as_deref(),
                                live.as_deref(),
                                incoming.as_deref(),
                                group,
                            )?
                        } else {
                            read_opt_bytes(&staging_path)?
                        }
                    } else {
                        read_opt_bytes(&staging_path)?
                    };
                    apply_incoming(&mut plan, rel, incoming, StagingCommitClass::EngineState);
                    continue;
                }
                StagingCommitClass::EngineStateMerge => {
                    // app-meta/sync/state.local.json、app-meta/sync/conflicts.json：
                    // 不能直接 apply incoming——用户在同步期间解决的冲突会被
                    // staging 旧基线覆盖复活（Issue #762 评论 5830266600）。
                    // 标记 needs_sync_state_merge=true，由调用方调
                    // merge_sync_state_three_way 做三方语义合并。
                    plan.needs_sync_state_merge = true;
                    continue;
                }
                StagingCommitClass::Content => {
                    // 走下方 ContentClass 决策。
                }
            }

            let base_path = base_root.join(&rel);
            let live_path = live_root.join(&rel);
            let staging_path = staging_root.join(&rel);

            let base: Option<Vec<u8>> = if base_path.exists() {
                Some(read_bytes(&base_path)?)
            } else {
                None
            };
            let local: Option<Vec<u8>> = if live_path.exists() {
                Some(read_bytes(&live_path)?)
            } else {
                None
            };
            let incoming: Option<Vec<u8>> = if staging_path.exists() {
                Some(read_bytes(&staging_path)?)
            } else {
                None
            };

            let local_eq_base = opt_bytes_eq(&local, &base);
            let incoming_eq_base = opt_bytes_eq(&incoming, &base);
            let local_eq_incoming = opt_bytes_eq(&local, &incoming);

            if is_document_content_path(&rel_str) {
                // 正文：走 three_way_resolve，BothChanged 才冲突。
                let base_hash = md5_hex(&base);
                let local_hash = md5_hex(&local);
                let incoming_hash = md5_hex(&incoming);
                match three_way_resolve(&base_hash, &local_hash, &incoming_hash) {
                    ThreeWayResult::NoConflict => {
                        plan.noop.push(rel);
                    }
                    ThreeWayResult::LocalChanged => {
                        // local 改了，incoming==base 没变 → 保留 local。
                        plan.keep_local.push(rel);
                    }
                    ThreeWayResult::RemoteChanged => {
                        // incoming 改了，local==base 没变 → Apply incoming。
                        apply_incoming(&mut plan, rel, incoming, StagingCommitClass::Content);
                    }
                    ThreeWayResult::BothChanged => {
                        // incoming.is_some() → 双端都改了 → BothChanged
                        // incoming.is_none() → 远端删除 + 本地修改 → RemoteDeleted
                        let kind = if incoming.is_some() {
                            crate::sync::types::SyncConflictKind::BothChanged
                        } else {
                            crate::sync::types::SyncConflictKind::RemoteDeleted
                        };
                        plan.conflict.push(StagingConflict {
                            rel_path: rel,
                            base_hash,
                            local_hash,
                            incoming_hash,
                            kind,
                            // 此处尚未保存快照；由 commit_helpers 在 staging cleanup 前
                            // 调 save_conflict_snapshots 填入（#757）。
                            // RemoteDeleted 不保存快照（远端已删除，无 incoming 正文）。
                            remote_snapshot_path: None,
                            remote_graph_snapshot_dir: None,
                        });
                    }
                }
            } else if is_starmap_index_path(&rel_str) {
                // 星图全局索引：推迟到循环后处理，需要先收集各星图的
                // 生命周期决策（issue #902 第十二轮复核）。
                // 此前在主循环内立即合并 index.json，不感知各图 graph 生命周期
                // 决策，导致删除-修改冲突的星图从全局索引消失。
                continue;
            } else if is_starmap_graph_meta_path(&rel_str) {
                // 星图 graph.json：由循环后的统一合并处理，此处跳过
                // （issue #902 第十轮复核）。
                continue;
            } else if is_starmap_meta_path(&rel_str) {
                // 星图 meta.json：由循环后的统一合并处理，作为整图生命周期的一部分
                // （issue #902 第十六轮复核）。不再独立走 LWW，避免 graph 冲突未裁决时
                // meta 被独立删除导致 list_starmaps() 失败。
                continue;
            } else if is_starmap_object_file_path(&rel_str) {
                // 星图对象文件：由循环后的统一合并处理，此处跳过
                // （issue #902 第十轮复核）。
                continue;
            } else {
                // Metadata/GeneratedCache/StarMap meta 文件 走真正 LWW——
                // 时间戳较大方获胜；同时间 device_id 字典序决胜。不再固定 remote-wins。
                // 星图对象文件已在上方 is_starmap_object_file_path 分支走对象级三方决策；
                // 此分支处理 meta 文件和其他 Metadata/GeneratedCache 文件。
                let content_class = classify_content_path(&rel_str);
                debug_assert!(
                    content_class == ContentClass::Metadata
                        || content_class == ContentClass::GeneratedCache
                        || content_class == ContentClass::StarMap,
                    "非正文/星图语义合并类路径必须是 Metadata/GeneratedCache/StarMap，got {:?} for {}",
                    content_class,
                    rel_str
                );

                if incoming_eq_base {
                    plan.keep_local.push(rel);
                } else if local_eq_base {
                    apply_incoming(&mut plan, rel, incoming, StagingCommitClass::Content);
                } else if local_eq_incoming {
                    plan.noop.push(rel);
                } else {
                    // 双方都改 → 真正 LWW 决策。
                    // 本地侧用 live 文件 hash mtime device_id；
                    // 远端侧优先从 staging manifest 读取真实 LWW 元数据
                    // （updated_at_ms、device_id、op），回退到 mtime-based。
                    //
                    // delete 时从 tombstones 查找 deleted_at。
                    // 若本地 delete 无 tombstone 记录，跳过此路径（不参与 LWW）。
                    let local_rec = build_local_lww_record_with_manifest(
                        live_root,
                        &rel,
                        &local,
                        &live_device_id,
                        &live_tombstones,
                        Some(&live_manifest),
                    );
                    let Some(local_rec) = local_rec else {
                        // 本地 delete 无 tombstone → 无法确定删除时间，
                        // 保留本地（不覆盖），下次同步时应补 tombstone。
                        plan.keep_local.push(rel);
                        continue;
                    };
                    let rel_str = rel.to_string_lossy().to_string();
                    let remote_rec = if let Some(manifest_rec) = staging_manifest.get(&rel_str) {
                        match build_remote_lww_record_from_manifest(manifest_rec, &incoming) {
                            Some(record) => record,
                            None => {
                                // manifest 声明 upsert 但本轮没有内容，或 op 未知：
                                // 没有可应用的远端版本，保留本地并等待后续同步。
                                plan.keep_local.push(rel);
                                continue;
                            }
                        }
                    } else {
                        // manifest 中无此路径（首次导入/旧 manifest），
                        // 回退到 staging 文件 mtime 建立初始记录；没有真实
                        // delete 事实时绝不合成远端删除记录。
                        match build_local_lww_record(&staging_root, &rel, &incoming, "remote", &[])
                        {
                            Some(record) => record,
                            None => {
                                plan.keep_local.push(rel);
                                continue;
                            }
                        }
                    };
                    match resolve_lww(&local_rec, &remote_rec) {
                        LwwWinner::Remote => {
                            apply_incoming(&mut plan, rel, incoming, StagingCommitClass::Content);
                        }
                        LwwWinner::Local => {
                            plan.keep_local.push(rel);
                        }
                        LwwWinner::Tie => {
                            plan.noop.push(rel);
                        }
                    }
                }
            }
        }

        // --- 统一处理星图 graph.json + 对象文件 ---
        // 对每个 starmap_group 执行统一合并，确保 graph.json 成员表与对象文件
        // 决策一致（issue #902 第十轮复核）。graph.json 和对象文件不再逐文件独立裁决。
        // 收集各星图的生命周期决策，供 index.json 合并使用（第十二轮复核）。
        let mut graph_lifecycle_decisions: std::collections::HashMap<
            String,
            crate::sync::staging::starmap_merge::GraphLifecycleDecision,
        > = std::collections::HashMap::new();
        let mut starmap_pruned_object_paths = std::collections::HashSet::new();

        for (starmap_id, group) in starmap_groups {
            // issue #902 第十七轮复核问题1：先读取 meta 三方 bytes，
            // 无论 graph 是否存在都需要处理 meta。
            // 刚创建的空星图只有 meta+index、无 graph.json，
            // 不能因 graph_meta_path 为 None 就跳过整个分组。
            let meta_rel = group.meta_path.clone();
            let (meta_base, meta_local, meta_incoming) = if let Some(mr) = &meta_rel {
                (
                    read_opt_bytes(&base_root.join(mr))?,
                    read_opt_bytes(&live_root.join(mr))?,
                    read_opt_bytes(&staging_root.join(mr))?,
                )
            } else {
                (None, None, None)
            };

            let graph_rel = group.graph_meta_path.clone();
            let (graph_base, graph_local, graph_incoming) = if let Some(graph_rel) = &graph_rel {
                (
                    read_opt_bytes(&base_root.join(graph_rel))?,
                    read_opt_bytes(&live_root.join(graph_rel))?,
                    read_opt_bytes(&staging_root.join(graph_rel))?,
                )
            } else {
                // 图文件不存在时仍合并 object_paths：starmap_merge 将没有成员引用的
                // 对象判为孤儿并生成 Delete，不让恢复到 staging 的文件被跳过。
                (None, None, None)
            };

            // 读取每个对象文件的三方 bytes
            // (rel_path, object_type, object_id, base, local, incoming)
            type ObjTriple = (
                PathBuf,
                String,
                String,
                Option<Vec<u8>>,
                Option<Vec<u8>>,
                Option<Vec<u8>>,
            );
            let mut objects: Vec<ObjTriple> = Vec::new();
            for obj_rel in &group.object_paths {
                let (object_type, object_id) = match parse_object_path(obj_rel) {
                    Some(t) => t,
                    None => continue,
                };
                let obj_base = read_opt_bytes(&base_root.join(obj_rel))?;
                let obj_local = read_opt_bytes(&live_root.join(obj_rel))?;
                let obj_incoming = read_opt_bytes(&staging_root.join(obj_rel))?;
                objects.push((
                    obj_rel.clone(),
                    object_type,
                    object_id,
                    obj_base,
                    obj_local,
                    obj_incoming,
                ));
            }

            // 对象版本事实：本地来自 live manifest，远端来自 staging manifest。
            // 只有内容无法判断胜负时才用它们做 LWW。
            let mut object_versions = std::collections::HashMap::new();
            for (obj_rel, _, _, _, obj_local, obj_incoming) in &objects {
                let rel_str = obj_rel.to_string_lossy().to_string();
                let local_record = build_local_lww_record_with_manifest(
                    live_root,
                    obj_rel,
                    obj_local,
                    &live_device_id,
                    &live_tombstones,
                    Some(&live_manifest),
                );
                let incoming_record = staging_manifest
                    .get(&rel_str)
                    .and_then(|record| build_remote_lww_record_from_manifest(record, obj_incoming));
                if local_record.is_some() || incoming_record.is_some() {
                    object_versions.insert(rel_str, (local_record, incoming_record));
                }
            }

            let output = merge_starmap_graph_with_object_versions(
                &starmap_id,
                graph_base.as_deref(),
                graph_local.as_deref(),
                graph_incoming.as_deref(),
                meta_base.as_deref(),
                meta_local.as_deref(),
                meta_incoming.as_deref(),
                &objects,
                &object_versions,
            )?;

            // 收集生命周期决策，供 index.json 合并使用（issue #902 第十二轮复核）。
            // DeleteModifyConflict 的星图不会被 merge_starmap_index 从全局索引移除。
            graph_lifecycle_decisions.insert(starmap_id.clone(), output.lifecycle_decision.clone());

            // graph 级冲突事实：DeleteModifyConflict 时产生 graph.json 的 StagingConflict
            // （issue #902 第十二轮复核）。此前只靠对象级 Conflict 隐式传递整图冲突，
            // 用户无法在 graph.json 层面看到冲突。
            if let (Some(graph_rel), Some(graph_conflict_kind)) =
                (&graph_rel, output.graph_conflict)
            {
                let graph_base_hash = md5_hex(&graph_base);
                let graph_local_hash = md5_hex(&graph_local);
                let graph_incoming_hash = md5_hex(&graph_incoming);
                plan.conflict.push(StagingConflict {
                    rel_path: graph_rel.clone(),
                    base_hash: graph_base_hash,
                    local_hash: graph_local_hash,
                    incoming_hash: graph_incoming_hash,
                    kind: graph_conflict_kind,
                    remote_snapshot_path: None,
                    // graph 级冲突的整图远端快照目录由 save_conflict_snapshots
                    // 在 staging cleanup 前填入（BothChanged 时保存全部子对象）。
                    remote_graph_snapshot_dir: None,
                });
            }

            // graph.json 结果装入 CommitPlan；没有 graph 路径时不生成虚构动作，
            // 但对象决策仍然进入共同处理路径。
            if let Some(graph_rel) = graph_rel.as_ref() {
                match output.graph_meta_result {
                    StarmapMergeResult::NoOp => {
                        plan.noop.push(graph_rel.clone());
                    }
                    StarmapMergeResult::KeepLocal => {
                        plan.keep_local.push(graph_rel.clone());
                    }
                    StarmapMergeResult::ApplyIncoming(content)
                    | StarmapMergeResult::ApplyMerged(content) => {
                        plan.content_actions.push(CommitAction::Apply {
                            rel_path: graph_rel.clone(),
                            content,
                        });
                    }
                    StarmapMergeResult::Delete => {
                        // 远端删除了整个 graph 且 local 未改 → 删除 live 的 graph.json
                        // 修复 issue #902 第十一轮复核
                        plan.content_actions.push(CommitAction::Delete {
                            rel_path: graph_rel.clone(),
                        });
                    }
                }
            }

            // 对象决策装入 CommitPlan
            for (idx, (obj_rel, decision)) in output.object_decisions.iter().enumerate() {
                let (_, _, _, obj_base, obj_local, obj_incoming) = &objects[idx];
                let base_hash = md5_hex(obj_base);
                let local_hash = md5_hex(obj_local);
                let incoming_hash = md5_hex(obj_incoming);
                match decision {
                    ObjectMergeDecision::NoOp => {
                        plan.noop.push(obj_rel.clone());
                    }
                    ObjectMergeDecision::KeepLocal => {
                        plan.keep_local.push(obj_rel.clone());
                    }
                    ObjectMergeDecision::ApplyIncoming(content) => {
                        plan.content_actions.push(CommitAction::Apply {
                            rel_path: obj_rel.clone(),
                            content: content.clone(),
                        });
                    }
                    ObjectMergeDecision::Delete => {
                        starmap_pruned_object_paths
                            .insert(obj_rel.to_string_lossy().replace('\\', "/"));
                        // 按 live 文件是否存在决定动作：
                        // - live 仍有该对象文件 → 必须发出 Delete 动作真正删除磁盘文件
                        // - live 已不存在 → 无需操作（noop）
                        // 修复 issue #902 第十一轮复核：此前一律 noop，导致
                        // base 有 Y、live 仍有未改的 Y、incoming 删除 Y 时，
                        // graph 已删成员但磁盘残留 Y 文件。
                        if obj_local.is_some() {
                            plan.content_actions.push(CommitAction::Delete {
                                rel_path: obj_rel.clone(),
                            });
                        } else {
                            plan.noop.push(obj_rel.clone());
                        }
                    }
                    ObjectMergeDecision::Conflict(kind) => {
                        plan.conflict.push(StagingConflict {
                            rel_path: obj_rel.clone(),
                            base_hash,
                            local_hash,
                            incoming_hash,
                            kind: *kind,
                            remote_snapshot_path: None,
                            remote_graph_snapshot_dir: None,
                        });
                    }
                }
            }

            // issue #902 第十六轮复核：meta.json 作为整图生命周期的一部分
            // 服从 graph 的生命周期决策，不再独立走 LWW。
            // issue #902 第十七轮复核：meta 处理逻辑提取为 apply_meta_lifecycle_decision，
            // 与 graph 不存在时的 meta-only 处理共用同一逻辑。
            if let Some(mr) = &meta_rel {
                apply_meta_lifecycle_decision(
                    &mut plan,
                    &output.lifecycle_decision,
                    mr,
                    &meta_base,
                    &meta_local,
                    &meta_incoming,
                    graph_local.is_some(),
                    graph_incoming.is_some(),
                    live_root,
                    &staging_root,
                    &live_device_id,
                    &live_tombstones,
                    &live_manifest,
                    &staging_manifest,
                )?;

                // meta-only 空星图的 DeleteModifyConflict 由 meta 路径承载，
                // 否则用户无法看到并选择整图生命周期冲突。
                if graph_rel.is_none() {
                    if let crate::sync::staging::starmap_merge::GraphLifecycleDecision::DeleteModifyConflict { is_local_deleted } = &output.lifecycle_decision {
                        let kind = if *is_local_deleted {
                            crate::sync::types::SyncConflictKind::BothChanged
                        } else {
                            crate::sync::types::SyncConflictKind::RemoteDeleted
                        };
                        plan.conflict.push(StagingConflict {
                            rel_path: mr.clone(),
                            base_hash: md5_hex(&meta_base),
                            local_hash: md5_hex(&meta_local),
                            incoming_hash: md5_hex(&meta_incoming),
                            kind,
                            remote_snapshot_path: None,
                            remote_graph_snapshot_dir: None,
                        });
                    }
                }
            }
        }

        // --- 处理星图全局索引 index.json（issue #902 第十二轮复核）---
        // 推迟到所有星图 graph+objects 合并完成后，传入各图生命周期决策。
        // 删除-修改冲突（DeleteModifyConflict）的星图不会被 merge_starmap_index
        // 从全局索引移除，避免 graph 成为未登记的孤儿。
        {
            use crate::sync::staging::starmap_merge::{merge_starmap_index, StarmapMergeResult};
            let index_rel = PathBuf::from("starmaps/index.json");
            let index_base = read_opt_bytes(&base_root.join(&index_rel))?;
            let index_local = read_opt_bytes(&live_root.join(&index_rel))?;
            let index_incoming = read_opt_bytes(&staging_root.join(&index_rel))?;

            // 只在三方至少一方存在时处理；三方都不存在则跳过
            if index_base.is_some() || index_local.is_some() || index_incoming.is_some() {
                match merge_starmap_index(
                    index_base.as_deref(),
                    index_local.as_deref(),
                    index_incoming.as_deref(),
                    &graph_lifecycle_decisions,
                )? {
                    StarmapMergeResult::NoOp => {
                        plan.noop.push(index_rel);
                    }
                    StarmapMergeResult::KeepLocal => {
                        plan.keep_local.push(index_rel);
                    }
                    StarmapMergeResult::ApplyIncoming(content)
                    | StarmapMergeResult::ApplyMerged(content) => {
                        plan.content_actions.push(CommitAction::Apply {
                            rel_path: index_rel,
                            content,
                        });
                    }
                    StarmapMergeResult::Delete => {
                        // index.json 不会走整图删除路径（Delete 变体仅用于
                        // graph.json 的整图删除）。此处兜底处理，避免漏分支。
                        plan.noop.push(index_rel);
                    }
                }
            }
        }

        if !starmap_pruned_object_paths.is_empty() {
            plan.needs_sync_state_merge = true;
            plan.starmap_pruned_object_paths = starmap_pruned_object_paths.clone();
            write_starmap_delete_records_to_manifest(
                &mut plan.engine_state_actions,
                &self.app_group,
                &starmap_pruned_object_paths,
                &base_manifest,
                &live_manifest,
                &staging_manifest,
                &live_device_id,
            )?;
        }

        Ok(plan)
    }

    /// 清理整个 run 目录。
    pub fn cleanup(&self) {
        let _ = fs::remove_dir_all(&self.run_root);
    }
}

/// Issue #1005: 规范化隔离 staging 中的 settings.sync.json，
/// 并让对应 manifest 的内容哈希与实际写盘字节保持一致。
fn normalize_syncable_settings_for_plan(
    root: &Path,
    manifest: &mut std::collections::HashMap<String, super::resolve::ManifestRecord>,
) -> Result<()> {
    const REL: &str = "settings.sync.json";
    if let Some(bytes) = crate::settings::normalize_syncable_settings_file(&root.join(REL))? {
        if let Some(record) = manifest.get_mut(REL) {
            record.content_hash = crate::sync::hash::content_md5(&bytes);
        }
    }
    Ok(())
}

fn write_starmap_delete_records_to_manifest(
    actions: &mut Vec<CommitAction>,
    app_group: &Option<crate::sync::types::AppDataGroup>,
    deleted_paths: &std::collections::HashSet<String>,
    base_records: &std::collections::HashMap<String, super::resolve::ManifestRecord>,
    live_records: &std::collections::HashMap<String, super::resolve::ManifestRecord>,
    staging_records: &std::collections::HashMap<String, super::resolve::ManifestRecord>,
    device_id: &str,
) -> Result<()> {
    use crate::sync::types::{ManifestFileRecord, SyncManifest};

    let manifest_rel = app_group
        .as_ref()
        .map(crate::sync::types::AppDataGroup::local_manifest_path)
        .unwrap_or_else(|| "app-meta/sync/manifest.sync.json".to_string());
    let action_index = actions.iter().position(|action| {
        let path = match action {
            CommitAction::Apply { rel_path, .. } | CommitAction::Delete { rel_path } => rel_path,
        };
        path.to_string_lossy().replace('\\', "/") == manifest_rel
    });

    let mut manifest = if let Some(index) = action_index {
        match &actions[index] {
            CommitAction::Apply { content, .. } => serde_json::from_slice::<SyncManifest>(content)
                .map_err(|error| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "write_starmap_delete_records_to_manifest: invalid merged manifest: {error}"
                    )))
                })?,
            CommitAction::Delete { .. } => SyncManifest::default(),
        }
    } else {
        // A manifest may not exist on the first sync. Preserve any records already
        // available in either snapshot, then add the semantic graph deletions.
        let to_file_record = |record: &super::resolve::ManifestRecord| ManifestFileRecord {
            path: record.path.clone(),
            content_hash: record.content_hash.clone(),
            updated_at_ms: record.updated_at_ms,
            deleted_at_ms: record.deleted_at_ms,
            device_id: record.device_id.clone(),
            op: record.op.clone(),
            schema_version: 1,
        };
        let mut records: std::collections::BTreeMap<
            String,
            crate::sync::types::ManifestFileRecord,
        > = base_records
            .iter()
            .map(|(path, record)| (path.clone(), to_file_record(record)))
            .collect();
        for (path, incoming_record) in live_records.iter().chain(staging_records.iter()) {
            match records.get(path) {
                Some(live_record) => {
                    let winner = crate::sync::content_class::resolve_lww(
                        &crate::sync::content_class::LwwRecord {
                            content_hash: live_record.content_hash.clone(),
                            updated_at_ms: live_record.updated_at_ms,
                            deleted_at_ms: live_record.deleted_at_ms,
                            device_id: live_record.device_id.clone(),
                            op: live_record.op.clone(),
                        },
                        &crate::sync::content_class::LwwRecord {
                            content_hash: incoming_record.content_hash.clone(),
                            updated_at_ms: incoming_record.updated_at_ms,
                            deleted_at_ms: incoming_record.deleted_at_ms,
                            device_id: incoming_record.device_id.clone(),
                            op: incoming_record.op.clone(),
                        },
                    );
                    if winner == crate::sync::content_class::LwwWinner::Remote {
                        records.insert(path.clone(), to_file_record(incoming_record));
                    }
                }
                None => {
                    records.insert(path.clone(), to_file_record(incoming_record));
                }
            }
        }
        SyncManifest {
            files: records.into_values().collect(),
        }
    };

    let now_ms = chrono::Utc::now().timestamp_millis();
    for path in deleted_paths {
        let merged_record = manifest.files.iter().find(|record| record.path == *path);
        let base_record = base_records.get(path);
        let stage_record = staging_records.get(path);
        let live_record = live_records.get(path);

        let record_time = |updated_at_ms: i64, deleted_at_ms: Option<i64>| {
            updated_at_ms.max(deleted_at_ms.unwrap_or(i64::MIN))
        };
        let latest_record_time = merged_record
            .map(|record| record_time(record.updated_at_ms, record.deleted_at_ms))
            .into_iter()
            .chain(
                base_record.map(|record| record_time(record.updated_at_ms, record.deleted_at_ms)),
            )
            .chain(
                stage_record.map(|record| record_time(record.updated_at_ms, record.deleted_at_ms)),
            )
            .chain(
                live_record.map(|record| record_time(record.updated_at_ms, record.deleted_at_ms)),
            )
            .max()
            .unwrap_or(i64::MIN);
        let merged_delete_wins = merged_record.is_some_and(|record| {
            record.op == "delete"
                && [base_record, stage_record, live_record]
                    .into_iter()
                    .flatten()
                    .all(|other| {
                        crate::sync::content_class::resolve_lww(
                            &crate::sync::content_class::LwwRecord {
                                content_hash: record.content_hash.clone(),
                                updated_at_ms: record.updated_at_ms,
                                deleted_at_ms: record.deleted_at_ms,
                                device_id: record.device_id.clone(),
                                op: record.op.clone(),
                            },
                            &crate::sync::content_class::LwwRecord {
                                content_hash: other.content_hash.clone(),
                                updated_at_ms: other.updated_at_ms,
                                deleted_at_ms: other.deleted_at_ms,
                                device_id: other.device_id.clone(),
                                op: other.op.clone(),
                            },
                        ) != crate::sync::content_class::LwwWinner::Remote
                    })
        });
        if merged_delete_wins {
            continue;
        }
        let deleted_at_ms = now_ms.max(latest_record_time).saturating_add(1);
        let schema_version = merged_record
            .map(|record| record.schema_version)
            .unwrap_or(1);
        let tombstone = ManifestFileRecord {
            path: path.clone(),
            content_hash: String::new(),
            updated_at_ms: deleted_at_ms,
            deleted_at_ms: Some(deleted_at_ms),
            device_id: device_id.to_string(),
            op: "delete".to_string(),
            schema_version,
        };
        if let Some(record) = manifest
            .files
            .iter_mut()
            .find(|record| record.path == *path)
        {
            *record = tombstone;
        } else {
            manifest.files.push(tombstone);
        }
    }
    manifest
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));

    let content = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| crate::Error::Io(std::io::Error::other(error.to_string())))?;
    let rel_path = PathBuf::from(&manifest_rel);
    let action = CommitAction::Apply { rel_path, content };
    if let Some(index) = action_index {
        actions[index] = action;
    } else {
        actions.push(action);
    }
    Ok(())
}

///   第1/2节：在**无 Core 锁**状态下创建并 seed 所有 staging runs。
///
/// 纯函数，不依赖 `WriterCore`，可在无锁状态下调用。
/// 对每个 target 创建 `StagingRun`，统一调 `seed_from_live`（文件级复制，跳过 `.git/`）。
///
/// seed 失败**不再**被 `log::warn!` 吞掉。
/// 任何一个 target 的 seed 失败都意味着该 target 的 staging 是半成品，
/// 不能拿来做三方比较。seed 失败直接返回 Err，让调用方终止本次 full sync。
///
/// 例外：本机统计设备组（#955）的旧事件库迁移发生在 seed 之前，失败记录在该
/// run 的 [StagingRun::prepare_error] 上，只让这一组本次失败，不终止整轮同步。
///
/// 不再在此函数中初始化 workspace Git。
/// 本地 Git 仓库由 bootstrap 阶段初始化，staging 只负责文件级快照。
/// 不再按 `active_provider` 分 Git/GithubApi 走不同 seed 路径。
///
/// 成功后 `plan.targets[*].staging_root` 被填充为对应 staging 目录。
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
        // #955：本机统计设备组必须在 seed 之前完成旧事件库迁移。
        // 迁移写的是 live 的 `stats/events/<device_id>/`；放到 Transfer 的 staging
        // 里执行时，staging 既没有 `device/` 也没有旧事件目录，条件永远不成立。
        // 迁移失败只记录到该 run：本组本次跳过 Transfer/Commit，其它组继续同步。
        if let Some(error) = migrate_local_stats_group_before_seed(planned) {
            run.record_prepare_error(error);
        } else if !planned.is_deleted_target() {
            //   deleted target 不 seed（不读本地目录，
            // 只枚举远端删除）。创建空 staging run 保持索引对齐，commit 阶段
            // compute_commit_plan 会发现 staging 为空，自然 Skip。
            // seed 失败必须传播，不能继续拿半成品 staging。
            // 统一调 seed_from_live，不再按 backend 分支。
            run.seed_from_live(&planned.target_live_root)?;
        }
        planned.staging_root = Some(run.staging_root());
        staging_runs.push(run);
    }

    Ok(staging_runs)
}

/// 本机统计设备组的旧事件库迁移（#955）。
///
/// 对 `AppDataGroup::StatsDevice(device_id)` 且 `device_id` 等于 live 根
/// `device/current_device.json` 中设备 ID 的 target，在 `seed_from_live` 之前
/// 调用幂等迁移，把 `app-meta/stats/events.local/` 写入 live 的
/// `stats/events/<device_id>/`。
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

impl Drop for StagingRun {
    fn drop(&mut self) {
        // 兜底清理：commit 正常路径会显式 cleanup，这里防止提前 drop 残留。
        let _ = fs::remove_dir_all(&self.run_root);
    }
}

/// 根据整图生命周期决策处理 meta.json 的 CommitAction（issue #902 第十七轮复核）。
///
/// meta 服从整图生命周期决策，不再独立走 LWW。
/// **问题3**：DeleteModifyConflict 时 meta 也必须 NoOp/KeepLocal，
/// 不提前写入 meta_incoming，和 graph、对象、index 维持同一未决状态。
/// 用户选择采用远端时再从已保存的 meta 快照连同 graph、成员对象和 index 一起写入。
///
/// **issue #902 第十九轮复核**：
/// - 问题2：Normal 分支先求出最终 meta 决策（MetaFinalAction），再统一检查
///   graph 存在性不变量（graph 仍存在时最终 meta 必须存在），最后才写入
///   CommitPlan。覆盖 KeepLocal/NoOp/Apply/Delete 全部分支，不再只给
///   Delete(meta) 加局部检查，避免把"有 graph/index、没有 meta"的半张图
///   当作 NoOp/KeepLocal。
/// - 问题3：Normal 分支双方都改时复用现有 LWW resolver，
///   不再无条件 KeepLocal 丢失 LWW 语义。
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::excessive_nesting
)]
fn apply_meta_lifecycle_decision(
    plan: &mut CommitPlan,
    lifecycle_decision: &crate::sync::staging::starmap_merge::GraphLifecycleDecision,
    meta_rel: &Path,
    meta_base: &Option<Vec<u8>>,
    meta_local: &Option<Vec<u8>>,
    meta_incoming: &Option<Vec<u8>>,
    graph_local_exists: bool,
    graph_incoming_exists: bool,
    live_root: &Path,
    staging_root: &Path,
    live_device_id: &str,
    live_tombstones: &[crate::sync::types::Tombstone],
    live_manifest: &std::collections::HashMap<
        String,
        crate::sync::staging::resolve::ManifestRecord,
    >,
    staging_manifest: &std::collections::HashMap<
        String,
        crate::sync::staging::resolve::ManifestRecord,
    >,
) -> Result<()> {
    use crate::sync::staging::starmap_merge::GraphLifecycleDecision;

    // issue #902 第十九轮复核问题2：Normal 分支统一 meta 存在性防护。
    // 先求出最终 meta 决策（MetaFinalAction），再统一检查 graph 存在性不变量，
    // 最后才写入 CommitPlan。避免 m_incoming_eq_base(KeepLocal) 和
    // m_local_eq_incoming(NoOp) 分支在 graph 仍存在时漏检 meta 缺失，
    // 把"有 graph/index、没有 meta"的半张图当作 NoOp/KeepLocal。
    enum MetaFinalAction {
        Apply(Vec<u8>),
        KeepLocal,
        NoOp,
        Delete,
    }

    // 将最终 meta 决策经 graph 存在性不变量检查后写入 CommitPlan。
    //
    // graph 仍存在时，最终 meta 必须存在（index 登记的有效实体必须存在 meta）：
    // - Apply(content)：content 即将写入，meta 会存在，恒通过。
    // - KeepLocal：保留本地 meta，要求 meta_local 存在。
    // - NoOp：不写任何动作，要求 meta_local 与 meta_incoming 至少一个存在
    //   （NoOp 出现路径上两者存在性一致，等价于 meta_local 存在）。
    // - Delete：将删除 meta，graph 仍存在时禁止。
    fn finalize_meta_action(
        plan: &mut CommitPlan,
        meta_rel: &Path,
        action: MetaFinalAction,
        graph_still_exists: bool,
        meta_local: &Option<Vec<u8>>,
        meta_incoming: &Option<Vec<u8>>,
    ) -> Result<()> {
        if graph_still_exists {
            let missing_reason = match &action {
                MetaFinalAction::Delete => Some("最终决策 Delete 将删除 meta"),
                MetaFinalAction::KeepLocal => {
                    if meta_local.is_none() {
                        Some("最终决策 KeepLocal 但本地 meta 不存在")
                    } else {
                        None
                    }
                }
                MetaFinalAction::NoOp => {
                    if meta_local.is_none() && meta_incoming.is_none() {
                        Some("最终决策 NoOp 但本地与远端 meta 均不存在")
                    } else {
                        None
                    }
                }
                MetaFinalAction::Apply(_) => None,
            };
            if let Some(reason) = missing_reason {
                return Err(crate::error::Error::Other(format!(
                    "apply_meta_lifecycle_decision: graph 仍存在但 meta 缺失 ({})，{}；\
                     index 登记的有效实体必须存在 meta",
                    meta_rel.display(),
                    reason
                )));
            }
        }
        match action {
            MetaFinalAction::Apply(content) => {
                plan.content_actions.push(CommitAction::Apply {
                    rel_path: meta_rel.to_path_buf(),
                    content,
                });
            }
            MetaFinalAction::KeepLocal => {
                plan.keep_local.push(meta_rel.to_path_buf());
            }
            MetaFinalAction::NoOp => {
                plan.noop.push(meta_rel.to_path_buf());
            }
            MetaFinalAction::Delete => {
                plan.content_actions.push(CommitAction::Delete {
                    rel_path: meta_rel.to_path_buf(),
                });
            }
        }
        Ok(())
    }

    let graph_still_exists = graph_local_exists || graph_incoming_exists;
    match lifecycle_decision {
        // 整图删除 → meta 也删除
        GraphLifecycleDecision::Delete => {
            if meta_local.is_some() {
                plan.content_actions.push(CommitAction::Delete {
                    rel_path: meta_rel.to_path_buf(),
                });
            } else {
                plan.noop.push(meta_rel.to_path_buf());
            }
        }
        // 正常合并 → meta 走正常三方比较
        GraphLifecycleDecision::Normal => {
            let m_local_eq_base = opt_bytes_eq(meta_local, meta_base);
            let m_incoming_eq_base = opt_bytes_eq(meta_incoming, meta_base);
            let m_local_eq_incoming = opt_bytes_eq(meta_local, meta_incoming);

            let final_action = if m_incoming_eq_base {
                MetaFinalAction::KeepLocal
            } else if m_local_eq_base {
                if let Some(content) = meta_incoming.clone() {
                    MetaFinalAction::Apply(content)
                } else {
                    MetaFinalAction::Delete
                }
            } else if m_local_eq_incoming {
                MetaFinalAction::NoOp
            } else {
                // 双方都改 → 复用现有 LWW resolver
                // （issue #902 第十九轮复核问题3）。
                // 不能悄悄将既有同步语义改为 local-wins。
                use crate::sync::content_class::{resolve_lww, LwwWinner};

                let local_rec = build_local_lww_record_with_manifest(
                    live_root,
                    meta_rel,
                    meta_local,
                    live_device_id,
                    live_tombstones,
                    Some(live_manifest),
                );
                let Some(local_rec) = local_rec else {
                    // 本地 delete 无 tombstone → 保留本地
                    let early_action = if meta_local.is_some() {
                        MetaFinalAction::KeepLocal
                    } else if let Some(content) = meta_incoming.clone() {
                        MetaFinalAction::Apply(content)
                    } else {
                        MetaFinalAction::NoOp
                    };
                    return finalize_meta_action(
                        plan,
                        meta_rel,
                        early_action,
                        graph_still_exists,
                        meta_local,
                        meta_incoming,
                    );
                };
                let meta_rel_str = meta_rel.to_string_lossy().to_string();
                let remote_rec = if let Some(manifest_rec) = staging_manifest.get(&meta_rel_str) {
                    match build_remote_lww_record_from_manifest(manifest_rec, meta_incoming) {
                        Some(record) => record,
                        None => {
                            let early_action = if meta_local.is_some() {
                                MetaFinalAction::KeepLocal
                            } else {
                                MetaFinalAction::NoOp
                            };
                            return finalize_meta_action(
                                plan,
                                meta_rel,
                                early_action,
                                graph_still_exists,
                                meta_local,
                                meta_incoming,
                            );
                        }
                    }
                } else {
                    // 首次导入没有版本记录：用 staging 文件 mtime 建立初始记录。
                    // 没有真实删除事实时保留本地，绝不合成远端 delete。
                    match build_local_lww_record(
                        staging_root,
                        meta_rel,
                        meta_incoming,
                        "remote",
                        &[],
                    ) {
                        Some(record) => record,
                        None => {
                            let early_action = if meta_local.is_some() {
                                MetaFinalAction::KeepLocal
                            } else {
                                MetaFinalAction::NoOp
                            };
                            return finalize_meta_action(
                                plan,
                                meta_rel,
                                early_action,
                                graph_still_exists,
                                meta_local,
                                meta_incoming,
                            );
                        }
                    }
                };
                match resolve_lww(&local_rec, &remote_rec) {
                    LwwWinner::Remote => {
                        if let Some(content) = meta_incoming.clone() {
                            MetaFinalAction::Apply(content)
                        } else {
                            MetaFinalAction::Delete
                        }
                    }
                    LwwWinner::Local => {
                        if meta_local.is_some() {
                            MetaFinalAction::KeepLocal
                        } else {
                            MetaFinalAction::Delete
                        }
                    }
                    LwwWinner::Tie => MetaFinalAction::NoOp,
                }
            };

            return finalize_meta_action(
                plan,
                meta_rel,
                final_action,
                graph_still_exists,
                meta_local,
                meta_incoming,
            );
        }
        // 整图冲突未决 → meta 也必须 NoOp/KeepLocal，
        // 和 graph、对象、index 维持同一未决状态。
        // issue #902 第十七轮复核问题3：不提前写入 meta_incoming，
        // 等用户选择采用远端时从已保存的 meta 快照连同 graph、
        // 成员对象和 index 一起写入。
        GraphLifecycleDecision::DeleteModifyConflict { .. } => {
            if meta_local.is_some() {
                plan.keep_local.push(meta_rel.to_path_buf());
            } else {
                plan.noop.push(meta_rel.to_path_buf());
            }
        }
    }
    Ok(())
}

/// 递归列出 live root 下所有需要参与同步的文件相对路径。
///
/// 跳过 `.git/`、`full-sync-staging/`（避免递归复制 staging 自身）、
/// `app-meta/transactions/`（事务暂存目录）。
fn list_live_file_paths(live_root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !live_root.exists() {
        return Ok(out);
    }
    walk_commit_candidates(live_root, live_root, &mut out)?;
    Ok(out)
}

/// 列出 base/staging 下参与 commit 比较的候选路径。
///
/// 与 [`list_live_file_paths`] 共用同一套跳过规则，确保 base/staging/live
/// 三方比较只看同步业务文件，不会把 `.git/`、`full-sync-staging/`、
/// `app-meta/transactions/` 当成正文比较。
///
/// `CommitAction` / `StagingConflict` 永远不会出现被跳过的路径。
fn list_commit_candidate_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    walk_commit_candidates(root, root, &mut out)?;
    Ok(out)
}

/// 递归遍历 `dir`，跳过同步引擎内部目录，把文件相对 `root` 的路径推入 `out`。
///
/// 跳过规则（与 live 扫描、commit candidate 共用同一套）：
/// - `.git/`（目录）：Git 仓库元数据，不是用户内容；
/// - `.git`（文件）： — gitlink file，同样不是用户内容；
/// - `.git.sujian-tmp-*`：迁移/恢复过程中的临时目录；
/// - `.git.sujian-migrate-source-*`：迁移崩溃后残留的源仓库快照；
/// - `full-sync-staging/`：staging run 自身，避免递归；
/// - `app-meta/transactions/`：事务暂存目录，commit 中间态。
///
/// 底层规则统一到
/// [`crate::storage::workspace_paths`]，本函数不再持有规则副本。
#[allow(clippy::excessive_nesting)]
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
        } else {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
    }
    Ok(())
}

fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

/// 读取文件内容，文件不存在时返回 `None`。
fn read_opt_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    if path.exists() {
        Ok(Some(read_bytes(path)?))
    } else {
        Ok(None)
    }
}

/// 递归列出 live_root 下 starmaps/ 目录中所有 .json 文件的相对路径。
///
/// 用于在 compute_commit_plan 中收集 live-only 星图文件，确保语义合并
/// 能看到 local 的完整状态。如果 starmaps/ 不存在返回空 Vec。
///
/// 与 [`walk_commit_candidates`] 不同，本函数不过滤 workspace 内部路径——
/// starmaps/ 本身就是业务目录，不会被 `.git/` 等内部路径包含。
fn list_starmap_content_paths(live_root: &Path) -> Result<Vec<PathBuf>> {
    let starmaps_dir = live_root.join("starmaps");
    if !starmaps_dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    walk_starmap_json_files(live_root, &starmaps_dir, &mut out)?;
    Ok(out)
}

/// 递归遍历 starmaps/ 目录，收集符合星图内容路径模式的 .json 文件。
///
/// 只收集 [`starmap_merge::is_starmap_content_path`] 认可的文件，
/// 跳过不符合预期布局的临时文件或异常文件。
fn walk_starmap_json_files(live_root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_starmap_json_files(live_root, &path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            collect_starmap_json(live_root, &path, out);
        }
    }
    Ok(())
}

/// 检查单个 `.json` 文件是否为星图内容路径，是则收集到 `out`。
fn collect_starmap_json(live_root: &Path, path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rel) = path.strip_prefix(live_root) else {
        return;
    };
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    if super::starmap_merge::is_starmap_content_path(&rel_str) {
        out.push(rel.to_path_buf());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starmap::store::meta::{DeletedSinceLastSync, GraphMeta};
    use std::collections::HashMap;

    /// 构造 GraphMeta JSON bytes。
    fn make_graph_meta_bytes(
        starmap_id: &str,
        node_ids: &[&str],
        node_revisions: &[(&str, u64)],
        package_revision: u64,
        updated_at: u64,
    ) -> Vec<u8> {
        let mut node_revs: HashMap<String, u64> = HashMap::new();
        for (id, rev) in node_revisions {
            node_revs.insert(id.to_string(), *rev);
        }
        let meta = GraphMeta {
            schema_version: crate::starmap::store::meta::CURRENT_SCHEMA_VERSION.to_string(),
            starmap_id: starmap_id.to_string(),
            node_ids: node_ids.iter().map(|s| s.to_string()).collect(),
            edge_ids: vec![],
            embed_instance_ids: vec![],
            link_ids: vec![],
            hyperlink_ids: vec![],
            edge_relation_index: vec![],
            embed_host_index: vec![],
            link_relation_index: vec![],
            hyperlink_relation_index: vec![],
            node_kind_counts: HashMap::new(),
            node_revisions: node_revs,
            edge_revisions: HashMap::new(),
            embed_revisions: HashMap::new(),
            link_revisions: HashMap::new(),
            hyperlink_revisions: HashMap::new(),
            package_revision,
            updated_at,
            deleted_since_last_sync: DeletedSinceLastSync::default(),
        };
        serde_json::to_vec_pretty(&meta).expect("serialize graph meta")
    }

    /// 写入文件，自动创建父目录。
    fn write_file(root: &Path, rel: &str, content: &[u8]) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create dirs");
        }
        fs::write(&path, content).expect("write file");
    }

    // --- 问题1 集成回归：live 仍有对象文件时 content_actions 含 Delete ---

    #[test]
    fn test_issue902_object_delete_emits_commit_action_delete() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let live_root = tmp.path().join("live");
        fs::create_dir_all(&live_root).expect("create live root");

        let run_parent = tmp.path().join("runs");
        fs::create_dir_all(&run_parent).expect("create run parent");

        let run = StagingRun::create(&run_parent, live_root.clone()).expect("create run");
        let base_root = run.base_root();
        let staging_root = run.staging_root();

        // graph.json：base 和 local 都含 n1，incoming 删除 n1
        let graph_with_n1 = make_graph_meta_bytes("sm1", &["n1"], &[("n1", 1)], 1, 100);
        let graph_without_n1 = make_graph_meta_bytes("sm1", &[], &[], 2, 200);

        let node_content = br#"{"id":"n1","kind":"text"}"#;

        // live（local）有 graph.json 和 n1.json
        write_file(&live_root, "starmaps/sm1/graph.json", &graph_with_n1);
        write_file(&live_root, "starmaps/sm1/nodes/00/n1.json", node_content);

        // base 有相同的文件
        write_file(&base_root, "starmaps/sm1/graph.json", &graph_with_n1);
        write_file(&base_root, "starmaps/sm1/nodes/00/n1.json", node_content);

        // staging（incoming）有 graph.json（不含 n1），没有 n1.json
        write_file(&staging_root, "starmaps/sm1/graph.json", &graph_without_n1);

        let plan = run
            .compute_commit_plan(&live_root)
            .expect("compute commit plan");

        // 检查 content_actions 含 Delete { rel_path: "starmaps/sm1/nodes/00/n1.json" }
        let has_delete = plan.content_actions.iter().any(|action| match action {
            CommitAction::Delete { rel_path } => {
                rel_path.to_string_lossy().replace('\\', "/") == "starmaps/sm1/nodes/00/n1.json"
            }
            _ => false,
        });
        assert!(
            has_delete,
            "content_actions should contain Delete for n1.json, got: {:?}",
            plan.content_actions
        );
    }

    // --- 问题3 集成回归：整图删除时 content_actions 含 graph.json Delete ---

    #[test]
    fn test_issue902_entire_graph_delete_emits_commit_action_delete() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let live_root = tmp.path().join("live");
        fs::create_dir_all(&live_root).expect("create live root");

        let run_parent = tmp.path().join("runs");
        fs::create_dir_all(&run_parent).expect("create run parent");

        let run = StagingRun::create(&run_parent, live_root.clone()).expect("create run");
        let base_root = run.base_root();
        let _staging_root = run.staging_root();

        let graph_with_n1 = make_graph_meta_bytes("sm1", &["n1"], &[("n1", 1)], 1, 100);
        let node_content = br#"{"id":"n1","kind":"text"}"#;

        // live（local）有 graph.json 和 n1.json（未改）
        write_file(&live_root, "starmaps/sm1/graph.json", &graph_with_n1);
        write_file(&live_root, "starmaps/sm1/nodes/00/n1.json", node_content);

        // base 有相同的文件
        write_file(&base_root, "starmaps/sm1/graph.json", &graph_with_n1);
        write_file(&base_root, "starmaps/sm1/nodes/00/n1.json", node_content);

        // staging（incoming）没有 graph.json 和 n1.json（整图删除）

        let plan = run
            .compute_commit_plan(&live_root)
            .expect("compute commit plan");

        // 检查 content_actions 含 Delete { rel_path: "starmaps/sm1/graph.json" }
        let has_graph_delete = plan.content_actions.iter().any(|action| match action {
            CommitAction::Delete { rel_path } => {
                rel_path.to_string_lossy().replace('\\', "/") == "starmaps/sm1/graph.json"
            }
            _ => false,
        });
        assert!(
            has_graph_delete,
            "content_actions should contain Delete for graph.json, got: {:?}",
            plan.content_actions
        );

        // 检查 content_actions 含 Delete { rel_path: "starmaps/sm1/nodes/00/n1.json" }
        let has_node_delete = plan.content_actions.iter().any(|action| match action {
            CommitAction::Delete { rel_path } => {
                rel_path.to_string_lossy().replace('\\', "/") == "starmaps/sm1/nodes/00/n1.json"
            }
            _ => false,
        });
        assert!(
            has_node_delete,
            "content_actions should contain Delete for n1.json, got: {:?}",
            plan.content_actions
        );
    }
}
