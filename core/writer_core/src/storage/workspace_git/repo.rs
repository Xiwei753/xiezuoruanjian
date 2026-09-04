use crate::storage::git_repo_layout::GitRepoLayout;
use crate::Result;

/// 确保唯一 workspace Git 仓库存在（init if missing）。
///
/// 委托给 `git_repo_layout::ensure_repo_layout_initialized`，
/// 该函数已包含布局迁移、外置 git_dir 处理和 init 逻辑。
///
/// 此函数在应用打开 workspace 时调用（bootstrap），不在同步流程中调用，
/// 保证本地 Git 历史生命周期独立于 SyncProvider。
pub fn ensure_workspace_repo(layout: &GitRepoLayout) -> Result<()> {
    crate::storage::git_repo_layout::ensure_repo_layout_initialized(layout)
}

/// 打开 workspace Git 仓库。
pub fn open_workspace_repo(layout: &GitRepoLayout) -> Result<git2::Repository> {
    crate::storage::git_repo_layout::open_repo(layout)
}
