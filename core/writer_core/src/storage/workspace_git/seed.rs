//! Git 仓库状态枚举。
//!
//! 用于 workspace 仓库的 finalize / crash recovery 路径判断。

/// workspace 仓库在 seed / finalize 时的 Git 状态。
#[derive(Debug, Clone)]
pub enum GitSeedState {
    /// workspace 不是 Git repo（没有 `.git/`）。
    NotGitRepo,
    /// workspace 是 Git repo 但 HEAD 是 unborn（`git init` 后尚未提交）。
    /// `head_ref` 是 symbolic HEAD 的真实目标引用名（如 `refs/heads/main`）。
    Unborn { head_ref: String },
    /// workspace 是 Git repo 且 HEAD 指向一个已存在的 commit。
    /// `head_ref` 是当前分支引用名，`head_oid` 是 seed 时的 HEAD OID。
    Existing {
        head_ref: String,
        head_oid: git2::Oid,
    },
    /// workspace 是 Git repo 但 HEAD 是 detached。
    /// `head_oid` 是 detached HEAD 指向的 commit OID。
    Detached { head_oid: git2::Oid },
}
