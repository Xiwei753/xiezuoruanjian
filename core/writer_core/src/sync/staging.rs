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
    /// 三方决策：
    /// - `local == base && incoming != base` → Apply incoming（含 incoming=None 的删除）
    /// - `incoming == base` → KeepLocal（远端没变）
    /// - `local == incoming` → NoOp（内容相同，无需操作）
    /// - 其它 → Conflict
    ///
    /// `incoming=None`（远端删除）+ `local==base` → [CommitAction::Delete]。
    /// `incoming=None` + `local!=base` → Conflict（本地改了、远端删了）。
    #[allow(clippy::excessive_nesting)]
    pub fn compute_commit_plan(&self, live_root: &Path) -> Result<CommitPlan> {
        let staging_root = self.staging_root();
        let base_root = self.base_root();
        let mut plan = CommitPlan::default();

        // 收集 base ∪ staging 的路径全集
        let base_paths = if base_root.exists() {
            list_relative_paths(&base_root)?
        } else {
            Vec::new()
        };
        let staging_paths = if staging_root.exists() {
            list_relative_paths(&staging_root)?
        } else {
            Vec::new()
        };
        let mut all_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for p in &base_paths {
            all_paths.insert(p.clone());
        }
        for p in &staging_paths {
            all_paths.insert(p.clone());
        }

        for rel in all_paths {
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

            if incoming_eq_base {
                // 远端没变，保留 local。
                plan.keep_local.push(rel);
            } else if local_eq_base {
                // local 没动，安全应用 incoming。
                match incoming {
                    Some(content) => {
                        plan.apply.push(CommitAction::Apply {
                            rel_path: rel,
                            content,
                        });
                    }
                    None => {
                        // 远端删除，local 没改 → 删除本地文件
                        plan.apply.push(CommitAction::Delete { rel_path: rel });
                    }
                }
            } else if local_eq_incoming {
                // 内容相同，无需操作。
                plan.noop.push(rel);
            } else {
                // 两边都改（或 base 不存在且 local 已改），三方冲突。
                plan.conflict.push(StagingConflict {
                    rel_path: rel,
                    base_hash: md5_hex(&base),
                    local_hash: md5_hex(&local),
                    incoming_hash: md5_hex(&incoming),
                });
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
/// 对每个 target 创建 `StagingRun`，然后调 `seed_from_live` 从 live 初始化
/// base snapshot + staging clone。
///
/// #644 评论 5473401065 第2节：seed 失败**不再**被 `log::warn!` 吞掉。
/// 任何一个 target 的 seed 失败都意味着该 target 的 staging 是半成品，
/// 不能拿来做三方比较。seed 失败直接返回 Err，让调用方终止本次 full sync。
///
/// 成功后 `plan.targets[*].staging_root` 被填充为对应 staging 目录。
pub fn prepare_staging_runs(
    plan: &mut crate::sync::full_sync::FullSyncPlan,
) -> crate::error::Result<Vec<StagingRun>> {
    let mut staging_runs: Vec<StagingRun> = Vec::new();

    for planned in &mut plan.targets {
        let run = StagingRun::create(&plan.app_data_root, planned.target_live_root.clone())?;
        // #644 评论 5473401065 第2节：seed 失败必须传播，不能继续拿半成品 staging。
        run.seed_from_live(&planned.target_live_root)?;
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

/// Commit plan — Commit 阶段对每个 staging 变化的处理决策。
#[derive(Default, Debug)]
pub struct CommitPlan {
    /// local==base，安全应用 incoming（含 incoming 独有的新增文件）。
    pub apply: Vec<CommitAction>,
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
#[derive(Debug)]
pub enum CommitAction {
    /// 把 `content` 写到 `rel_path`（相对 target_root）。
    Apply { rel_path: PathBuf, content: Vec<u8> },
    /// 删除 `rel_path`（远端删除，local 没改）。
    Delete { rel_path: PathBuf },
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

/// 递归列出 `root` 下所有文件的相对路径。
fn list_relative_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    walk(root, root, &mut out)?;
    Ok(out)
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
    walk_live(live_root, live_root, &mut out)?;
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out)?;
        } else {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
    }
    Ok(())
}

#[allow(clippy::excessive_nesting)]
fn walk_live(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Ok(rel) = path.strip_prefix(root) {
                let rel_str = rel.to_string_lossy();
                // 跳过内部目录
                if rel_str == ".git"
                    || rel_str == "full-sync-staging"
                    || rel_str.starts_with("app-meta/transactions")
                {
                    continue;
                }
            }
            walk_live(root, &path, out)?;
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
        let run = StagingRun::create(tmp.path(), live).unwrap();
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

        let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
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

        let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
        run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
            .unwrap();
        // local 仍是 base（没动），incoming 改了。
        fs::write(run.staging_root().join("f.txt"), "incoming").unwrap();

        let plan = run.compute_commit_plan(&live).unwrap();
        assert_eq!(plan.apply.len(), 1);
        assert!(plan.keep_local.is_empty());
        assert!(plan.conflict.is_empty());
    }

    #[test]
    fn commit_plan_incoming_eq_base_keeps_local() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("f.txt"), "base").unwrap();

        let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
        run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
            .unwrap();
        // local 改了（走 atomic_write rename 替换，hard-link 的 base 保留旧 inode），
        // incoming == base。
        crate::storage::atomic_write_string(&live.join("f.txt"), "local-changed").unwrap();
        fs::write(run.staging_root().join("f.txt"), "base").unwrap();

        let plan = run.compute_commit_plan(&live).unwrap();
        assert_eq!(plan.keep_local.len(), 1);
        assert!(plan.apply.is_empty());
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

        let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
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
        fs::write(live.join("f.txt"), "base").unwrap();

        let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
        run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
            .unwrap();
        // local 改了（atomic_write rename 替换，base 保留旧 inode）。
        crate::storage::atomic_write_string(&live.join("f.txt"), "local-changed").unwrap();
        fs::write(run.staging_root().join("f.txt"), "incoming-changed").unwrap();

        let plan = run.compute_commit_plan(&live).unwrap();
        assert_eq!(plan.conflict.len(), 1);
        assert!(plan.apply.is_empty());
        assert!(plan.keep_local.is_empty());
    }

    #[test]
    fn commit_plan_remote_delete_local_unchanged_produces_delete() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("f.txt"), "base").unwrap();

        let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
        run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
            .unwrap();
        // 远端删除：staging 中没有 f.txt（incoming=None），local 没改。
        // base ∪ staging = {f.txt}（来自 base），三方比较：local==base, incoming=None → Delete。

        let plan = run.compute_commit_plan(&live).unwrap();
        assert_eq!(plan.apply.len(), 1);
        assert!(matches!(plan.apply[0], CommitAction::Delete { .. }));
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
        let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
        // base 为空（没有 build_base_snapshot）
        fs::write(run.staging_root().join("f.txt"), "new-from-remote").unwrap();

        let plan = run.compute_commit_plan(&live).unwrap();
        // base=None, local=None, incoming=Some → local==base (both None), incoming!=base → Apply
        assert_eq!(plan.apply.len(), 1);
        assert!(plan.conflict.is_empty());
    }

    #[test]
    fn commit_plan_local_changed_remote_deleted_is_conflict() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("f.txt"), "base").unwrap();

        let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
        run.build_base_snapshot_from_live(&live, &[PathBuf::from("f.txt")])
            .unwrap();
        // local 改了，远端删除（staging 没有 f.txt）。
        crate::storage::atomic_write_string(&live.join("f.txt"), "local-changed").unwrap();

        let plan = run.compute_commit_plan(&live).unwrap();
        // local!=base, incoming=None → local!=incoming → Conflict
        assert_eq!(plan.conflict.len(), 1);
        assert!(plan.apply.is_empty());
    }
}
