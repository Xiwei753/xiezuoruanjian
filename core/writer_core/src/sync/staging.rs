//! #644 评论 5462823517 第2/4节：full sync staging run。
//!
//! 只负责 staging：创建 run 目录、从 live 建 base snapshot、比较 base/live/staging、
//! 生成 commit plan、清理 run。能 hard-link 就 hard-link，失败回退 copy。
//!
//! 三段式 full sync 里：
//! - **Prepare**：调 [StagingRun::create] 建隔离 run 目录，再调
//!   [StagingRun::build_base_snapshot_from_live] 把每个 live 文件 hard-link/copy 进
//!   `base/` 子目录，记录 base hash。
//! - **Transfer**：网络阶段把远端内容写进 `staging/` 子目录（不碰 live）。
//! - **Commit**：调 [StagingRun::compute_commit_plan] 做三方判断
//!   （base=Prepare 时 live、local=现在 live、incoming=Transfer 后 staging），
//!   生成 [CommitPlan]，再用 [crate::storage::transaction::SaveTransaction] 提交。
//!
//! 本模块不持 Core 锁、不做网络、不写 live 文件（commit 由调用方用 SaveTransaction 落盘）。

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::Result;
use uuid::Uuid;

const BASE_SUBDIR: &str = "base";
const STAGING_SUBDIR: &str = "staging";

/// 一次 full sync 的隔离 staging run。
///
/// `run_root` = `<parent>/<run_id>`，下含 `base/`（Prepare 时 live 快照）与
/// `staging/`（Transfer 后远端内容）。run 结束调 [StagingRun::cleanup] 整体删除。
pub struct StagingRun {
    run_root: PathBuf,
    run_id: String,
    /// Commit 阶段需要知道这个 staging run 对应的 live root，
    /// 才能调 `compute_commit_plan(live_root)` 做三方比较并把变更写回 live。
    target_live_root: PathBuf,
    /// #644 评论 5474772497 第1节 + #644 评论 5475110422 第1节：
    /// seed 时记录的 live Git 仓库状态。
    /// 仅 Git backend 有值；GithubApi backend 保持 None。
    /// `None` 只表示本 target 根本不是 Git backend。
    git_seed_state: Option<crate::sync::git_staging::GitSeedState>,
    /// #644 评论 5476546134 第5节：resolved backend type，
    /// 不再靠 `git_seed_state.is_some()` 间接猜 backend。
    backend_type: crate::sync::types::BackendType,
    /// #644 评论 5491531984 问题1：target 的 Git 仓库布局。
    /// Seed/Commit 使用 layout 指定的 git_dir 而非从 target_live_root 猜路径。
    git_layout: Option<crate::storage::git_repo_layout::GitRepoLayout>,
}

impl StagingRun {
    /// 创建隔离 run 目录。`parent` 通常在 app-data 下（不进 live project root），
    /// 避免被 scanner 当成作品文件。`target_live_root` 记录 Commit 阶段要写回的 live 根。
    /// `git_layout` 记录 target 的 Git 仓库布局（可选）。
    pub fn create(
        parent: &Path,
        target_live_root: PathBuf,
        backend_type: crate::sync::types::BackendType,
        git_layout: Option<crate::storage::git_repo_layout::GitRepoLayout>,
    ) -> Result<Self> {
        let run_id = Uuid::new_v4().to_string();
        let run_root = parent.join("full-sync-staging").join(&run_id);
        fs::create_dir_all(run_root.join(BASE_SUBDIR))?;
        fs::create_dir_all(run_root.join(STAGING_SUBDIR))?;
        Ok(Self {
            run_root,
            run_id,
            target_live_root,
            git_seed_state: None,
            backend_type,
            git_layout,
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

    /// #644 评论 5474772497 第1节 + #644 评论 5475110422 第1节：
    /// seed 时记录的 live Git 仓库状态。
    /// 仅 Git backend 有值；GithubApi backend 保持 None。
    pub fn git_seed_state(&self) -> Option<&crate::sync::git_staging::GitSeedState> {
        self.git_seed_state.as_ref()
    }

    /// #644 评论 5474772497 第1节 + #644 评论 5475110422 第1节：
    /// 设置 seed 时记录的 live Git 仓库状态。
    pub fn set_git_seed_state(&mut self, state: crate::sync::git_staging::GitSeedState) {
        self.git_seed_state = Some(state);
    }

    /// #644 评论 5491531984 问题1：target 的 Git 仓库布局。
    pub fn git_layout(&self) -> Option<&crate::storage::git_repo_layout::GitRepoLayout> {
        self.git_layout.as_ref()
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
        // GithubApi backend 的 Transfer 阶段会写入 manifest；
        // Git backend 不产生 manifest，此映射为空，回退到 mtime-based LWW。
        //
        // #644 评论 5475805198 第4节 + #644 评论 5476546134 第5节：
        // - Git backend：manifest 不存在 → mtime fallback；解析失败 → warn + mtime fallback。
        // - GithubApi backend：manifest 不存在 → Err；解析失败 → Err。
        //   manifest 是 GithubApi 的事实来源，坏了就应该让 target 失败。
        let is_git_backend = matches!(self.backend_type, crate::sync::types::BackendType::Git);
        let staging_manifest = match read_staging_manifest(&staging_root) {
            Ok(Some(map)) => map,
            Ok(None) => {
                if is_git_backend {
                    // Git backend：manifest 不是决策依据，mtime fallback。
                    std::collections::HashMap::new()
                } else {
                    // GithubApi backend：manifest 不存在是错误。
                    return Err(crate::Error::Io(std::io::Error::other(
                        "compute_commit_plan: GithubApi backend but no manifest.sync.json, \
                         cannot proceed without LWW metadata",
                    )));
                }
            }
            Err(e) => {
                if is_git_backend {
                    // Git backend：manifest 不是决策依据，警告 + mtime fallback。
                    log::warn!(
                        "compute_commit_plan: manifest parse failed ({}), \
                         Git backend falling back to mtime-based LWW",
                        e
                    );
                    std::collections::HashMap::new()
                } else {
                    // GithubApi backend：manifest 是事实来源，解析失败直接 Err。
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "compute_commit_plan: GithubApi manifest parse failed: {}",
                        e
                    ))));
                }
            }
        };

        // 收集 base ∪ staging 的路径全集。
        // #644 评论 5473789298 第2节：base 和 staging 都走 list_commit_candidate_paths，
        // 排除 `.git/`、`full-sync-staging/`、`app-meta/transactions/`，
        // 不让 Git 元数据被当成正文比较。
        let base_paths = list_commit_candidate_paths(&base_root)?;
        let staging_paths = list_commit_candidate_paths(&staging_root)?;
        let mut all_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for p in &base_paths {
            all_paths.insert(p.clone());
        }
        for p in &staging_paths {
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
                        // manifest 中无此路径（Git backend 或 manifest 缺失），
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
/// 对每个 target 创建 `StagingRun`，然后根据 backend 类型选择对应准备方式：
/// - `GithubApi`：调 `seed_from_live`（文件级复制，跳过 `.git/`）；
/// - `Git`：调 `seed_from_live_as_git_repo`（`git clone --local`，保留仓库身份）。
///
/// #644 评论 5473401065 第2节：seed 失败**不再**被 `log::warn!` 吞掉。
/// 任何一个 target 的 seed 失败都意味着该 target 的 staging 是半成品，
/// 不能拿来做三方比较。seed 失败直接返回 Err，让调用方终止本次 full sync。
///
/// #644 评论 5473551127 第2节：Git 后端不能共用"复制业务文件但删掉 .git"的 staging，
/// 否则 `SyncService::perform_sync` 在 staging 里判断 `has_repo=false`，
/// 把已有仓库历史/HEAD/remote 语义全部丢掉。
///
/// #644 评论 5493295108 问题1/2：在 seed 前先解析/迁移每个 target 的 Git layout。
/// - App target：调 `resolve_existing_repo_layout`（只处理已有仓库位置，不 init 新仓库）。
/// - Project target：调 `ensure_project_repo_with_layout`（产品契约要求作品必有 Git）。
///
/// 这样迁移发生在已释放 Core 写锁之后，不会堵住冷启动卷章读取。
///
/// 成功后 `plan.targets[*].staging_root` 被填充为对应 staging 目录。
pub fn prepare_staging_runs(
    plan: &mut crate::sync::full_sync::FullSyncPlan,
    backend_type: &crate::sync::types::BackendType,
) -> crate::error::Result<Vec<StagingRun>> {
    let mut staging_runs: Vec<StagingRun> = Vec::new();

    for planned in &mut plan.targets {
        // #644 评论 5493295108 问题1/2：在 seed 前先解析/迁移 Git layout。
        // 这一步在已释放 Core 写锁之后执行，不会堵住冷启动卷章读取。
        if let Some(layout) = &planned.git_layout {
            prepare_target_git_layout(planned, backend_type, layout)?;
        }

        let mut run = StagingRun::create(
            &plan.app_data_root,
            planned.target_live_root.clone(),
            backend_type.clone(),
            planned.git_layout.clone(),
        )?;
        // #644 评论 5473401065 第2节：seed 失败必须传播，不能继续拿半成品 staging。
        // #644 评论 5473551127 第2节：按 backend 类型选择对应 seed 方式。
        match backend_type {
            crate::sync::types::BackendType::Git => {
                // #644 评论 5473789298 第1节：Git 专属 staging 移到 git_staging.rs。
                // #644 评论 5474772497 第1节 + #644 评论 5475110422 第1节：
                // seed 返回 live 的 GitSeedState，供 finalize 决定路径。
                // #644 评论 5491531984 问题1：传入 git_layout 用于正确打开仓库。
                let seed_state = crate::sync::git_staging::seed_from_live_as_git_repo(
                    &run,
                    &planned.target_live_root,
                    planned.git_layout.as_ref(),
                )?;
                run.set_git_seed_state(seed_state);
            }
            crate::sync::types::BackendType::GithubApi => {
                run.seed_from_live(&planned.target_live_root)?;
            }
        }
        planned.staging_root = Some(run.staging_root());
        staging_runs.push(run);
    }

    Ok(staging_runs)
}

/// #644 评论 5493295108 问题1/2：在 seed 前解析/迁移 target 的 Git layout。
///
/// - App target（`project_id == None`）：调 `resolve_existing_repo_layout`，
///   只处理已有仓库位置，不 init 新仓库。App data root 不应被误 init 成 Git repo。
/// - Project target（`project_id == Some`）：调 `ensure_project_repo_with_layout`，
///   产品契约要求作品必有 Git，missing repo 会被 init。
///
/// 这一步在已释放 Core 写锁之后执行（由 `prepare_staging_runs` 调用），
/// 不会堵住冷启动卷章读取。
#[allow(clippy::excessive_nesting)]
fn prepare_target_git_layout(
    planned: &crate::sync::full_sync::PlannedTarget,
    backend_type: &crate::sync::types::BackendType,
    layout: &crate::storage::git_repo_layout::GitRepoLayout,
) -> crate::error::Result<()> {
    // GithubApi backend 不需要 Git layout 迁移。
    if !matches!(backend_type, crate::sync::types::BackendType::Git) {
        return Ok(());
    }

    match planned.project_id {
        Some(_) => {
            // Project target：产品契约要求作品必有 Git。
            // ensure_project_repo_with_layout 会 init missing repo。
            crate::storage::git_repo_layout::ensure_project_repo_with_layout(layout)?;
        }
        None => {
            // App target：只处理已有仓库位置，不 init 新仓库。
            // resolve_existing_repo_layout 语义：
            // - private git_dir 已有 repo → Ready
            // - private 没有 + worktree/.git 有 repo → 迁移后 Ready
            // - 两边都没有 → NotGitRepo（不 init）
            let _ = crate::storage::git_repo_layout::resolve_existing_repo_layout(layout)?;
        }
    }
    Ok(())
}

impl Drop for StagingRun {
    fn drop(&mut self) {
        // 兜底清理：commit 正常路径会显式 cleanup，这里防止提前 drop 残留。
        let _ = fs::remove_dir_all(&self.run_root);
    }
}

/// Commit plan — Commit 阶段对每个 staging 变化的处理决策。
///
/// #644 评论 5474166587 问题1：拆 `content_actions` + `engine_state_actions`。
/// - `content_actions`：用户内容（正文、元数据、缓存）的写回动作。
/// - `engine_state_actions`：同步引擎自身状态（manifest.sync.json、
///   state.local.json、conflicts.json）的写回动作。
///
/// 两类最后用同一个 `SaveTransaction` 一次写回 live，不另起第二套保存路径。
#[derive(Default, Debug)]
pub struct CommitPlan {
    /// 用户内容写回动作：local==base 时安全应用 incoming（含 incoming 独有新增）。
    pub content_actions: Vec<CommitAction>,
    /// 引擎状态写回动作：app-meta/sync/manifest.sync.json、state.local.json、
    /// conflicts.json 等。Transfer 在 staging 里更新了它们，Commit 必须写回 live。
    pub engine_state_actions: Vec<CommitAction>,
    /// incoming==base，保留 local（无需动作，记录供诊断）。
    pub keep_local: Vec<PathBuf>,
    /// local==incoming，内容相同，无需操作。
    pub noop: Vec<PathBuf>,
    /// 两边都改，三方冲突（正文走三方合并语义，metadata 走 LWW，由调用方决定）。
    /// #644 评论 5473401065 第4节：用 `StagingConflict` 替代 `PathBuf`，
    /// 保留 base/local/incoming 哈希，让 Commit 阶段能映射成 `SyncConflict` 并持久化。
    pub conflict: Vec<StagingConflict>,
}

/// #644 评论 5473401065 第4节：三方冲突的完整信息。
///
/// 保留 `rel_path` + 三方哈希，Commit 阶段映射成 `SyncConflict` 时不再丢失信息。
#[derive(Debug, Clone)]
pub struct StagingConflict {
    pub rel_path: PathBuf,
    pub base_hash: String,
    pub local_hash: String,
    pub incoming_hash: String,
}

/// 单个文件的 commit 动作。
#[derive(Debug, Clone)]
pub enum CommitAction {
    /// 把 `content` 写到 `rel_path`（相对 target_root）。
    Apply { rel_path: PathBuf, content: Vec<u8> },
    /// 删除 `rel_path`（远端删除，local 没改）。
    Delete { rel_path: PathBuf },
}

/// #644 评论 5474166587 问题1：staging commit 写回语义分类。
///
/// 与 [`ContentClass`]（远端同步语义）正交。决定 Transfer 在 staging 里产生的
/// 哪些本地状态必须写回 live：
/// - `Content`：用户内容，走三方比较/LWW 决策。
/// - `EngineState`：同步引擎自身状态（manifest/state/conflicts），直接写回 live。
/// - `Skip`：永不进 commit（.git/、full-sync-staging/、app-meta/transactions/、
///   config.local.json、secrets）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagingCommitClass {
    /// 用户内容：正文、元数据、缓存。走三方比较/LWW 决策。
    Content,
    /// 引擎状态：manifest.sync.json、state.local.json、conflicts.json。
    /// Transfer 在 staging 里更新了它们，Commit 必须写回 live。
    EngineState,
    /// 永不进 commit：.git/、full-sync-staging/、app-meta/transactions/、
    /// config.local.json、secrets。
    Skip,
}

/// 判断路径是否为内部 Git 工件（不应被当成用户内容同步）。
///
/// 统一过滤以下模式：
/// - `.git`（精确匹配）：Git 仓库元数据目录或 gitlink 文件；
/// - `.git/`（前缀匹配）：Git 仓库元数据子目录；
/// - `.git.sujian-tmp-*`：迁移/恢复过程中的临时目录；
/// - `.git.sujian-migrate-source-*`：迁移崩溃后残留的源仓库快照。
fn is_internal_git_artifact(path: &str) -> bool {
    // Normalize backslashes to forward slashes for consistent matching on Windows
    let normalized = if path.contains('\\') {
        path.replace('\\', "/")
    } else {
        path.to_string()
    };

    // .git (exact) or .git/* (subdirectory)
    if normalized == ".git" || normalized.starts_with(".git/") {
        return true;
    }

    // .git.sujian-tmp-* (migration temp directory)
    if normalized.starts_with(".git.sujian-tmp-") {
        return true;
    }

    // .git.sujian-migrate-source-* (migration crash residual)
    if normalized.starts_with(".git.sujian-migrate-source-") {
        return true;
    }

    false
}

/// #644 评论 5474166587 问题1：按 staging commit 写回语义分类。
///
/// 与 [`crate::sync::content_class::classify_content_path`]（远端同步语义）正交。
/// `app-meta/` 下只有 `sync/manifest.sync.json`、`sync/state.local.json`、
/// `sync/conflicts.json` 是 EngineState，其余 app-meta 内容（如 transactions/、
/// logs/）不进 commit。
pub(crate) fn classify_staging_commit_path(raw_path: &str) -> StagingCommitClass {
    // Normalize backslashes to forward slashes for consistent matching on Windows
    let path = if raw_path.contains('\\') {
        std::borrow::Cow::Owned(raw_path.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(raw_path)
    };

    // 永不进 commit 的内部目录（walk_commit_candidates 已跳过，这里兜底）。
    // 使用统一过滤函数判断 Git 工件。
    if is_internal_git_artifact(&path) {
        return StagingCommitClass::Skip;
    }
    if path.starts_with("full-sync-staging/") || path == "full-sync-staging" {
        return StagingCommitClass::Skip;
    }
    if path.starts_with("app-meta/transactions/") {
        return StagingCommitClass::Skip;
    }

    // EngineState：同步引擎自身状态，Transfer 在 staging 里更新了它们，Commit 必须写回 live。
    if path == "app-meta/sync/manifest.sync.json"
        || path == "app-meta/sync/state.local.json"
        || path == "app-meta/sync/conflicts.json"
    {
        return StagingCommitClass::EngineState;
    }

    // 平台配置/凭证：不从 staging 覆盖 live（设备专属）。
    if path == "app-meta/sync/config.local.json" || path.starts_with("app-meta/sync/secrets") {
        return StagingCommitClass::Skip;
    }

    // 其余 app-meta/ 内容（logs/、stats/ 等）不进 commit。
    if path.starts_with("app-meta/") {
        return StagingCommitClass::Skip;
    }

    StagingCommitClass::Content
}

/// 比较两个 `Option<Vec<u8>>` 是否相等。
/// `None == None` → true，`None == Some(_)` → false。
fn opt_bytes_eq(a: &Option<Vec<u8>>, b: &Option<Vec<u8>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a == b,
        (None, None) => true,
        _ => false,
    }
}

/// #644 评论 5473789298 第3节：把 incoming 内容推入 plan 的 actions 列表。
///
/// `incoming = Some` → [`CommitAction::Apply`]；`incoming = None`（远端删除）→
/// [`CommitAction::Delete`]。按 `class` 决定推入 `content_actions` 还是
/// `engine_state_actions`。
fn apply_incoming(
    plan: &mut CommitPlan,
    rel: PathBuf,
    incoming: Option<Vec<u8>>,
    class: StagingCommitClass,
) {
    let action = match incoming {
        Some(content) => CommitAction::Apply {
            rel_path: rel,
            content,
        },
        None => CommitAction::Delete { rel_path: rel },
    };
    match class {
        StagingCommitClass::EngineState => plan.engine_state_actions.push(action),
        StagingCommitClass::Content => plan.content_actions.push(action),
        StagingCommitClass::Skip => {
            // classify_staging_commit_path 已过滤 Skip，不应到达此处。
            // 防御性丢弃，不写回 live。
        }
    }
}

/// #644 评论 5474166587 问题3：从文件系统构造本地侧 LWW 记录。
///
/// 读取文件 mtime 作为 `updated_at_ms`；`op` 按内容是否存在决定（Some → upsert，
/// None → delete）。`device_id` 用 live 的 device_id。
///
/// #644 评论 5475110422 第4节：delete 时从 `tombstones` 查找 `deleted_at`，
/// 不再固定写 0。若 tombstones 中无记录，返回 `None`（调用方应报错或补 tombstone）。
fn build_local_lww_record(
    root: &Path,
    rel: &Path,
    content: &Option<Vec<u8>>,
    device_id: &str,
    tombstones: &[crate::sync::types::Tombstone],
) -> Option<crate::sync::content_class::LwwRecord> {
    use crate::sync::content_class::LwwRecord;

    let rel_str = rel.to_string_lossy().to_string();

    let (content_hash, op, updated_at_ms, deleted_at_ms) = match content {
        Some(bytes) => {
            let hash = md5_hex(&Some(bytes.clone()));
            let mtime = read_mtime_ms(root, rel).unwrap_or(0);
            (hash, "upsert", mtime, None)
        }
        None => {
            // #644 评论 5475110422 第4节：从 tombstones 查找删除时间。
            // 没有 tombstone 记录 → 无法确定删除时间，返回 None。
            let ts = tombstones.iter().find(|t| t.original_path == rel_str)?;
            let deleted_at = ts.deleted_at * 1000; // tombstone.deleted_at 是秒，LWW 用毫秒
            (String::new(), "delete", deleted_at, Some(deleted_at))
        }
    };

    Some(LwwRecord {
        content_hash,
        updated_at_ms,
        deleted_at_ms,
        device_id: device_id.to_string(),
        op: op.to_string(),
    })
}

/// #644 评论 5474772497 第2节：staging manifest 中的文件记录（JSON 反序列化用）。
///
/// 与 `types::ManifestFileRecord` 字段一致，但不依赖 `github-api` feature gate。
/// 仅用于从 staging 的 `manifest.sync.json` 读取远端 LWW 元数据。
#[derive(Debug, Clone, serde::Deserialize)]
struct ManifestRecord {
    path: String,
    #[serde(rename = "content_hash")]
    _content_hash: String,
    updated_at_ms: i64,
    #[serde(default)]
    deleted_at_ms: Option<i64>,
    device_id: String,
    op: String,
}

/// #644 评论 5474772497 第2节：从 staging 的 manifest.sync.json 读取远端 LWW 记录。
///
/// #644 评论 5475805198 第4节：改为 `Result<Option<...>>`。
/// - manifest 不存在 → `Ok(None)`（Git backend 不产生 manifest，mtime fallback）。
/// - manifest 存在且解析成功 → `Ok(Some(map))`。
/// - manifest 存在但读/解析失败 → `Err`（GithubApi backend 的 manifest 是事实来源，
///   解析失败不能静默切成另一套决策规则）。
///
/// 调用方根据 `is_github_api` 决定是否允许 `None`（mtime fallback）。
fn read_staging_manifest(
    staging_root: &Path,
) -> Result<Option<std::collections::HashMap<String, ManifestRecord>>> {
    let manifest_path = staging_root.join("app-meta/sync/manifest.sync.json");
    if !manifest_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "read_staging_manifest: failed to read manifest.sync.json: {}",
            e
        )))
    })?;
    #[derive(serde::Deserialize)]
    struct ManifestContainer {
        files: Vec<ManifestRecord>,
    }
    let container: ManifestContainer = serde_json::from_str(&content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "read_staging_manifest: manifest.sync.json parse error: {}",
            e
        )))
    })?;
    Ok(Some(
        container
            .files
            .into_iter()
            .map(|r| (r.path.clone(), r))
            .collect(),
    ))
}

/// #644 评论 5474772497 第2节：从 manifest 记录构造远端侧 LWW 记录。
///
/// 使用 manifest 中的真实 `updated_at_ms`、`device_id`、`op`，
/// 而非文件系统 mtime 和固定 "remote" 字符串。
fn build_remote_lww_record_from_manifest(
    manifest_record: &ManifestRecord,
    incoming_content: &Option<Vec<u8>>,
) -> crate::sync::content_class::LwwRecord {
    use crate::sync::content_class::LwwRecord;

    let content_hash = match incoming_content {
        Some(bytes) => md5_hex(&Some(bytes.clone())),
        None => String::new(),
    };

    LwwRecord {
        content_hash,
        updated_at_ms: manifest_record.updated_at_ms,
        deleted_at_ms: manifest_record.deleted_at_ms,
        device_id: manifest_record.device_id.clone(),
        op: manifest_record.op.clone(),
    }
}

/// 读取文件 mtime（Unix 毫秒），失败返回 None。
fn read_mtime_ms(root: &Path, rel: &Path) -> Option<i64> {
    let path = root.join(rel);
    std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .and_then(|t| {
            t.duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map_err(std::io::Error::other)
        })
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .ok()
}

/// 读取 live 的 device_id（从 app-meta/sync/state.local.json）。
///
/// 读取失败（文件不存在、JSON 损坏等）时返回 None，调用方回退空字符串，
/// LWW 退化为纯时间戳比较（仍优于固定 remote-wins）。
fn read_live_device_id(live_root: &Path) -> Option<String> {
    let state_path = live_root.join("app-meta/sync/state.local.json");
    if !state_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&state_path).ok()?;
    let state: crate::sync::types::SyncState = serde_json::from_str(&content).ok()?;
    if state.device_id.is_empty() {
        None
    } else {
        Some(state.device_id)
    }
}

/// #644 评论 5475110422 第4节：读取 live 的完整 SyncState。
///
/// 用于获取 tombstones（delete 的 deleted_at 时间戳）。
/// 读取失败时返回 None。
fn read_live_sync_state(live_root: &Path) -> Option<crate::sync::types::SyncState> {
    let state_path = live_root.join("app-meta/sync/state.local.json");
    if !state_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&state_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// 计算字节内容的 MD5 hex 摘要。`None`（文件不存在）返回空字符串。
fn md5_hex(content: &Option<Vec<u8>>) -> String {
    match content {
        Some(bytes) => {
            use std::io::Write;
            let mut hasher = md5::Context::new();
            hasher.write_all(bytes).ok();
            format!("{:x}", hasher.compute())
        }
        None => String::new(),
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
#[allow(clippy::excessive_nesting)]
fn walk_commit_candidates(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Ok(rel) = path.strip_prefix(root) {
            let rel_str = rel.to_string_lossy();
            // 使用统一过滤函数判断 Git 工件和内部目录
            if is_internal_git_artifact(&rel_str)
                || rel_str == "full-sync-staging"
                || rel_str.starts_with("app-meta/transactions")
            {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn staging_run_create_and_cleanup() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        let run = StagingRun::create(
            tmp.path(),
            live,
            crate::sync::types::BackendType::GithubApi,
            None,
        )
        .unwrap();
        assert!(run.base_root().exists());
        assert!(run.staging_root().exists());
        run.cleanup();
        assert!(!run.run_root().exists());
    }

    #[test]
    fn base_snapshot_hardlink_or_copy() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("a.txt"), "hello").unwrap();
        fs::create_dir_all(live.join("sub")).unwrap();
        fs::write(live.join("sub/b.txt"), "world").unwrap();

        let run = StagingRun::create(
            tmp.path(),
            live.clone(),
            crate::sync::types::BackendType::GithubApi,
            None,
        )
        .unwrap();
        run.build_base_snapshot_from_live(
            &live,
            &[
                PathBuf::from("a.txt"),
                PathBuf::from("sub/b.txt"),
                PathBuf::from("missing.txt"),
            ],
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(run.base_root().join("a.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            fs::read_to_string(run.base_root().join("sub/b.txt")).unwrap(),
            "world"
        );
        assert!(!run.base_root().join("missing.txt").exists());
    }

    #[test]
    fn commit_plan_local_eq_base_applies_incoming() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("f.txt"), "base").unwrap();

        let run = StagingRun::create(
            tmp.path(),
            live.clone(),
            crate::sync::types::BackendType::Git,
            None,
        )
        .unwrap();
        run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
            .unwrap();
        // local 仍是 base（没动），incoming 改了。
        fs::write(run.staging_root().join("f.txt"), "incoming").unwrap();

        let plan = run.compute_commit_plan(&live).unwrap();
        assert_eq!(plan.content_actions.len(), 1);
        assert!(plan.keep_local.is_empty());
        assert!(plan.conflict.is_empty());
    }

    #[test]
    fn commit_plan_incoming_eq_base_keeps_local() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("f.txt"), "base").unwrap();

        let run = StagingRun::create(
            tmp.path(),
            live.clone(),
            crate::sync::types::BackendType::Git,
            None,
        )
        .unwrap();
        run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
            .unwrap();
        // local 改了（走 atomic_write rename 替换，hard-link 的 base 保留旧 inode），
        // incoming == base。
        crate::storage::atomic_write_string(&live.join("f.txt"), "local-changed").unwrap();
        fs::write(run.staging_root().join("f.txt"), "base").unwrap();

        let plan = run.compute_commit_plan(&live).unwrap();
        assert_eq!(plan.keep_local.len(), 1);
        assert!(plan.content_actions.is_empty());
        assert!(plan.conflict.is_empty());
    }

    #[test]
    fn seed_from_live_copies_all_files_to_base_and_staging() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(live.join("sub")).unwrap();
        fs::write(live.join("a.txt"), "hello").unwrap();
        fs::write(live.join("sub/b.txt"), "world").unwrap();
        // 内部目录应被跳过
        fs::create_dir_all(live.join(".git/objects")).unwrap();
        fs::write(live.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
        fs::create_dir_all(live.join("app-meta/transactions/tx1")).unwrap();
        fs::write(live.join("app-meta/transactions/tx1/staged"), "tmp").unwrap();

        let run = StagingRun::create(
            tmp.path(),
            live.clone(),
            crate::sync::types::BackendType::GithubApi,
            None,
        )
        .unwrap();
        run.seed_from_live(&live).unwrap();

        // base 有业务文件
        assert_eq!(
            fs::read_to_string(run.base_root().join("a.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            fs::read_to_string(run.base_root().join("sub/b.txt")).unwrap(),
            "world"
        );
        // staging 也有业务文件
        assert_eq!(
            fs::read_to_string(run.staging_root().join("a.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            fs::read_to_string(run.staging_root().join("sub/b.txt")).unwrap(),
            "world"
        );
        // 内部目录被跳过
        assert!(!run.base_root().join(".git").exists());
        assert!(!run.staging_root().join(".git").exists());
        assert!(!run.base_root().join("app-meta/transactions").exists());
        assert!(!run.staging_root().join("app-meta/transactions").exists());
    }

    #[test]
    fn commit_plan_both_changed_is_conflict() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        // #644 评论 5473789298 第3节：UserTextDocument 双方都改才冲突。
        // 用 note.md（正文类）而非 .txt（GeneratedCache 走 LWW 不冲突）。
        fs::write(live.join("note.md"), "base").unwrap();

        let run = StagingRun::create(
            tmp.path(),
            live.clone(),
            crate::sync::types::BackendType::Git,
            None,
        )
        .unwrap();
        run.build_base_snapshot_from_live(&live, &[PathBuf::from("note.md")])
            .unwrap();
        // local 改了（atomic_write rename 替换，base 保留旧 inode）。
        crate::storage::atomic_write_string(&live.join("note.md"), "local-changed").unwrap();
        fs::write(run.staging_root().join("note.md"), "incoming-changed").unwrap();

        let plan = run.compute_commit_plan(&live).unwrap();
        assert_eq!(plan.conflict.len(), 1);
        assert!(plan.content_actions.is_empty());
        assert!(plan.keep_local.is_empty());
    }

    #[test]
    fn commit_plan_remote_delete_local_unchanged_produces_delete() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("f.txt"), "base").unwrap();

        let run = StagingRun::create(
            tmp.path(),
            live.clone(),
            crate::sync::types::BackendType::Git,
            None,
        )
        .unwrap();
        run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
            .unwrap();
        // 远端删除：staging 中没有 f.txt（incoming=None），local 没改。
        // base ∪ staging = {f.txt}（来自 base），三方比较：local==base, incoming=None → Delete。

        let plan = run.compute_commit_plan(&live).unwrap();
        assert_eq!(plan.content_actions.len(), 1);
        assert!(matches!(
            plan.content_actions[0],
            CommitAction::Delete { .. }
        ));
        assert!(plan.conflict.is_empty());
        assert!(plan.keep_local.is_empty());
    }

    #[test]
    fn commit_plan_remote_new_local_none_applies() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        // live 没有 f.txt（local=None），base 也没有（base=None）。
        // staging 有 f.txt（incoming=Some）。
        let run = StagingRun::create(
            tmp.path(),
            live.clone(),
            crate::sync::types::BackendType::Git,
            None,
        )
        .unwrap();
        // base 为空（没有 build_base_snapshot）
        fs::write(run.staging_root().join("f.txt"), "new-from-remote").unwrap();

        let plan = run.compute_commit_plan(&live).unwrap();
        // base=None, local=None, incoming=Some → local==base (both None), incoming!=base → Apply
        assert_eq!(plan.content_actions.len(), 1);
        assert!(plan.conflict.is_empty());
    }

    #[test]
    fn commit_plan_local_changed_remote_deleted_is_conflict() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        // #644 评论 5473789298 第3节：UserTextDocument local 改了 + 远端删除 → 冲突。
        // 用 note.md（正文类）而非 .txt（GeneratedCache 走 LWW：Apply Delete 不冲突）。
        fs::write(live.join("note.md"), "base").unwrap();

        let run = StagingRun::create(
            tmp.path(),
            live.clone(),
            crate::sync::types::BackendType::Git,
            None,
        )
        .unwrap();
        run.build_base_snapshot_from_live(&live, &[PathBuf::from("note.md")])
            .unwrap();
        // local 改了，远端删除（staging 没有 note.md）。
        crate::storage::atomic_write_string(&live.join("note.md"), "local-changed").unwrap();

        let plan = run.compute_commit_plan(&live).unwrap();
        // local!=base, incoming=None → local!=incoming → Conflict
        assert_eq!(plan.conflict.len(), 1);
        assert!(plan.content_actions.is_empty());
        assert!(plan.keep_local.is_empty());
    }

    #[test]
    fn walk_commit_candidates_skips_git_sujian_migrate_source() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // 创建测试目录结构
        fs::create_dir_all(root.join("normal")).unwrap();
        fs::write(root.join("normal/file.txt"), "normal").unwrap();
        // .git.sujian-migrate-source-* 目录应被跳过
        fs::create_dir_all(root.join(".git.sujian-migrate-source-abc/objects")).unwrap();
        fs::write(
            root.join(".git.sujian-migrate-source-abc/HEAD"),
            "ref: main",
        )
        .unwrap();
        fs::write(root.join(".git.sujian-migrate-source-abc/config"), "[core]").unwrap();
        // .git.sujian-tmp-* 目录也应被跳过
        fs::create_dir_all(root.join(".git.sujian-tmp-tmp123")).unwrap();
        fs::write(root.join(".git.sujian-tmp-tmp123/tmp"), "tmp").unwrap();
        // .git 目录也应被跳过
        fs::create_dir_all(root.join(".git/objects")).unwrap();
        fs::write(root.join(".git/HEAD"), "ref: main").unwrap();

        let mut out = Vec::new();
        walk_commit_candidates(root, root, &mut out).unwrap();

        // 只应看到 normal/file.txt
        assert_eq!(out.len(), 1);
        assert!(out.contains(&PathBuf::from("normal/file.txt")));
        // 不应包含任何内部 Git 工件
        assert!(!out.iter().any(|p| p.to_string_lossy().contains(".git")));
    }

    #[test]
    fn classify_staging_commit_path_skips_git_sujian_migrate_source() {
        // .git.sujian-migrate-source-* 应被分类为 Skip
        assert_eq!(
            classify_staging_commit_path(".git.sujian-migrate-source-abc"),
            StagingCommitClass::Skip
        );
        assert_eq!(
            classify_staging_commit_path(".git.sujian-migrate-source-abc/objects/HEAD"),
            StagingCommitClass::Skip
        );
        // Windows 路径格式也应正确处理
        assert_eq!(
            classify_staging_commit_path(".git.sujian-migrate-source-abc\\objects\\HEAD"),
            StagingCommitClass::Skip
        );

        // .git.sujian-tmp-* 也应被分类为 Skip
        assert_eq!(
            classify_staging_commit_path(".git.sujian-tmp-tmp123"),
            StagingCommitClass::Skip
        );
        assert_eq!(
            classify_staging_commit_path(".git.sujian-tmp-tmp123/tmp"),
            StagingCommitClass::Skip
        );

        // .git 也应被分类为 Skip
        assert_eq!(
            classify_staging_commit_path(".git"),
            StagingCommitClass::Skip
        );
        assert_eq!(
            classify_staging_commit_path(".git/objects/HEAD"),
            StagingCommitClass::Skip
        );

        // 普通内容应被分类为 Content
        assert_eq!(
            classify_staging_commit_path("normal/file.txt"),
            StagingCommitClass::Content
        );
    }
}
