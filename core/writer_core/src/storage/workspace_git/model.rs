use std::path::PathBuf;

/// 本地 workspace Git commit 的结果。
#[derive(Debug)]
pub struct WorkspaceCommitResult {
    /// 本次 commit 的 OID；`None` 表示没有变更需要提交。
    pub oid: Option<git2::Oid>,
    /// 本次 stage 的 workspace-relative paths 数量。
    pub staged_count: usize,
}

/// #645 评论 5504296097 问题2：本地写结果统一成存储层事实。
///
/// 每个变体描述一种底层持久化真正执行过的文件变更，由真正执行写入/删除的
/// 持久化函数返回，API 层只转交给 `workspace_git`，不再手工猜路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceHistoryChange {
    /// 新增或修改文件（stage 到 index）。路径为 workspace-relative。
    Upsert(PathBuf),
    /// 删除单个文件（`index.remove_path`）。路径为 workspace-relative。
    Delete(PathBuf),
    /// #645 评论 5504296097 问题2(b/c)：删除整棵子树。
    ///
    /// 在 Git index 层按 prefix 删除所有 tracked entries（遍历 index entries，
    /// 对路径前缀匹配的调 `index.remove_path`），不重新扫描整个 workspace。
    /// 路径为 workspace-relative 目录前缀（如 `projects/{pid}`）。
    DeleteTree(PathBuf),
}

/// #645 评论 5504296097 问题2：一次写事务的真实变更集合。
///
/// 由底层持久化函数返回，API 层转交给 `record_workspace_change_set`。
/// 提供 builder 方法（[`Self::add_upsert`] / [`Self::add_delete`] /
/// [`Self::add_delete_tree`]）方便构造。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceChangeSet {
    pub changes: Vec<WorkspaceHistoryChange>,
}

impl WorkspaceChangeSet {
    /// 空变更集。
    pub fn new() -> Self {
        Self::default()
    }

    /// 从单个 change 构造。
    pub fn from_change(change: WorkspaceHistoryChange) -> Self {
        Self {
            changes: vec![change],
        }
    }

    /// 添加一个 Upsert 变更。
    pub fn add_upsert(mut self, path: PathBuf) -> Self {
        self.changes.push(WorkspaceHistoryChange::Upsert(path));
        self
    }

    /// 添加一个 Delete 变更。
    pub fn add_delete(mut self, path: PathBuf) -> Self {
        self.changes.push(WorkspaceHistoryChange::Delete(path));
        self
    }

    /// 添加一个 DeleteTree 变更。
    pub fn add_delete_tree(mut self, prefix: PathBuf) -> Self {
        self.changes
            .push(WorkspaceHistoryChange::DeleteTree(prefix));
        self
    }

    /// 合并另一个变更集。
    pub fn merge(mut self, other: WorkspaceChangeSet) -> Self {
        self.changes.extend(other.changes);
        self
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// #645 评论 5504296097 问题2：把变更集展开成扁平的 workspace-relative path 列表。
    ///
    /// 供需要 `&[PathBuf]` 的旧接口或测试断言使用。`DeleteTree` 展开成其前缀
    /// 本身（仅作 path 表示，实际删除语义在 `record_workspace_change_set` 中
    /// 按 prefix 处理）。
    pub fn to_flat_paths(&self) -> Vec<PathBuf> {
        self.changes
            .iter()
            .map(|c| match c {
                WorkspaceHistoryChange::Upsert(p)
                | WorkspaceHistoryChange::Delete(p)
                | WorkspaceHistoryChange::DeleteTree(p) => p.clone(),
            })
            .collect()
    }
}

impl From<WorkspaceHistoryChange> for WorkspaceChangeSet {
    fn from(change: WorkspaceHistoryChange) -> Self {
        Self::from_change(change)
    }
}
