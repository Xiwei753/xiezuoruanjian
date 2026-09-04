/// 本地 workspace Git commit 的结果。
#[derive(Debug)]
pub struct WorkspaceCommitResult {
    /// 本次 commit 的 OID；`None` 表示没有变更需要提交。
    pub oid: Option<git2::Oid>,
    /// 本次 stage 的 workspace-relative paths 数量。
    pub staged_count: usize,
}
