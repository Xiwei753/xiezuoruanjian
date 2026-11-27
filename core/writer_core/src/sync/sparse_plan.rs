//! 对象级增量同步的输入契约（Issue #1065 复审 #7213234617 第 2 项）。
//!
//! 旧三方引擎把"目录里没有这个文件"当成"本地删除"。一旦只为受影响对象建
//! 快照，未 materialize 的正常对象就会被误判删除、并触发全量基线重建。
//! 这里把对象状态显式化：
//!
//! - [`ObjectState::Unloaded`]：本轮没有加载它的字节，**不代表删除**，必须沿用
//!   已确认的 manifest/known_files 事实；
//! - [`ObjectState::Present`]：本轮有确定内容（摘要），参与合并与发布；
//! - [`ObjectState::Deleted`]：有显式删除证据（durable 删除事务、tombstone 或
//!   远端删除事实），才允许进入删除决议。
//!
//! [`SparseObjectPlan`] 是本轮唯一允许读写的对象集合：affected set = 未确认
//! durable mutation ∪ 固定 HEAD 的 tree diff ∪ 必要依赖闭包。引擎其余部分
//! 迭代受影响集合，而不是枚举整个目标目录。

use std::collections::{BTreeMap, BTreeSet};

/// 单个对象在本轮已知的状态。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ObjectState {
    /// 未加载：本轮不读字节、不改写、不参与删除判定。
    Unloaded,
    /// 已加载：带有本轮确定的字节摘要（MD5）。
    Present { hash: String },
    /// 有删除证据；`reason` 用于诊断与幂等重放。
    Deleted { reason: DeleteEvidence },
}

/// 删除证据来源。没有证据的"文件不见了"不能构造 [`ObjectState::Deleted`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeleteEvidence {
    /// 本地 durable 删除事务（章节删除、作品生命周期删除）。
    LocalTransaction,
    /// 已持久化的 tombstone。
    Tombstone,
    /// 远端 tree diff 明确删除了该路径。
    RemoteDiff,
}

/// 本轮的稀疏对象计划。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SparseObjectPlan {
    /// 计划所基于的远端提交（固定 HEAD）；空表示首次同步/迁移。
    base_head: Option<String>,
    /// affected set：只有这里的对象允许被读取、合并、发布或删除。
    affected: BTreeSet<String>,
    /// 已确定状态的对象；不在表里的 affected 路径默认 [`ObjectState::Unloaded`]。
    states: BTreeMap<String, ObjectState>,
    /// 对象依赖闭包（例如星图 graph.json ↔ 对象文件、章节正文 ↔ meta）。
    dependencies: BTreeMap<String, BTreeSet<String>>,
    /// 本轮固定输入：短事务锁内取得的未确认 mutation 事实（含 seq/op/base
    /// hash/content hash/业务时间/删除时间）。它不是 live 账文的副本，而是
    /// 本轮唯一允许使用的对象版本集合；后续编辑产生下一版本，不改变本轮输入。
    #[serde(default)]
    mutations: BTreeMap<String, crate::sync::local_mutations::LocalMutationRecord>,
}

impl SparseObjectPlan {
    /// 计划随 staging run 一起落盘，merge/commit 在各自阶段按同一份计划工作，
    /// 不需要把计划塞进每个调用签名。
    const FILE_NAME: &'static str = "app-meta/sync/sparse-plan.json";

    pub(crate) fn save(&self, root: &std::path::Path) -> crate::error::Result<()> {
        let path = root.join(Self::FILE_NAME);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string(self)?;
        crate::storage::atomic_write_string(&path, &text)
    }

    pub(crate) fn load(root: &std::path::Path) -> Option<Self> {
        Self::load_checked(root).ok().flatten()
    }

    /// 严格加载：计划文件不存在表示本阶段没有稀疏输入（None）；存在但读不到
    /// 或解析失败必须报错，不能被调用方静默降级成"全量模式"（评审 #7214314073）。
    pub(crate) fn load_checked(root: &std::path::Path) -> crate::error::Result<Option<Self>> {
        let path = root.join(Self::FILE_NAME);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(crate::error::Error::Io(std::io::Error::new(
                    error.kind(),
                    format!("read sparse object plan at {}: {error}", path.display()),
                )))
            }
        };
        let plan = serde_json::from_str(&text).map_err(|error| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("parse sparse object plan at {}: {error}", path.display()),
            ))
        })?;
        Ok(Some(plan))
    }

    pub(crate) fn remove(root: &std::path::Path) {
        let _ = std::fs::remove_file(root.join(Self::FILE_NAME));
    }
}

/// 计划文件相对工作根的路径；staging 扫描必须跳过它。
pub const PLAN_FILE_NAME: &str = "app-meta/sync/sparse-plan.json";

impl SparseObjectPlan {
    /// 用本轮三类输入构造计划：未确认 mutation、远端 tree diff、必需依赖。
    pub(crate) fn new(
        base_head: Option<String>,
        pending_paths: impl IntoIterator<Item = String>,
        remote_changed_paths: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut affected: BTreeSet<String> = BTreeSet::new();
        affected.extend(pending_paths);
        affected.extend(remote_changed_paths);
        Self {
            base_head,
            affected,
            states: BTreeMap::new(),
            dependencies: BTreeMap::new(),
            mutations: BTreeMap::new(),
        }
    }

    /// 用本轮固定的未确认 mutation 事实填充计划（同时进入 affected set）。
    pub(crate) fn with_mutations(
        mut self,
        mutations: BTreeMap<String, crate::sync::local_mutations::LocalMutationRecord>,
    ) -> Self {
        for (path, record) in &mutations {
            self.affected.insert(path.clone());
            let state = if record.is_delete() {
                ObjectState::Deleted {
                    reason: DeleteEvidence::LocalTransaction,
                }
            } else {
                ObjectState::Present {
                    hash: record.content_hash.clone(),
                }
            };
            self.states.insert(path.clone(), state);
        }
        self.mutations = mutations;
        self
    }

    /// 本轮的未确认 mutation 事实（path → 最新事实）。
    pub(crate) fn mutations(
        &self,
    ) -> &BTreeMap<String, crate::sync::local_mutations::LocalMutationRecord> {
        &self.mutations
    }

    /// 是否处于"对象级增量"模式；没有固定基线（首次同步/迁移）时只能完整核对。
    pub(crate) fn is_incremental(&self) -> bool {
        self.base_head.is_some()
    }

    pub(crate) fn base_head(&self) -> Option<&str> {
        self.base_head.as_deref()
    }

    /// 本轮允许触碰的路径集合。
    pub(crate) fn affected_paths(&self) -> &BTreeSet<String> {
        &self.affected
    }

    pub(crate) fn contains(&self, path: &str) -> bool {
        self.affected.contains(path)
    }

    /// 查询对象状态；affected 之外的路径一律是 `Unloaded`（沿用已确认事实）。
    pub(crate) fn state_of(&self, path: &str) -> ObjectState {
        match self.states.get(path) {
            Some(state) => state.clone(),
            None => ObjectState::Unloaded,
        }
    }

    /// 登记一个本轮已加载的对象。
    pub(crate) fn mark_present(&mut self, path: impl Into<String>, hash: impl Into<String>) {
        let path = path.into();
        self.affected.insert(path.clone());
        self.states
            .insert(path, ObjectState::Present { hash: hash.into() });
    }

    /// 登记一个有明确证据的删除。调用方必须给出证据；没有证据时不要调用。
    pub(crate) fn mark_deleted(&mut self, path: impl Into<String>, reason: DeleteEvidence) {
        let path = path.into();
        self.affected.insert(path.clone());
        self.states.insert(path, ObjectState::Deleted { reason });
    }

    /// 显式扩展依赖闭包：依赖对象同样进入 affected set。
    pub(crate) fn add_dependencies(
        &mut self,
        path: impl Into<String>,
        deps: impl IntoIterator<Item = String>,
    ) {
        let path = path.into();
        let entry = self.dependencies.entry(path).or_default();
        for dep in deps {
            self.affected.insert(dep.clone());
            entry.insert(dep);
        }
    }

    /// 某个对象的依赖集合。
    pub(crate) fn dependencies_of(&self, path: &str) -> Option<&BTreeSet<String>> {
        self.dependencies.get(path)
    }

    /// 本轮可以被删除的路径（只含显式证据）。
    pub(crate) fn deletable_paths(&self) -> impl Iterator<Item = &str> {
        self.states.iter().filter_map(|(path, state)| match state {
            ObjectState::Deleted { .. } => Some(path.as_str()),
            _ => None,
        })
    }
}
