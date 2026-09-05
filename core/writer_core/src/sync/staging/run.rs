use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::Result;
use uuid::Uuid;

use super::commit_plan::*;
use super::resolve::*;

const BASE_SUBDIR: &str = "base";
const STAGING_SUBDIR: &str = "staging";

/// 一次 full sync 的隔离 staging run。
///
/// `run_root` = `<parent>/<run_id>`，下含 `base/`（Prepare 时 live 快照）与
/// `staging/`（Transfer 后远端内容）。run 结束调 [StagingRun::cleanup] 整体删除。
///
/// #645 评论 5504296097 第2点：`StagingRun` 不再携带 `active_provider` /
/// `git_seed_state` / `git_layout` 字段。staging 统一走文件级 `seed_from_live`，
/// 不再按 Git/GithubApi backend 走不同 seed 路径。workspace Git 由 bootstrap 阶段
/// 初始化，staging 不负责 Git 生命周期。
pub struct StagingRun {
    run_root: PathBuf,
    run_id: String,
    /// Commit 阶段需要知道这个 staging run 对应的 live root，
    /// 才能调 `compute_commit_plan(live_root)` 做三方比较并把变更写回 live。
    target_live_root: PathBuf,
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
        })
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

    /// staging 子目录（Transfer 阶段写入远端内容的目标）。
    pub fn staging_root(&self) -> PathBuf {
        self.run_root.join(STAGING_SUBDIR)
    }

    /// base 子目录（Prepare 阶段从 live 建 snapshot）。
    pub fn base_root(&self) -> PathBuf {
        self.run_root.join(BASE_SUBDIR)
    }

    /// 从 live 建 base snapshot：对每个 `rel_path`，把 `live_root/rel_path`
    /// hard-link 到 `base/rel_path`；hard-link 失败（跨文件系统等）回退 copy。
    /// live 文件不存在时跳过（base 也不留，表示该文件 Prepare 时不存在）。
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
            // 能 hard-link 就 hard-link（同文件系统零拷贝、inode 共享）。
            if fs::hard_link(&live_path, &base_path).is_err() {
                // 回退 copy。
                fs::copy(&live_path, &base_path)?;
            }
        }
        Ok(())
    }

    /// #644 评论 5473105049 第1节：从 live 完整初始化 staging run。
    ///
    /// 1. 递归扫描 `live_root` 下所有文件（跳过 `.git/`、`full-sync-staging/`、
    ///    `app-meta/transactions/`），把每个文件 hard-link/copy 到 `base/`；
    /// 2. 把同一份文件也 hard-link/copy 到 `staging/`（Transfer 在这份克隆上执行）；
    /// 3. 把 live 的 sync state 和 manifest 复制到 `staging/`，让 LWW 在 staging 里
    ///    能读到 live 的 known_files / device_id / pending / conflict 状态，
    ///    不会重新生成设备身份。
    pub fn seed_from_live(&self, live_root: &Path) -> Result<()> {
        let rel_paths = list_live_file_paths(live_root)?;
        // 1. base snapshot
        self.build_base_snapshot_from_live(live_root, &rel_paths)?;
        // 2. staging clone（Transfer 在这份上执行）
        for rel in &rel_paths {
            let live_path = live_root.join(rel);
            let staging_path = self.staging_root().join(rel);
            if let Some(parent) = staging_path.parent() {
                fs::create_dir_all(parent)?;
            }
            if fs::hard_link(&live_path, &staging_path).is_err() {
                fs::copy(&live_path, &staging_path)?;
            }
        }
        Ok(())
    }

    /// 三方比较生成 commit plan。
    ///
    /// #644 评论 5473105049 第2节：按 `base ∪ staging` 的路径全集做真正的三方比较，
    /// 值统一用 `Option<bytes>`。`incoming=None` 表示远端删除。
    ///
    /// - `base` = Prepare 时 live（[Self::base_root] 下）
    /// - `local` = 现在 live（`live_root` 下）
    /// - `incoming` = Transfer 后 staging（[Self::staging_root] 下）
    ///
    /// #644 评论 5473789298 第3节：按 [`ContentClass`] 分类决策，不再统一字节比较：
    /// - [`ContentClass::UserTextDocument`]：走 [`three_way_resolve`]，
    ///   `BothChanged` → [`StagingConflict`]；其余按 NoOp/KeepLocal/Apply。
    /// - [`ContentClass::Metadata`] / [`ContentClass::GeneratedCache`]：真正 LWW
    ///   （#644 评论 5474166587 问题3：时间戳 + device_id 决胜，不再固定 remote-wins）。
    /// - [`ContentClass::LocalOnly`]：按 [`StagingCommitClass`] 进一步细分——
    ///   EngineState 写回 live，PlatformConfig/Skip 不写回。
    ///
    /// #644 评论 5474166587 问题1：CommitPlan 拆 `content_actions` + `engine_state_actions`。
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

        let staging_root = self.staging_root();
        let base_root = self.base_root();
        let mut plan = CommitPlan::default();

        // 读取 live 的 device_id（用于 LWW 决胜）。读取失败时回退空字符串，
        // 退化为纯时间戳比较（仍优于固定 remote-wins）。
        let live_device_id = read_live_device_id(live_root).unwrap_or_default();

        // #644 评论 5475110422 第4节：加载 live 的 SyncState，获取 tombstones。
        // delete 参与 LWW 时需要 tombstones 里的 deleted_at 时间戳。
        let live_sync_state = read_live_sync_state(live_root);
        let live_tombstones = live_sync_state
            .as_ref()
            .map(|s| s.tombstones.clone())
            .unwrap_or_default();

        // #644 评论 5474772497 第2节：读取 staging 的 manifest.sync.json，
        // 获取远端文件的真实 LWW 元数据（updated_at_ms、device_id、op）。
        // Transfer 阶段会写入 manifest；manifest 不存在或解析失败时回退到
        // mtime-based LWW（空 HashMap），不再区分 Git/GithubApi backend 的不同错误处理。
        //
        // #645 评论 5504296097 第2点：staging 不再按 active_provider 分支。
        // 通用 Provider 的 manifest 是 LWW 决策依据但不是硬性事实来源——
        // 缺失或损坏时回退到 mtime-based LWW，让同步继续而不是直接失败。
        let staging_manifest = match read_staging_manifest(&staging_root) {
            Ok(Some(map)) => map,
            Ok(None) => {
                // manifest 不存在 → mtime fallback（空 HashMap）。
                std::collections::HashMap::new()
            }
            Err(e) => {
                // manifest 解析失败 → warn + mtime fallback（空 HashMap）。
                log::warn!(
                    "compute_commit_plan: manifest parse failed ({}), \
                     falling back to mtime-based LWW",
                    e
                );
                std::collections::HashMap::new()
            }
        };

        // 收集 base ∪ staging ∪ live 的路径全集。
        // #644 评论 5473789298 第2节：base 和 staging 都走 list_commit_candidate_paths，
        // 排除 `.git/`、`full-sync-staging/`、`app-meta/transactions/`，
        // 不让 Git 元数据被当成正文比较。
        // #645 评论 5504296097 问题2：加入 live_paths — ReplaceProject 整树替换时
        // live-only 旧文件需要进入 commit plan 做 Delete。普通三方 commit 时
        // live-only 文件不在 base/staging 中，不会被处理（保持原行为）。
        let base_paths = list_commit_candidate_paths(&base_root)?;
        let staging_paths = list_commit_candidate_paths(&staging_root)?;
        let live_paths = list_commit_candidate_paths(live_root)?;
        let mut all_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for p in &base_paths {
            all_paths.insert(p.clone());
        }
        for p in &staging_paths {
            all_paths.insert(p.clone());
        }
        for p in &live_paths {
            all_paths.insert(p.clone());
        }

        for rel in all_paths {
            let rel_str = rel.to_string_lossy().to_string();

            // #644 评论 5474166587 问题1：按 StagingCommitClass 决定 staging commit 写回语义。
            // 远端同步语义（ContentClass）和 staging commit 写回语义（StagingCommitClass）
            // 是两个正交维度，不再复用 LocalOnly。
            match classify_staging_commit_path(&rel_str) {
                StagingCommitClass::Skip => {
                    // .git/、full-sync-staging/、app-meta/transactions/、
                    // config.local.json、secrets：永不进 commit。
                    continue;
                }
                StagingCommitClass::EngineState => {
                    // app-meta/sync/manifest.sync.json、app-meta/sync/state.local.json、
                    // app-meta/sync/conflicts.json：Transfer 在 staging 里更新了它们，
                    // Commit 必须写回 live。直接 apply incoming（无三方比较——这些是
                    // 同步引擎自己产生的状态，staging 里的就是最新权威值）。
                    let staging_path = staging_root.join(&rel);
                    let incoming: Option<Vec<u8>> = if staging_path.exists() {
                        Some(read_bytes(&staging_path)?)
                    } else {
                        None
                    };
                    apply_incoming(&mut plan, rel, incoming, StagingCommitClass::EngineState);
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
                        plan.conflict.push(StagingConflict {
                            rel_path: rel,
                            base_hash,
                            local_hash,
                            incoming_hash,
                        });
                    }
                }
            } else {
                // #644 评论 5474166587 问题3：Metadata/GeneratedCache 走真正 LWW——
                // 时间戳较大方获胜；同时间 device_id 字典序决胜。不再固定 remote-wins。
                let content_class = classify_content_path(&rel_str);
                debug_assert!(
                    content_class == ContentClass::Metadata
                        || content_class == ContentClass::GeneratedCache,
                    "非正文类路径必须是 Metadata 或 GeneratedCache，got {:?} for {}",
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
                    // #644 评论 5474772497 第2节：本地侧用 live 文件 hash + mtime + device_id；
                    // 远端侧优先从 staging manifest 读取真实 LWW 元数据
                    // （updated_at_ms、device_id、op），回退到 mtime-based。
                    //
                    // #644 评论 5475110422 第4节：delete 时从 tombstones 查找 deleted_at。
                    // 若本地 delete 无 tombstone 记录，跳过此路径（不参与 LWW）。
                    let local_rec = build_local_lww_record(
                        live_root,
                        &rel,
                        &local,
                        &live_device_id,
                        &live_tombstones,
                    );
                    let Some(local_rec) = local_rec else {
                        // 本地 delete 无 tombstone → 无法确定删除时间，
                        // 保留本地（不覆盖），下次同步时应补 tombstone。
                        plan.keep_local.push(rel);
                        continue;
                    };
                    let rel_str = rel.to_string_lossy().to_string();
                    let remote_rec = if let Some(manifest_rec) = staging_manifest.get(&rel_str) {
                        build_remote_lww_record_from_manifest(manifest_rec, &incoming)
                    } else {
                        // manifest 中无此路径（manifest 缺失），
                        // 回退到 mtime-based LWW。远端侧不需要 tombstones。
                        build_local_lww_record(&staging_root, &rel, &incoming, "remote", &[])
                            .unwrap_or_else(|| {
                                // 远端 delete 无 tombstone → 用当前时间兜底。
                                use crate::sync::content_class::LwwRecord;
                                LwwRecord {
                                    content_hash: String::new(),
                                    updated_at_ms: 0,
                                    deleted_at_ms: Some(0),
                                    device_id: "remote".to_string(),
                                    op: "delete".to_string(),
                                }
                            })
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
        Ok(plan)
    }

    /// 清理整个 run 目录。
    pub fn cleanup(&self) {
        let _ = fs::remove_dir_all(&self.run_root);
    }
}

/// #644 评论 5473401065 第1/2节：在**无 Core 锁**状态下创建并 seed 所有 staging runs。
///
/// 纯函数，不依赖 `WriterCore`，可在无锁状态下调用。
/// 对每个 target 创建 `StagingRun`，统一调 `seed_from_live`（文件级复制，跳过 `.git/`）。
///
/// #644 评论 5473401065 第2节：seed 失败**不再**被 `log::warn!` 吞掉。
/// 任何一个 target 的 seed 失败都意味着该 target 的 staging 是半成品，
/// 不能拿来做三方比较。seed 失败直接返回 Err，让调用方终止本次 full sync。
///
/// #645 评论 5504296097 第2点：不再在此函数中初始化 workspace Git。
/// 本地 Git 仓库由 bootstrap 阶段初始化，staging 只负责文件级快照。
/// 不再按 `active_provider` 分 Git/GithubApi 走不同 seed 路径。
///
/// 成功后 `plan.targets[*].staging_root` 被填充为对应 staging 目录。
pub fn prepare_staging_runs(
    plan: &mut crate::sync::full_sync::FullSyncPlan,
) -> crate::error::Result<Vec<StagingRun>> {
    let mut staging_runs: Vec<StagingRun> = Vec::new();

    for planned in &mut plan.targets {
        let run = StagingRun::create(&plan.app_data_root, planned.target_live_root.clone())?;
        // #645 评论 5504296097 问题1：deleted target 不 seed（不读本地目录，
        // 只枚举远端删除）。创建空 staging run 保持索引对齐，commit 阶段
        // compute_commit_plan 会发现 staging 为空，自然 Skip。
        if !planned.is_deleted_target() {
            // #644 评论 5473401065 第2节：seed 失败必须传播，不能继续拿半成品 staging。
            // #645 评论 5504296097 第2点：统一调 seed_from_live，不再按 backend 分支。
            run.seed_from_live(&planned.target_live_root)?;
        }
        planned.staging_root = Some(run.staging_root());
        staging_runs.push(run);
    }

    Ok(staging_runs)
}

impl Drop for StagingRun {
    fn drop(&mut self) {
        // 兜底清理：commit 正常路径会显式 cleanup，这里防止提前 drop 残留。
        let _ = fs::remove_dir_all(&self.run_root);
    }
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

/// #644 评论 5473789298 第2节：列出 base/staging 下参与 commit 比较的候选路径。
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
/// - `.git`（文件）：评论 5491531984 问题2 — gitlink file，同样不是用户内容；
/// - `.git.sujian-tmp-*`：迁移/恢复过程中的临时目录；
/// - `.git.sujian-migrate-source-*`：迁移崩溃后残留的源仓库快照；
/// - `full-sync-staging/`：staging run 自身，避免递归；
/// - `app-meta/transactions/`：事务暂存目录，commit 中间态。
///
/// #645 评论 5504296097 问题1：底层规则统一到
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
