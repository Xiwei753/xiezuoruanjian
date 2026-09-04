use std::fs;
use std::path::{Path, PathBuf};

use super::model::{sha256_bytes, GitFinalizeError};
use crate::error::Result;

/// #644 评论 5486167472 问题1+问题2 + #644 评论 5486852142 问题1：持久 ownership 的 index lock。
///
/// ## IndexLockProtocol（目录锁模型，Android shared-storage 可落地）
///
/// Android 共享存储（AOSP FUSE）不提供 hardlink，`rename` 的带 flag 版本也不是
/// 可依赖的 no-replace CAS。本协议只用 Android shared-storage 能提供的原语：
/// `create_dir`（原子创建目录，已存在则失败）、`rename`（覆盖式）、`fsync`、读写文件内容。
///
/// - **canonical lock** = `.git/index.lock` 作为**目录**（不是 regular file）。
///   `fs::create_dir(&lock_path)` 原子创建目录，已存在则失败。标准 Git 创建 regular
///   `index.lock` 会因路径已存在（是目录）而失败，目录类型本身就是 Sujian 协议事实。
///   `create_dir` 成功就是原子的 ownership 证明，**不再有** O_EXCL create-to-write 窗口。
/// - 目录内放：
///   - `owner` 文件：写 owner metadata（`INDEX_LOCK_MARKER` + `owner=<owner>`），fsync。
///   - `prepared` 文件：写 prepared bytes（目标 index 内容），fsync。
/// - **ownership** 通过目录存在性 + 目录内 `owner` 文件内容证明。恢复时：
///   - `lock_path` 是 regular file → 外部 Git 进程的 lock（标准 Git 创建 regular file）。
///   - `lock_path` 是目录 → Sujian lock（不可能是外部 Git 的）。读 `owner` 文件判断归属：
///     - `owner` 文件不存在/为空 → Sujian lock 但未完成写入（crash 在 create_dir 和 write
///       owner 之间），可安全清理。
///     - `owner` 文件存在且内容匹配 → 本轮 lock。
///     - `owner` 文件存在但不匹配 → 不同事务的 lock。
/// - **commit_rename**：rename `prepared` → index（覆盖），然后删除 lock 目录，每步 fsync 父目录。
/// - **commit_delete**（Missing 路径）：删 index → 删 prepared → 删 lock 目录，每步 fsync 父目录。
///
/// 关键不变量：`create_dir` 成功就是原子的 ownership 证明。即使 crash 在 create_dir 后、
/// write owner 前，恢复时看到目录就知道是 Sujian lock（不可能是外部 Git 的），可以安全清理。
/// 不再有"空/半截 lock 被当成外部 Git lock"的窗口。
///
/// #644 评论 5486167472 问题2：State 2（已 commit）带方向事实。
/// `AlreadyCommitted` 只有在 live index 的内容 hash **等于本次 `prepared_bytes` 的 hash**
/// 时才成立。如果 owner/index 内容是 forward 的 new，而当前调用准备的是 rollback 的 old，
/// 就只能认定"上一阶段 forward commit 已完成"，先清 lock 目录，再 fall through
/// 重新 acquire（写本次 prepared_bytes 并获取 lock），不能直接返回 `AlreadyCommitted`。
/// Missing 路径：`prepared_bytes` 为空切片时，`AlreadyCommitted` 要求 live index 也不存在
///（Missing == Missing）。
pub struct OwnedIndexLock {
    /// `.git/index.lock`（现在是目录，不是 regular file）。
    lock_path: PathBuf,
    /// `lock_path/prepared`：写 prepared bytes（目标 index 内容）。
    prepared_file: PathBuf,
    /// `true` 表示已成功 commit（lock 目录已删走），`drop` 无操作。
    /// `false` 表示未 commit（出错路径），`drop` 清理 lock 目录。
    disarmed: bool,
}

/// #644 评论 5486167472 问题1：owner metadata 格式常量。
///
/// 写入 `.git/index.lock/owner` 的 owner metadata，用于区分本轮 lock 和不同事务的 lock。
/// 外部 Git 创建的 `index.lock` 是 regular file（不是目录），恢复时看到 regular file
/// 就知道是外部 Git 的 lock → `ConcurrentMetadataChanged`，绝不删。
pub const INDEX_LOCK_MARKER: &str = "sujian-index-lock-v1";

/// #644 评论 5486167472 问题1：构造 owner metadata 字节串。
pub fn owner_metadata(owner: &str) -> Vec<u8> {
    format!("{INDEX_LOCK_MARKER}\nowner={owner}\n").into_bytes()
}

/// #644 评论 5486167472 问题1：解析 owner 文件内容，判断是否是本轮 owner。
///
/// 返回 `true` 表示 owner 文件内容是本轮 owner metadata（格式匹配 + owner 匹配）。
/// 返回 `false` 表示无法解析（损坏的 owner 文件）或 owner 不匹配（不同事务的 lock）。
///
/// 注意：此函数只解析 owner metadata 字节串。目录锁模型的归属判断用
/// `lock_dir_belongs_to_owner`，它先检查 lock_path 是否是目录，再读 owner 文件。
pub fn lock_belongs_to_owner(lock_bytes: &[u8], owner: &str) -> bool {
    let Ok(text) = std::str::from_utf8(lock_bytes) else {
        return false;
    };
    let mut lines = text.lines();
    if lines.next() != Some(INDEX_LOCK_MARKER) {
        return false;
    }
    let expected = format!("owner={owner}");
    lines.next() == Some(&expected)
}

/// #644 评论 5486852142 问题1 + #644 评论 5487751293 问题4：
/// 目录锁模型的归属判断。
///
/// 返回值语义：
/// - `LockOwner::Ours`：lock_path 是目录，且 owner 文件内容匹配本轮 owner。
///   可以安全清理。
/// - `LockOwner::IncompleteSujianLock`：lock_path 是目录，但 owner 文件不存在或为空
///   （crash 在 create_dir 和 write owner 之间）。Sujian 目录锁不可能是外部 Git 的，
///   可安全清理。
/// - `LockOwner::External`：lock_path 是 regular file（外部 Git 进程的 lock），
///   或 lock_path 是目录但 owner 文件内容不匹配（不同事务的 lock）。绝不碰。
/// - `LockOwner::Unknown`：lock_path 是目录但 owner 文件读取失败（EIO、权限异常等）。
///   不能降级成"owner 为空 = ours"，绝不删。
/// - `LockOwner::Absent`：lock_path 不存在。
pub enum LockOwner {
    /// lock_path 不存在。
    Absent,
    /// lock_path 是目录且 owner 文件内容匹配本轮 owner。
    Ours,
    /// lock_path 是目录但 owner 文件不存在或为空（crash 在 create_dir 和 write owner 之间）。
    /// Sujian 目录锁不可能是外部 Git 的，可安全清理。
    IncompleteSujianLock,
    /// lock_path 是 regular file（外部 Git）或是目录但 owner 不匹配（不同事务）。
    External,
    /// #644 评论 5487751293 问题4：lock_path 是目录但 owner 文件读取失败。
    /// 不能降级成"owner 为空 = ours"，绝不删。
    Unknown,
}

/// #644 评论 5486852142 问题1 + #644 评论 5487751293 问题4：
/// 判断 lock_path 的归属。
///
/// - `lock_path` 不存在 → `Absent`。
/// - `lock_path` 是目录 → Sujian lock。读 `lock_path/owner` 文件：
///   - owner 文件 `NotFound` → `IncompleteSujianLock`（crash 在 create_dir 和 write
///     owner 之间，可安全清理，因为是 Sujian 目录锁不可能是外部 Git 的）。
///   - owner 文件存在且为空 → `IncompleteSujianLock`（同上）。
///   - owner 文件存在且内容匹配 → `Ours`。
///   - owner 文件存在但不匹配 → `External`（不同事务的 lock）。
///   - 其它读取错误（EIO、权限异常等）→ `Unknown`，绝不删。
/// - `lock_path` 是 regular file → `External`（外部 Git 进程的 lock）。
pub fn lock_dir_belongs_to_owner(lock_path: &Path, owner: &str) -> LockOwner {
    if !lock_path.exists() {
        return LockOwner::Absent;
    }
    if !lock_path.is_dir() {
        // regular file → 外部 Git 进程的 lock。
        return LockOwner::External;
    }
    // lock_path 是目录 → Sujian lock。读 owner 文件判断归属。
    let owner_file = lock_path.join("owner");
    match fs::read(&owner_file) {
        Ok(owner_bytes) if owner_bytes.is_empty() => {
            // owner 文件存在但为空 → Sujian lock 但未完成写入，可安全清理。
            LockOwner::IncompleteSujianLock
        }
        Ok(owner_bytes) => {
            if lock_belongs_to_owner(&owner_bytes, owner) {
                LockOwner::Ours
            } else {
                LockOwner::External
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // owner 文件不存在 → crash 在 create_dir 和 write owner 之间。
            // Sujian 目录锁不可能是外部 Git 的，可安全清理。
            LockOwner::IncompleteSujianLock
        }
        Err(_) => {
            // #644 评论 5487751293 问题4：其它读取错误（EIO、权限异常等）。
            // 不能降级成"owner 为空 = ours"，返回 Unknown 绝不删。
            LockOwner::Unknown
        }
    }
}

/// #644 评论 5486852142 问题1 + #644 评论 5488100307 问题2：
/// 删除 lock 目录（如果存在且是目录）并 fsync 父目录。
///
/// 提取为辅助函数以减少 `acquire` 的嵌套深度。
/// - lock_path 不存在或不是目录 → Ok(())（no-op）。
/// - remove_dir_all 失败 → Err（调用方必须保留 transaction，不能返回 Reverted）。
/// - sync_dir 失败 → Err（同上）。
pub fn remove_lock_dir_if_exists(lock_path: &Path, git_dir: &Path) -> Result<()> {
    if !lock_path.exists() {
        return Ok(());
    }
    if !lock_path.is_dir() {
        return Ok(());
    }
    fs::remove_dir_all(lock_path)?;
    crate::storage::sync_dir(git_dir)?;
    Ok(())
}

/// #644 评论 5485518160 修改点 1：`OwnedIndexLock::acquire` 的返回类型。
///
/// 区分"新获取的锁（未提交，需调用方 commit）"和"上次已提交的锁（调用方应跳过 commit）"。
/// `AlreadyCommitted` 表示磁盘状态显示上一次 `commit_rename`/`commit_delete` 已完成
///（lock 不存在 + live index hash == 本次 prepared_bytes hash），调用方应把 lock 视为
/// 已 disarm，不再 commit，直接依据 current index 的 old/new 状态继续恢复。
pub enum AcquireOutcome {
    /// 新获取的锁，调用方需在适当时机调用 `commit_rename`/`commit_delete`。
    NewlyAcquired(OwnedIndexLock),
    /// 上次 acquire 后已成功 commit（rename/delete 已完成），调用方应跳过 commit。
    AlreadyCommitted,
}

impl OwnedIndexLock {
    /// #644 评论 5486167472 问题1+问题2 + #644 评论 5486852142 问题1：恢复已有 ownership → 再创建新 ownership 的两段状态机。
    ///
    /// ## Phase 1：检查磁盘崩溃状态
    ///
    /// lock_path = `.git/index.lock`（目录），index_path = `.git/index`。
    /// 根据 lock_path 类型 + index hash 判断上一次 acquire/commit 死在哪一步：
    ///
    /// - **State 1（已拿锁未 commit）**：lock_path 是目录 + owner 文件不存在/为空或
    ///   owner 匹配本轮。说明上次 acquire 已成功但 commit 未执行。删除 lock 目录，
    ///   fsync `.git`，fall through 到 Phase 2 重新 acquire。
    /// - **State 2（已 commit，且方向匹配）**：lock_path 不存在 + live index hash == 本次
    ///   `prepared_bytes` hash（Missing == Missing 也算）。说明上次 `commit_rename`/`commit_delete`
    ///   已完成且内容方向匹配。返回 `AlreadyCommitted`。
    /// - **State 3（已 commit，但方向不匹配）**：lock_path 不存在 + live index hash != 本次
    ///   `prepared_bytes` hash。说明是另一方向的已提交状态（forward commit 后崩溃，现在
    ///   进入 rollback）。fall through 到 Phase 2 重新 acquire（写本次 prepared_bytes 并获取 lock）。
    /// - **State 4（归属未知/外部 Git）**：lock_path 是 regular file（外部 Git 进程的 lock），
    ///   或 lock_path 是目录但 owner 文件内容不匹配本轮（不同事务的 lock）。返回
    ///   `ConcurrentMetadataChanged`，保留事务，绝不碰 lock。
    ///
    /// ## Phase 2：新建 ownership（目录锁模型）
    ///
    /// 1. `fs::create_dir(&lock_path)` 原子创建 lock 目录（已存在则失败）。
    ///    这一步本身就是原子的 ownership 证明。
    /// 2. 在 `lock_path/owner` 写 owner metadata，fsync。如果失败，清理目录并返回 Err。
    /// 3. 在 `lock_path/prepared` 写 `prepared_bytes`，fsync。如果失败，清理目录并返回 Err。
    ///
    /// 关键安全属性：`create_dir` 成功就是原子的 ownership 证明。即使 crash 在 create_dir 后、
    /// write owner 前，恢复时看到目录就知道是 Sujian lock（不可能是外部 Git 的），可以安全清理。
    /// 不再有"空/半截 lock 被当成外部 Git lock"的窗口。
    #[allow(clippy::too_many_lines)]
    pub fn acquire(
        git_dir: &Path,
        owner: &str,
        prepared_bytes: &[u8],
    ) -> std::result::Result<AcquireOutcome, GitFinalizeError> {
        let lock_path = git_dir.join("index.lock");
        let owner_file = lock_path.join("owner");
        let prepared_file = lock_path.join("prepared");
        let index_path = git_dir.join("index");

        let metadata = owner_metadata(owner);

        // ── Phase 1：检查磁盘崩溃状态 ──
        match lock_dir_belongs_to_owner(&lock_path, owner) {
            LockOwner::Absent => {
                // lock 不存在：检查 index hash 判断是否已 commit（State 2 / State 3）。
                let current_index_hash = if index_path.exists() {
                    let bytes = fs::read(&index_path)
                        .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
                    Some(sha256_bytes(&bytes))
                } else {
                    None
                };
                let prepared_hash = sha256_bytes(prepared_bytes);
                // Missing == Missing 也算 AlreadyCommitted（prepared_bytes 为空 + index 不存在）。
                let index_matches_prepared = match &current_index_hash {
                    Some(h) => *h == prepared_hash,
                    None => prepared_bytes.is_empty(),
                };

                if index_matches_prepared {
                    // State 2：上次 commit 已完成且方向匹配。返回 AlreadyCommitted。
                    return Ok(AcquireOutcome::AlreadyCommitted);
                }
                // State 3：lock 不存在但 index hash != prepared_bytes hash。
                // 可能是另一方向的已提交状态（forward commit 后崩溃，现在 rollback）。
                // fall through 到 Phase 2 重新 acquire。
            }
            LockOwner::Ours | LockOwner::IncompleteSujianLock => {
                // State 1：上次已拿锁但还没 commit（owner 匹配或 owner 未完成写入）。
                // lock 目录是本事务自己的。删除 lock 目录，fsync .git，
                // fall through 到 Phase 2 重新 acquire。
                fs::remove_dir_all(&lock_path)
                    .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
                crate::storage::sync_dir(git_dir).map_err(GitFinalizeError::FinalizeFailed)?;
            }
            LockOwner::External => {
                // State 4：lock_path 是 regular file（外部 Git 进程的 lock），
                // 或 lock_path 是目录但 owner 不匹配（不同事务的 lock）。
                // 绝不碰 lock，返回 ConcurrentMetadataChanged。
                return Err(GitFinalizeError::ConcurrentMetadataChanged {
                    reason: "index.lock exists but does not belong to us: \
                             concurrent git process (regular file lock) or different \
                             transaction (directory lock with mismatched owner) is writing index"
                        .to_string(),
                });
            }
            LockOwner::Unknown => {
                // #644 评论 5487751293 问题4：owner 文件读取失败（EIO 等）。
                // 不能降级成"owner 为空 = ours"，返回 ConcurrentMetadataChanged。
                return Err(GitFinalizeError::ConcurrentMetadataChanged {
                    reason: "index.lock directory exists but owner file read failed \
                             (IO error) — cannot determine lock ownership"
                        .to_string(),
                });
            }
        }

        // ── Phase 2：新建 ownership（目录锁模型） ──
        // 1. create_dir 原子创建 lock 目录（已存在则失败）。
        //    这一步本身就是原子的 ownership 证明。
        fs::create_dir(&lock_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        crate::storage::sync_dir(git_dir).map_err(GitFinalizeError::FinalizeFailed)?;

        // #644 评论 5488100307 问题3：owner 和 prepared 文件用原子写入（tmp + fsync + rename），
        // 避免 write_all 中途失败留下半截非空 owner 文件。
        // 半截非空 owner 会被 lock_dir_belongs_to_owner 判成 External（不匹配），
        // 导致 recovery 永远不敢删它，transaction 永远无法继续。
        // atomic_write_bytes 内部：create tmp → write_all → flush → sync_all → rename → sync_parent。
        // 任何一步失败时清理 lock 目录并返回 Err。
        //
        // 2. 在 lock 目录内原子写入 owner 文件。
        if let Err(e) = crate::storage::atomic_write_bytes(&owner_file, &metadata) {
            let _ = fs::remove_dir_all(&lock_path);
            let _ = crate::storage::sync_dir(git_dir);
            return Err(GitFinalizeError::FinalizeFailed(e));
        }

        // 3. 在 lock 目录内原子写入 prepared 文件。
        if let Err(e) = crate::storage::atomic_write_bytes(&prepared_file, prepared_bytes) {
            let _ = fs::remove_dir_all(&lock_path);
            let _ = crate::storage::sync_dir(git_dir);
            return Err(GitFinalizeError::FinalizeFailed(e));
        }

        Ok(AcquireOutcome::NewlyAcquired(Self {
            lock_path,
            prepared_file,
            disarmed: false,
        }))
    }

    /// 提交（Bytes 路径）：`rename prepared_file → index`（原子提交），然后删 lock 目录。
    /// 每步 fsync 父目录。成功后 `disarm`，`drop` 无操作。
    pub fn commit_rename(
        &mut self,
        index_path: &Path,
    ) -> std::result::Result<(), GitFinalizeError> {
        // rename prepared_file → index（覆盖式 rename，原子提交）。
        fs::rename(&self.prepared_file, index_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        crate::storage::sync_parent(index_path).map_err(GitFinalizeError::FinalizeFailed)?;
        // 删 lock 目录，fsync .git。
        fs::remove_dir_all(&self.lock_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        crate::storage::sync_parent(&self.lock_path).map_err(GitFinalizeError::FinalizeFailed)?;
        self.disarmed = true;
        Ok(())
    }

    /// 提交（Missing 路径）：删除 index，然后删除 prepared_file，然后删除 lock 目录，
    /// 每步 fsync 父目录。
    pub fn commit_delete(
        &mut self,
        index_path: &Path,
    ) -> std::result::Result<(), GitFinalizeError> {
        fs::remove_file(index_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        crate::storage::sync_parent(index_path).map_err(GitFinalizeError::FinalizeFailed)?;
        fs::remove_file(&self.prepared_file)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        crate::storage::sync_parent(&self.prepared_file)
            .map_err(GitFinalizeError::FinalizeFailed)?;
        fs::remove_dir_all(&self.lock_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        crate::storage::sync_parent(&self.lock_path).map_err(GitFinalizeError::FinalizeFailed)?;
        self.disarmed = true;
        Ok(())
    }
}

impl Drop for OwnedIndexLock {
    fn drop(&mut self) {
        if !self.disarmed {
            // 未 commit（出错路径）：清理 lock 目录（如果还在）。
            // remove_dir_all 会递归删除 owner_file 和 prepared_file。
            let _ = fs::remove_dir_all(&self.lock_path);
        }
        // commit 成功后 lock 目录已被 remove_dir_all 删走；
        // 未 commit 时上面的 remove_dir_all 已清理。无需额外操作。
    }
}
