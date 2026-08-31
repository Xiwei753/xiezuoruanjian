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

    /// 三方比较生成 commit plan。
    ///
    /// - `base` = Prepare 时 live（[Self::base_root] 下）
    /// - `local` = 现在 live（`live_root` 下）
    /// - `incoming` = Transfer 后 staging（[Self::staging_root] 下）
    ///
    /// 判断（对每个 incoming 文件）：
    /// - `local == base`：安全应用 incoming → [CommitAction::Apply]
    /// - `incoming == base`：远端没变，保留 local → [CommitAction::KeepLocal]
    /// - 两边都改（local != base && incoming != base）：三方冲突 → [CommitAction::Conflict]
    ///
    /// local 独有（incoming 没有）的文件不在本方法输出里（保留 local，无需动作）。
    /// incoming 独有（base/local 都没有）按 [CommitAction::Apply]（新增文件）。
    pub fn compute_commit_plan(&self, live_root: &Path) -> Result<CommitPlan> {
        let staging_root = self.staging_root();
        let base_root = self.base_root();
        let mut plan = CommitPlan::default();
        if !staging_root.exists() {
            return Ok(plan);
        }
        let rels = list_relative_paths(&staging_root)?;
        for rel in rels {
            let staging_path = staging_root.join(&rel);
            let base_path = base_root.join(&rel);
            let live_path = live_root.join(&rel);

            let incoming = read_bytes(&staging_path)?;
            let base = if base_path.exists() {
                Some(read_bytes(&base_path)?)
            } else {
                None
            };
            let local = if live_path.exists() {
                Some(read_bytes(&live_path)?)
            } else {
                None
            };

            let incoming_eq_base = match (&base, &incoming) {
                (Some(b), i) => b == i,
                (None, _) => false,
            };
            let local_eq_base = match (&base, &local) {
                (Some(b), Some(l)) => b == l,
                _ => false,
            };

            if incoming_eq_base {
                // 远端没变，保留 local。
                plan.keep_local.push(rel);
            } else if local_eq_base {
                // local 没动，安全应用 incoming。
                plan.apply.push(CommitAction::Apply {
                    rel_path: rel,
                    content: incoming,
                });
            } else {
                // 两边都改（或 base 不存在且 local 已改），三方冲突。
                plan.conflict.push(rel);
            }
        }
        Ok(plan)
    }

    /// 清理整个 run 目录。
    pub fn cleanup(&self) {
        let _ = fs::remove_dir_all(&self.run_root);
    }
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
    /// 两边都改，三方冲突（正文走三方合并语义，metadata 走 LWW，由调用方决定）。
    pub conflict: Vec<PathBuf>,
}

/// 单个文件的 commit 动作。
#[derive(Debug)]
pub enum CommitAction {
    /// 把 `content` 写到 `rel_path`（相对 target_root）。
    Apply { rel_path: PathBuf, content: Vec<u8> },
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
}
