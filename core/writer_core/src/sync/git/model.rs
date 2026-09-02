use serde::{Deserialize, Serialize};

// ── 快照类型 ──

/// #644 评论 5475805198 第2节 + #644 评论 5476546134 第4节：
/// 单个引用的快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RefSnapshot {
    /// 引用不存在（finalize 会创建它）。
    DidNotExist,
    /// 引用存在且是 direct OID。
    Existed { oid: String },
    /// 引用存在且是 symbolic（如 HEAD → refs/heads/main）。
    Symbolic { target: String },
}

/// #644 评论 5476546134 第4节：index 快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndexSnapshot {
    /// index 文件不存在。
    Missing,
    /// index 文件的原始字节。
    Bytes(Vec<u8>),
}

/// #644 评论 5475805198 第2节 + #644 评论 5476546134 第4节：
/// finalize 前的 Git metadata 快照。
///
/// 供崩溃恢复使用：进程在 `SaveTransaction.commit()` 和 `commit_git_finalize()`
/// 之间退出时，下次启动可通过本快照完成或回滚 Git metadata。
///
/// #644 评论 5476546134 第4节：不再混用 `head_snapshot + head_ref`。
/// `head` 只对应 HEAD 引用本身；`refs` 记录所有本轮会修改的 branch/remote refs。
/// rollback 按 ref 名逐项恢复/删除，所有错误直接 `?` 传播。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitMetadataSnapshot {
    /// HEAD 引用的快照（symbolic 或 detached）。
    pub head: RefSnapshot,
    /// 所有本轮会修改的 branch/remote refs 的快照。
    /// key = ref 名（如 `refs/heads/main`、`refs/remotes/origin/main`）。
    /// value = finalize 前的状态（`DidNotExist` 表示 finalize 会新建）。
    pub refs: std::collections::BTreeMap<String, RefSnapshot>,
    /// index 快照。
    pub index: IndexSnapshot,
    /// finalize 前 live 是否已是 Git repo。
    pub repo_existed: bool,
}

/// #644 评论 5476546134 第2节：Git finalize 崩溃恢复记录。
///
/// 写入 `TransactionManifest.git_finalize`，使重启后能独立从 live + transaction 目录
/// 完成恢复，不依赖可能残缺的 staging run。
///
/// #644 评论 5480360027：`plan` 是 write-ahead plan，在 `prepare_git_finalize` 时
/// 一次完整生成，随 manifest 原子落盘。finalize 不再依赖只存在内存里的 mutation_log；
/// 成功就 finish，失败/崩溃恢复都只依赖磁盘上的 plan。
///
/// #644 评论 5491531984 问题4：新增 `git_dir` 和 `worktree_root` 字段，
/// 使崩溃恢复知道私有 git_dir 在哪里，不再硬编码 `target_root/.git`。
/// 旧 manifest 中无此字段时反序列化为 None，解释为 legacy `target_root/.git`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFinalizeRecoveryRecord {
    /// seed 时记录的 live Git 仓库状态（序列化友好版本）。
    pub seed_state: SerializableGitSeedState,
    /// finalize 前的 Git metadata 快照。
    pub metadata_snapshot: GitMetadataSnapshot,
    /// #644 评论 5480360027：write-ahead plan，在写 live Git metadata 前完整落盘。
    /// 旧 manifest 中无此字段时反序列化为 `GitFinalizePlan::default()`（向后兼容）。
    #[serde(default)]
    pub plan: GitFinalizePlan,
    /// #644 评论 5478237852 问题2：旧 mutation journal 字段，保留向后兼容。
    /// 新代码使用 `plan`，不再读写此字段。
    #[serde(default)]
    pub mutation_log: GitFinalizeMutationLog,
    /// #644 评论 5491531984 问题4：可写 Git metadata 的根目录。
    /// `None` 表示 legacy manifest，使用 `target_root/.git`。
    /// `Some(path)` 表示私有 git_dir（如 `filesDir/sujian-git/<project-id>/`）。
    #[serde(default)]
    pub git_dir: Option<std::path::PathBuf>,
    /// #644 评论 5491531984 问题4：用户可见文件的根目录（worktree）。
    /// `None` 表示 legacy manifest，使用 `target_root`。
    /// `Some(path)` 表示显式记录的 worktree root。
    #[serde(default)]
    pub worktree_root: Option<std::path::PathBuf>,
}

/// #644 评论 5480360027：write-ahead plan，在 `prepare_git_finalize` 时完整生成。
///
/// 与 `GitFinalizeMutationLog` 的根本区别：plan 在写任何 live Git metadata **之前**
/// 就能完整落盘到 manifest，不依赖 finalize 过程中的内存状态。崩溃恢复时只依赖
/// 磁盘上的 plan，根据当前 live 值判断某一步是否真的发生：
/// - current == new_oid → 这一步已执行，反向 CAS 回 old_oid；
/// - current == old_oid → 这一步没执行，无需 rollback；
/// - 两者都不是 → 有并发新状态，不能覆盖。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitFinalizePlan {
    /// 是否会创建 repo（NotGitRepo 路径）。
    pub repo_create: bool,
    /// 计划写入的 index 的 SHA-256。
    /// `None` 表示不会写 index（staging 无 .git 或无 HEAD）。
    /// rollback 时只有当前 index hash 等于此值才恢复 snapshot.index。
    pub new_index_sha256: Option<[u8; 32]>,
    /// 本轮会修改的 ref 变更计划，按计划写入顺序。
    /// key = ref 名，value = (old_oid_or_none, new_oid)。
    /// old_oid_or_none = None 表示 ref 原本不存在（DidNotExist），finalize 会新建。
    pub ref_plans: Vec<(String, Option<String>, String)>,
    /// #644 评论 5481496190 问题3：NotGitRepo 路径的 owner marker（uuid）。
    /// 在 prepare 阶段生成，finalize_not_git_repo 写入 tmp_git/.sujian-sync-owner，
    /// rename 后进入 live .git。rollback 只有 marker 匹配才删除 live .git，
    /// 避免误删外部后来创建的仓库。正常 finalize 完成后删除 marker。
    #[serde(default)]
    pub repo_create_owner: Option<String>,
    /// #644 评论 5484539222 缺陷1 + #644 评论 5486852142 问题1：index.lock 持久 ownership 的 owner uuid。
    /// 在 prepare 阶段生成（当 `new_index_sha256.is_some()` 时），随 manifest 落盘。
    /// finalize/rollback 用 `OwnedIndexLock` 把 `.git/index.lock` 创建为**目录**，
    /// 目录内 `owner` 文件写 owner metadata，`prepared` 文件写目标 index 字节。
    /// 恢复时检查 lock_path 是否是目录 + owner 文件内容判断 ownership：
    /// 属于本轮才清理，不属于本轮（外部 Git regular file lock 或不同事务的目录锁）
    /// 绝不碰。即使进程被 SIGKILL，Drop 不执行，磁盘上 lock 目录仍在，
    /// 恢复时可通过目录类型 + owner 文件判断归属。
    #[serde(default)]
    pub index_lock_owner: Option<String>,
    /// #644 评论 5489192105 问题2：ref transaction 持久 ownership 的 owner uuid。
    /// 在 prepare 阶段生成（当 `ref_plans` 非空时），随 manifest 落盘。
    /// finalize/rollback 用 `RefTransaction` 锁住本轮全部 refs，
    /// 每个 ref 旁边写 `<ref>.sujian-ref-lock` owner marker 文件。
    /// 恢复时检查 `<ref>.lock` + `<ref>.sujian-ref-lock` 判断 ownership：
    /// - lock 不存在 → 无 lock，清理 orphan owner marker。
    /// - lock 存在 + owner marker 匹配 → 本事务 stale lock，可清理。
    /// - lock 存在 + owner marker 不匹配 → 别的素笺事务 lock，不碰。
    /// - lock 存在 + 无 owner marker → 外部 Git regular lock，不碰。
    ///
    /// 即使进程被 SIGKILL，Drop 不执行，磁盘上 lock file + owner marker 仍在，
    /// 恢复时可通过 owner marker 判断归属。
    #[serde(default)]
    pub ref_tx_owner: Option<String>,
    /// #644 评论 5490206957 问题3：forward transaction 的完整 ref lock 集合。
    ///
    /// 不要从"会写哪些 refs"（`ref_plans`）反推"会锁哪些 refs"——Unborn/Existing
    /// 的 `ref_plans` 只有 head_ref，但 forward 实际还锁了 HEAD + remote refs。
    /// 本字段在 `build_finalize_plan` / forward finalize 时**一次写全**：
    /// HEAD + head_ref + remote refs，按实际 forward transaction 的完整锁集合保存。
    ///
    /// forward acquire、rollback stale-lock cleanup、owner migration 全部只读这一个集合。
    #[serde(default)]
    pub ref_lock_names: Vec<String>,
}

/// #644 评论 5478237852 问题2：旧 mutation journal 字段，保留向后兼容。
///
/// 新代码使用 `GitFinalizePlan`。此结构仅用于反序列化旧 manifest。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitFinalizeMutationLog {
    /// 本轮 finalize 是否真的把 .git 安装进了 live（仅 NotGitRepo 路径）。
    pub created_repo_by_us: bool,
    /// 本轮成功写入的 ref 变更，按写入顺序。
    /// key = ref 名，value = (old_oid_or_none, written_oid)。
    /// old_oid_or_none = None 表示 ref 原本不存在（DidNotExist），finalize 新建了它。
    pub ref_mutations: Vec<(String, Option<String>, String)>,
    /// 本轮是否成功写入了 index。
    pub index_written: bool,
    /// 写入前 index 的 SHA-256（用于 CAS rollback：只有当前 index 仍等于我们写的那份才恢复）。
    pub written_index_sha256: Option<[u8; 32]>,
}

/// #644 评论 5476546134 第2节：`GitSeedState` 的序列化友好版本。
///
/// `git2::Oid` 不实现 `Serialize/Deserialize`，用 hex 字符串存储。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SerializableGitSeedState {
    NotGitRepo,
    Unborn { head_ref: String },
    Existing { head_ref: String, head_oid: String },
    Detached { head_oid: String },
}

impl SerializableGitSeedState {
    /// 从 `GitSeedState` 转换。
    pub fn from_seed_state(state: &super::seed::GitSeedState) -> Self {
        use super::seed::GitSeedState;
        match state {
            GitSeedState::NotGitRepo => Self::NotGitRepo,
            GitSeedState::Unborn { head_ref } => Self::Unborn {
                head_ref: head_ref.clone(),
            },
            GitSeedState::Existing { head_ref, head_oid } => Self::Existing {
                head_ref: head_ref.clone(),
                head_oid: head_oid.to_string(),
            },
            GitSeedState::Detached { head_oid } => Self::Detached {
                head_oid: head_oid.to_string(),
            },
        }
    }

    /// 还原为 `GitSeedState`。
    pub fn to_seed_state(
        &self,
    ) -> std::result::Result<super::seed::GitSeedState, String> {
        use super::seed::GitSeedState;
        match self {
            Self::NotGitRepo => Ok(GitSeedState::NotGitRepo),
            Self::Unborn { head_ref } => Ok(GitSeedState::Unborn {
                head_ref: head_ref.clone(),
            }),
            Self::Existing { head_ref, head_oid } => {
                let oid =
                    git2::Oid::from_str(head_oid).map_err(|e| format!("invalid head_oid: {e}"))?;
                Ok(GitSeedState::Existing {
                    head_ref: head_ref.clone(),
                    head_oid: oid,
                })
            }
            Self::Detached { head_oid } => {
                let oid =
                    git2::Oid::from_str(head_oid).map_err(|e| format!("invalid head_oid: {e}"))?;
                Ok(GitSeedState::Detached { head_oid: oid })
            }
        }
    }
}

// ── Git finalize 错误类型 ──

/// #644 评论 5476546134 第4节 + #644 评论 5477439446 问题2：
/// Git finalize 错误类型，区分 finalize 失败和 rollback 失败。
///
/// 上层遇到 `RollbackFailed` 时不能把 transaction 清掉，必须保留给下次恢复。
///
/// #644 评论 5477439446 问题2：`ConcurrentMetadataChanged` 表示在改 live Git
/// metadata 之前检测到并发修改（index/HEAD/branch/remote refs 与 snapshot 不一致）。
/// 此时本轮 finalize 还没改过 live Git metadata，**不能调用 rollback_git_finalize**，
/// 否则会把别人刚写的新状态覆盖成 Transfer 开始前的旧 snapshot。上层对这类错误
/// 应直接返回失败，不触发 Git metadata rollback（文件 rollback 仍可由调用方决定）。
#[derive(Debug, thiserror::Error)]
pub enum GitFinalizeError {
    #[error("finalize failed: {0}")]
    FinalizeFailed(#[from] crate::Error),
    #[error("finalize failed ({finalize}), rollback also failed: {rollback}")]
    RollbackFailed { finalize: String, rollback: String },
    /// #644 评论 5477439446 问题2：并发校验失败，本轮尚未修改 live Git metadata。
    /// 不触发 Git metadata rollback。`reason` 描述哪个 metadata 不一致。
    #[error("concurrent git metadata changed before finalize wrote anything: {reason}")]
    ConcurrentMetadataChanged { reason: String },
}

/// #644 评论 5482310913 问题3：Git finalize rollback 的明确结果。
///
/// `rollback_git_finalize` 不再返回 `Result<()>`，而是 `Result<GitRollbackOutcome>`，
/// 让上层 `rollback_full_sync_transaction` 能区分"已安全回滚，可继续恢复文件 backup"
/// 与"检测到并发变更或事务已成功，不能继续回滚文件"。
///
/// - `Reverted`：所有 index/ref 反向 CAS 都完整成功，上层可恢复文件 backup。
/// - `ConcurrentChanged`：repo_create ownership 不匹配（外部仓库）等无法证明归属的情况。
///   保留 transaction，不继续改 live 文件。上层应返回 Err 让 `recover_pending_transactions`
///   保留事务目录给下次恢复。
/// - `RepoInstallCommitted`：NotGitRepo 已完成 owner-matched `.git` rename。
///   按 commit-point 逻辑收尾，不再把文件回滚成旧版。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitRollbackOutcome {
    /// 所有 index/ref 反向 CAS 都完整成功，上层可恢复文件 backup。
    Reverted,
    /// repo_create ownership 不匹配 / 外部仓库，保留 transaction 不继续改 live 文件。
    ConcurrentChanged,
    /// NotGitRepo 已完成 owner-matched `.git` rename，按 commit-point 逻辑收尾。
    RepoInstallCommitted,
}

/// #644 评论 5488871385 问题1：只读检查 Git rollback 状态，不修改任何 Git/live 文件。
///
/// 在真正进入 rollback 路径之前调用，决定需要做什么：
/// - `NeedsRollback`：需要回滚 Git metadata + 文件 backup。
/// - `RepoInstallCommitted`：NotGitRepo 已完成 owner-matched `.git` rename，直接 Finished。
/// - `ConcurrentChanged`：并发变更，保留事务。
///
/// 与 `rollback_git_finalize()` 的区别：本函数**只读**，不修改 index/refs/live 文件。
/// 调用方拿到结果后决定 preflight → Git rollback → file rollback 的顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitRollbackState {
    /// 需要回滚 Git metadata（index + refs）。调用方应先 preflight backup，再 rollback。
    NeedsRollback,
    /// NotGitRepo 已完成 owner-matched `.git` rename。按 commit-point 逻辑收尾。
    RepoInstallCommitted,
    /// 并发变更（ownership 不匹配、外部仓库等）。保留事务，不继续改 live 文件。
    ConcurrentChanged,
}

/// #644 评论 5485518160 修改点 2：`index_lock_owner=None` 旧 manifest 迁移判定结果。
///
/// 当 `plan.new_index_sha256.is_some()` 且 `plan.index_lock_owner.is_none()` 时，
/// 恢复入口（`rollback_full_sync_transaction`）在调用 `rollback_git_finalize` 之前
/// 用 `check_index_lock_owner_migration` 判断磁盘状态，决定如何处理：
pub enum IndexLockOwnerMigration {
    /// canonical `.git/index.lock` 已存在。不知道是谁的，不能 terminalize transaction，
    /// 返回 Err 保留 tx_dir。
    LockExists,
    /// 没有 `index.lock` 且 current index == snapshot.index（old）。index 这一步
    /// 本来就没发生，安全按 AlreadyReverted 继续（无需补 owner，直接让
    /// rollback_git_finalize 走 current==old 分支）。
    AlreadyReverted,
    /// 没有 `index.lock` 且 current index == new（plan.new_index_sha256 命中）。
    /// 生成新的 owner UUID，调用方应先原子写回 manifest.git_finalize.plan.index_lock_owner
    /// 并 fsync，再进入反向 rollback（此时 plan.index_lock_owner 已是 Some，
    /// rollback_git_finalize 的 current==new 分支能拿到 owner）。
    MigrateToNewOwner(String),
    /// current 既不是 old 也不是 new。并发修改，返回 Err 保留事务。
    ConcurrentModification,
}

impl std::fmt::Debug for IndexLockOwnerMigration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LockExists => write!(f, "LockExists"),
            Self::AlreadyReverted => write!(f, "AlreadyReverted"),
            Self::MigrateToNewOwner(_) => write!(f, "MigrateToNewOwner(<uuid>)"),
            Self::ConcurrentModification => write!(f, "ConcurrentModification"),
        }
    }
}

/// 评论 5489750244 问题5：`ref_tx_owner=None` 旧 manifest 迁移判定结果。
///
/// 当 `plan.ref_plans` 非空且 `plan.ref_tx_owner` 是 None 时（旧 manifest 升级到新代码），
/// 恢复入口在调用 `rollback_git_finalize` 之前用 `check_ref_tx_owner_migration`
/// 判断磁盘状态，决定如何处理。
pub enum RefTxOwnerMigration {
    /// plan 中任一 ref 存在 canonical lock file。不知道归属，不能 terminalize。
    LockExists,
    /// 任一 ref 的当前值既不是 old 也不是 new（CAS miss）。并发修改。
    ConcurrentModification,
    /// 无 canonical lock 且所有 ref 的当前值均匹配 old/new/absent（三态允许）。
    /// 生成新的 owner UUID，调用方应先原子写回 manifest.git_finalize.plan.ref_tx_owner
    /// 并 fsync，再进入 rollback 路径。
    MigrateToNewOwner(String),
}

impl std::fmt::Debug for RefTxOwnerMigration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LockExists => write!(f, "LockExists"),
            Self::ConcurrentModification => write!(f, "ConcurrentModification"),
            Self::MigrateToNewOwner(_) => write!(f, "MigrateToNewOwner(<uuid>)"),
        }
    }
}

/// 比较两个 `RefSnapshot` 是否相等。
pub fn ref_snapshot_eq(a: &RefSnapshot, b: &RefSnapshot) -> bool {
    match (a, b) {
        (RefSnapshot::DidNotExist, RefSnapshot::DidNotExist) => true,
        (RefSnapshot::Existed { oid: a }, RefSnapshot::Existed { oid: b }) => a == b,
        (RefSnapshot::Symbolic { target: a }, RefSnapshot::Symbolic { target: b }) => a == b,
        _ => false,
    }
}

/// 比较两个 `IndexSnapshot` 是否相等。
pub fn index_snapshot_eq(a: &IndexSnapshot, b: &IndexSnapshot) -> bool {
    match (a, b) {
        (IndexSnapshot::Missing, IndexSnapshot::Missing) => true,
        (IndexSnapshot::Bytes(a), IndexSnapshot::Bytes(b)) => a == b,
        _ => false,
    }
}

pub fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}
