use std::fs;
use std::path::{Path, PathBuf};

// ── RAII 临时目录守卫 ──

/// #644 评论 5475805198 第4节：RAII 守卫，保证临时目录在 drop 时删除。
pub(crate) struct TmpDirGuard(Option<PathBuf>);

#[allow(clippy::expect_used)]
impl TmpDirGuard {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }

    pub(crate) fn path(&self) -> &Path {
        // SAFETY: 构造后 `0` 始终为 Some，直到 `disarm()` 置 None。
        self.0.as_ref().expect("TmpDirGuard already disarmed")
    }

    /// 取消自动删除（成功 rename 后调用），返回路径。
    pub(crate) fn disarm(&mut self) -> PathBuf {
        self.0.take().expect("TmpDirGuard already disarmed")
    }
}

impl Drop for TmpDirGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

/// #644 评论 5480360027：RAII 守卫，保证临时文件在 drop 时删除。
pub(crate) struct TmpFileGuard(PathBuf);

impl TmpFileGuard {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self(path)
    }
}

impl Drop for TmpFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

// #644 评论 5481496190 问题2：IndexLockGuard 已删除。
// install_index_with_lock 改用 lockfile rename 模型（把内容直接写入 lock 文件，
// rename lock → index 作为提交和解锁），不再需要独立的 RAII lock 守卫。

/// #644 评论 5492740265 问题2：repo install tmp 路径的统一计算。
///
/// tmp 永远建在**最终 live_git 的父目录**，保证 `tmp -> live_git` 是同一文件系统的
/// 原子 rename。Android 上 live_root（共享 worktree）和 live_git（私有 git_dir）
/// 在不同文件系统，把 tmp 建在 live_root 会导致 rename 跨 mount 失败。
///
/// - `explicit_git_dir = Some(p)`：tmp 建在 `p.parent().join(".git.sujian-tmp-{owner}")`。
/// - `explicit_git_dir = None`：tmp 建在 `live_root.join(".git.sujian-tmp-{owner}")`
///   （标准布局，live_git = live_root/.git，父目录就是 live_root）。
///
/// forward（`finalize_not_git_repo`）和 recovery（`rollback_git_finalize`）必须对同一
/// owner 算出同一物理 tmp 路径，否则崩溃后找不到自己留下的 repo。
pub(crate) fn repo_install_tmp_path(
    live_root: &Path,
    explicit_git_dir: Option<&Path>,
    owner: &str,
) -> crate::error::Result<PathBuf> {
    match explicit_git_dir {
        Some(git_dir) => {
            let parent = git_dir.parent().ok_or_else(|| {
                crate::Error::Io(std::io::Error::other(format!(
                    "repo_install_tmp_path: explicit_git_dir has no parent: {}",
                    git_dir.display(),
                )))
            })?;
            Ok(parent.join(format!(".git.sujian-tmp-{}", owner)))
        }
        None => Ok(live_root.join(format!(".git.sujian-tmp-{}", owner))),
    }
}
