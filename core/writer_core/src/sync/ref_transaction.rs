//! #644 评论 5489192105：统一的 ref transaction。
//!
//! 解决评论 5489192105 指出的 3 个问题（问题1+2+3）：
//! - **问题1（TOCTOU）**：先 acquire 全部 refs 的 writer exclusion，锁内 classify，
//!   再执行修改。消除 `rollback_git_finalize` 的 read→lock TOCTOU 窗口。
//! - **问题2（持久 ownership）**：owner marker 文件做崩溃归属判断，区分
//!   本事务 stale lock、别的素笺事务 lock、外部 Git regular lock。
//! - **问题3（packed refs）**：set/delete 通过 `git2::Transaction`（libgit2 refdb），
//!   不直接碰 loose ref 文件，正确处理 loose ref 和 packed-refs 两种存储形式。
//!
//! ## Lock 协议
//!
//! `git2::Transaction::lock_ref` 创建 regular lock file `<ref>.lock`（libgit2 标准格式）。
//! 我们在 `lock_ref` **之前**写 owner marker 文件 `<ref>.sujian-ref-lock`
//! （内容是 owner metadata，原子写入 + fsync）。
//!
//! 恢复时检查（见 `inspect_ref_lock_owner`）：
//! - `<ref>.lock` 不存在 → 无 lock，清理可能残留的 owner marker。
//! - `<ref>.lock` 存在 + `<ref>.sujian-ref-lock` 存在 + owner 匹配 → 本事务 stale lock，可清理。
//! - `<ref>.lock` 存在 + `<ref>.sujian-ref-lock` 存在 + owner 不匹配 → 别的素笺事务 lock，不碰。
//! - `<ref>.lock` 存在 + `<ref>.sujian-ref-lock` 不存在 → 外部 Git lock，不碰。
//!
//! ## TOCTOU 分析
//!
//! - **写 owner marker → lock_ref 之间崩溃**：owner marker 存在但 lock file 不存在。
//!   恢复时看到 owner marker 但无 lock file（`Absent`），可清理 owner marker。安全。
//! - **lock_ref → 后续操作之间崩溃**：lock file 存在，owner marker 存在
//!   （在 lock_ref 前写的）。恢复时识别为本事务 stale lock（`Ours`），可清理。安全。
//! - **commit 后、删 owner marker 前崩溃**：lock file 不存在（commit 释放了），
//!   owner marker 存在。恢复时看到 owner marker 但无 lock file（`Absent`），
//!   可清理 owner marker。安全。
//!
//! ## forward / rollback 统一
//!
//! `finalize_unborn`（forward）和 `rollback_git_finalize`（rollback）都使用
//! `RefTransaction`，统一 ref writer exclusion 机制，替代之前 forward 用
//! `git2::Transaction::lock_ref` / rollback 用 `OwnedRefLock` 的两套不同机制。

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

/// #644 评论 5489192105：ref lock owner metadata 格式常量。
///
/// 写入 `<ref>.sujian-ref-lock` 的 owner metadata，用于区分本轮 lock 和不同事务的 lock。
/// 外部 Git 创建的 `<ref>.lock` 是 regular file，旁边没有 `.sujian-ref-lock` marker。
pub const REF_LOCK_MARKER: &str = "sujian-ref-lock-v1";

/// #644 评论 5489192105：构造 ref lock owner metadata 字节串。
pub fn ref_lock_owner_metadata(owner: &str) -> Vec<u8> {
    format!("{REF_LOCK_MARKER}\nowner={owner}\n").into_bytes()
}

/// #644 评论 5489192105：解析 owner 文件内容，判断是否是本轮 owner。
///
/// 返回 `true` 表示 owner 文件内容是本轮 owner metadata（格式匹配 + owner 匹配）。
/// 返回 `false` 表示无法解析（损坏的 owner 文件）或 owner 不匹配（不同事务的 lock）。
pub fn ref_lock_belongs_to_owner(lock_bytes: &[u8], owner: &str) -> bool {
    let Ok(text) = std::str::from_utf8(lock_bytes) else {
        return false;
    };
    let mut lines = text.lines();
    if lines.next() != Some(REF_LOCK_MARKER) {
        return false;
    }
    let expected = format!("owner={owner}");
    lines.next() == Some(&expected)
}

/// #644 评论 5489192105：ref lock 归属判断结果。
///
/// 用于崩溃恢复时区分 lock 归属。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefLockOwner {
    /// `<ref>.lock` 不存在 → 无 lock。
    Absent,
    /// `<ref>.lock` 存在 + owner marker 存在 + owner 匹配 → 本事务 stale lock，可清理。
    Ours,
    /// `<ref>.lock` 存在 + owner marker 存在 + owner 不匹配 → 别的素笺事务 lock，不碰。
    OtherSujian,
    /// `<ref>.lock` 存在 + owner marker 不存在 → 外部 Git lock，不碰。
    External,
    /// owner marker 读取失败（EIO 等）或 owner marker 为空（crash 在写入中间）。
    /// 不能降级归属，绝不删。
    Unknown,
}

/// #644 评论 5489192105：判断 ref lock 归属。
///
/// `git_dir` = `.git` 目录，`ref_name` = 如 `refs/heads/main`，`owner` = 本事务 owner UUID。
///
/// 检查 `<ref>.lock` 和 `<ref>.sujian-ref-lock` 的存在性和内容，返回归属判断。
pub fn inspect_ref_lock_owner(git_dir: &Path, ref_name: &str, owner: &str) -> RefLockOwner {
    let lock_path = git_dir.join(format!("{}.lock", ref_name));
    let owner_marker = git_dir.join(format!("{}.sujian-ref-lock", ref_name));

    if !lock_path.exists() {
        return RefLockOwner::Absent;
    }

    // lock file 存在，检查 owner marker。
    match fs::read(&owner_marker) {
        Ok(bytes) if bytes.is_empty() => {
            // owner marker 存在但为空 → crash 在写 owner marker 中间。
            // 不安全归属，返回 Unknown。
            RefLockOwner::Unknown
        }
        Ok(bytes) => {
            if ref_lock_belongs_to_owner(&bytes, owner) {
                RefLockOwner::Ours
            } else {
                RefLockOwner::OtherSujian
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // owner marker 不存在 → 外部 Git lock。
            RefLockOwner::External
        }
        Err(_) => {
            // owner marker 读取失败 → Unknown。
            RefLockOwner::Unknown
        }
    }
}

/// #644 评论 5489192105：清理本事务的 stale ref lock。
///
/// 删除 `<ref>.lock`（regular file）和 `<ref>.sujian-ref-lock`，fsync 父目录。
/// 只在 `inspect_ref_lock_owner` 返回 `Ours` 时调用。
///
/// `<ref>.lock` 是 regular file（libgit2 创建），用 `fs::remove_file` 删除。
/// 如果 `<ref>.lock` 是目录（不应该，但防御性处理），返回 Err。
pub fn clean_stale_ref_lock(git_dir: &Path, ref_name: &str) -> Result<()> {
    let lock_path = git_dir.join(format!("{}.lock", ref_name));
    let owner_marker = git_dir.join(format!("{}.sujian-ref-lock", ref_name));

    if lock_path.exists() {
        if lock_path.is_dir() {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "clean_stale_ref_lock: {} is a directory, expected regular file — \
                 refusing to remove_dir_all (may be external tool's directory lock)",
                lock_path.display()
            ))));
        }
        fs::remove_file(&lock_path)?;
    }
    if owner_marker.exists() {
        fs::remove_file(&owner_marker)?;
    }
    // fsync 父目录。
    if let Some(parent) = lock_path.parent() {
        crate::storage::sync_dir(parent)?;
    }
    Ok(())
}

/// #644 评论 5489192105：清理可能残留的 orphan owner marker（lock 不存在时）。
///
/// 当 `<ref>.lock` 不存在但 `<ref>.sujian-ref-lock` 存在时，删除 owner marker。
/// 这是 crash 在"写 owner marker → lock_ref 之间"或"commit 后、删 owner marker 前"的清理路径。
pub fn clean_orphan_owner_marker(git_dir: &Path, ref_name: &str) -> Result<()> {
    let lock_path = git_dir.join(format!("{}.lock", ref_name));
    let owner_marker = git_dir.join(format!("{}.sujian-ref-lock", ref_name));

    if !lock_path.exists() && owner_marker.exists() {
        fs::remove_file(&owner_marker)?;
        if let Some(parent) = owner_marker.parent() {
            crate::storage::sync_dir(parent)?;
        }
    }
    Ok(())
}

/// #644 评论 5489192105：统一的 ref transaction。
///
/// 结合 `git2::Transaction`（ref lock + set/delete，正确处理 packed refs）
/// 和 owner marker 文件（持久 ownership，崩溃归属判断）。
///
/// ## 生命周期
///
/// 1. `acquire_all_refs(repo, ref_names, owner)`：先写全部 owner marker，再 `tx.lock_ref` 全部。
/// 2. 锁内 `find_reference` 读取并分类 ref 状态。
/// 3. `set_target` / `remove` 通过 `git2::Transaction`（libgit2 refdb，正确处理 packed refs）。
/// 4. `commit(self)`：`tx.commit()` 提交所有操作 + 删全部 owner marker。
/// 5. `drop`（未 commit 路径）：清理 owner marker，`git2::Transaction::drop` 释放 lock。
pub struct RefTransaction<'repo> {
    repo: &'repo git2::Repository,
    /// `Option` 以支持 `commit` 时 take 出来消耗。
    tx: Option<git2::Transaction<'repo>>,
    /// 所有已 lock 的 ref name（sorted，去重）。
    locked_refs: Vec<String>,
    /// owner marker 文件路径列表（与 locked_refs 一一对应）。
    owner_markers: Vec<PathBuf>,
    /// `true` 表示已成功 commit，`drop` 无操作。
    disarmed: bool,
}

impl<'repo> RefTransaction<'repo> {
    /// #644 评论 5489192105：acquire 全部 refs 的 lock。
    ///
    /// 流程：
    /// 1. 对每个 ref（sorted 去重）：写 owner marker 文件（原子写入 + fsync）。
    /// 2. 创建 `git2::Transaction`，对每个 ref `tx.lock_ref`。
    ///
    /// 如果任一 step 失败，清理已写的 owner marker，返回 Err。
    ///
    /// `ref_names` 为空时返回一个空 transaction（无 lock，commit 是 no-op）。
    #[allow(clippy::excessive_nesting)]
    pub fn acquire_all_refs(
        repo: &'repo git2::Repository,
        ref_names: &[String],
        owner: &str,
    ) -> Result<Self> {
        // sorted + 去重，避免死锁。
        let mut sorted_refs: Vec<String> = ref_names.to_vec();
        sorted_refs.sort();
        sorted_refs.dedup();

        let git_dir = repo.path();
        let metadata = ref_lock_owner_metadata(owner);

        let mut owner_markers: Vec<PathBuf> = Vec::with_capacity(sorted_refs.len());
        let mut written_markers: Vec<PathBuf> = Vec::new();

        // 1. 写全部 owner marker。
        for ref_name in &sorted_refs {
            let owner_marker = git_dir.join(format!("{}.sujian-ref-lock", ref_name));
            // 创建父目录（refs/heads/ 等）。
            if let Some(parent) = owner_marker.parent() {
                fs::create_dir_all(parent)?;
            }
            // 原子写 owner marker。
            if let Err(e) = crate::storage::atomic_write_bytes(&owner_marker, &metadata) {
                // 清理已写的 owner marker。
                for m in &written_markers {
                    let _ = fs::remove_file(m);
                }
                return Err(e);
            }
            owner_markers.push(owner_marker.clone());
            written_markers.push(owner_marker);
        }

        // 2. 创建 git2::Transaction 并 lock 全部 refs。
        let mut tx = repo.transaction().map_err(|e| {
            // 清理已写的 owner marker。
            for m in &written_markers {
                let _ = fs::remove_file(m);
            }
            crate::Error::Io(std::io::Error::other(format!(
                "RefTransaction: create transaction: {e}"
            )))
        })?;

        for ref_name in &sorted_refs {
            if let Err(e) = tx.lock_ref(ref_name) {
                // 清理已写的 owner marker。
                for m in &written_markers {
                    let _ = fs::remove_file(m);
                }
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "RefTransaction: lock_ref {}: {e}",
                    ref_name
                ))));
            }
        }

        Ok(Self {
            repo,
            tx: Some(tx),
            locked_refs: sorted_refs,
            owner_markers,
            disarmed: false,
        })
    }

    /// #644 评论 5489192105：在锁保护下读取 ref 当前值。
    ///
    /// 必须在 `acquire_all_refs` 之后、`commit` 之前调用。
    ///
    /// 返回的 `git2::Reference<'repo>` 借用底层的 `git2::Repository`
    /// （生命周期 `'repo`），不借用 `RefTransaction` 本身。这样可以在
    /// `commit`（move `RefTransaction`）之前 drop `Reference`，不会阻止 commit。
    pub fn find_reference(
        &self,
        ref_name: &str,
    ) -> std::result::Result<git2::Reference<'repo>, git2::Error> {
        self.repo.find_reference(ref_name)
    }

    /// #644 评论 5489192105：set ref target（通过 git2::Transaction，正确处理 packed refs）。
    ///
    /// ref 必须已通过 `acquire_all_refs` lock。
    /// `reflog_message` 写入 reflog。
    pub fn set_target(
        &mut self,
        ref_name: &str,
        target: git2::Oid,
        reflog_message: &str,
    ) -> Result<()> {
        let tx = self.tx.as_mut().ok_or_else(|| {
            crate::Error::Io(std::io::Error::other(
                "RefTransaction: already committed or dropped",
            ))
        })?;
        tx.set_target(ref_name, target, None, reflog_message)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "RefTransaction: set_target {}: {e}",
                    ref_name
                )))
            })
    }

    /// #644 评论 5489192105：set symbolic ref target（通过 git2::Transaction）。
    ///
    /// ref 必须已通过 `acquire_all_refs` lock。
    pub fn set_symbolic_target(
        &mut self,
        ref_name: &str,
        target: &str,
        reflog_message: &str,
    ) -> Result<()> {
        let tx = self.tx.as_mut().ok_or_else(|| {
            crate::Error::Io(std::io::Error::other(
                "RefTransaction: already committed or dropped",
            ))
        })?;
        tx.set_symbolic_target(ref_name, target, None, reflog_message)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "RefTransaction: set_symbolic_target {}: {e}",
                    ref_name
                )))
            })
    }

    /// #644 评论 5489192105：remove ref（通过 git2::Transaction，正确处理 packed refs）。
    ///
    /// ref 必须已通过 `acquire_all_refs` lock。
    /// 正确处理 loose ref 和 packed-refs 两种存储形式。
    pub fn remove(&mut self, ref_name: &str) -> Result<()> {
        let tx = self.tx.as_mut().ok_or_else(|| {
            crate::Error::Io(std::io::Error::other(
                "RefTransaction: already committed or dropped",
            ))
        })?;
        tx.remove(ref_name).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "RefTransaction: remove {}: {e}",
                ref_name
            )))
        })
    }

    /// #644 评论 5489192105：commit 事务。
    ///
    /// `tx.commit()` 提交所有 set/remove 操作，释放所有 lock。
    /// 成功后删除所有 owner marker。
    ///
    /// 消耗 self，commit 后不能再使用。
    #[allow(clippy::excessive_nesting)]
    pub fn commit(mut self) -> Result<()> {
        let tx = self.tx.take().ok_or_else(|| {
            crate::Error::Io(std::io::Error::other(
                "RefTransaction: already committed or dropped",
            ))
        })?;
        tx.commit().map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "RefTransaction: commit: {e}"
            )))
        })?;

        // commit 成功，删除所有 owner marker。
        for marker in &self.owner_markers {
            if marker.exists() {
                if let Err(e) = fs::remove_file(marker) {
                    log::warn!(
                        "RefTransaction: failed to clean owner marker after commit: {}: {}",
                        marker.display(),
                        e
                    );
                }
            }
        }
        // fsync 父目录。
        for marker in &self.owner_markers {
            if let Some(parent) = marker.parent() {
                let _ = crate::storage::sync_dir(parent);
            }
        }

        self.disarmed = true;
        Ok(())
    }

    /// #644 评论 5489192105：获取已 lock 的 ref name 列表。
    pub fn locked_refs(&self) -> &[String] {
        &self.locked_refs
    }

    /// #644 评论 5489192105：获取底层 `git2::Repository` 引用。
    ///
    /// 用于在锁保护下读取 ref 当前值（如 `classify_locked_ref_rollback`）。
    /// 返回的引用生命周期与 `RefTransaction` 相同，确保读取在锁保护下完成。
    pub fn repo(&self) -> &git2::Repository {
        self.repo
    }
}

impl<'repo> Drop for RefTransaction<'repo> {
    fn drop(&mut self) {
        if !self.disarmed {
            // 未 commit（出错路径）：清理 owner marker。
            // tx 仍存在（Option<Some>），drop 时 git2::Transaction::drop 会释放 lock。
            // tx 是 None 表示 commit 中途失败（take 后 err），lock 已由 tx.commit 内部处理。
            for marker in &self.owner_markers {
                let _ = fs::remove_file(marker);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_lock_owner_metadata_roundtrip() {
        let owner = "test-uuid-1234";
        let metadata = ref_lock_owner_metadata(owner);
        assert!(ref_lock_belongs_to_owner(&metadata, owner));
        assert!(!ref_lock_belongs_to_owner(&metadata, "other-uuid"));
        assert!(!ref_lock_belongs_to_owner(b"garbage", owner));
        assert!(!ref_lock_belongs_to_owner(b"", owner));
    }

    #[test]
    fn ref_lock_owner_metadata_format() {
        let metadata = ref_lock_owner_metadata("abc");
        let text = std::str::from_utf8(&metadata).expect("valid utf8");
        assert!(text.starts_with("sujian-ref-lock-v1\n"));
        assert!(text.contains("owner=abc\n"));
    }
}
