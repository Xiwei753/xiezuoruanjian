//! 本地提交事务产出的对象决议（Issue #1065 复审 #7213234617 第 1 项）。
//!
//! 远端发布只能由这里的事实驱动：每个对象在本地事务里已经确定要发布什么、
//! 是否已经被远端消费、是否属于冲突/被取代。发布前不再从"transfer 阶段
//! 预先生成的缓冲"或"回读 live 猜"构造批次，manifest 也从同一批决议对应的
//! 已落盘 manifest 取字节，保证请求体内的对象与描述互相一致。

/// 应用决议的结果。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DecisionApplyOutcome {
    /// 用已落盘字节替换了缓冲内容的条数。
    pub rewritten: usize,
    /// 决议要求本轮不发布的条数（消费/冲突）。
    pub withheld: usize,
    /// 决议要求上传但本地文件已不存在的条数（不能发布旧内容）。
    pub missing: usize,
}

/// 本地业务提交的不变对象决议（复审 #7213791503/#7214105551 第 2 项）。
///
/// 内容在**持锁的业务提交阶段**就已冻结，之后不再回读 live；`transaction_id`/`mutation_seq`/`base_hash` 来自真实
/// durable 事实，`origin` 说明它是本地改动、真实合并还是纯远端消费。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecisionOp {
    Upsert,
    Delete,
}

/// 决议来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecisionOrigin {
    /// 本地业务事务产生（章节提交、设置保存、变更集投影）。
    Local,
    /// 本地与远端真实合并后的结果。
    Merged,
    /// 纯远端落盘：本地只消费，不再上传。
    RemoteConsumed,
    /// 本地与远端都改过且未决：本轮不得发布旧本地版本（只能沿用固定远端基线）。
    Conflict,
}

/// 单个对象的不可变决议。
#[derive(Debug, Clone)]
pub(crate) struct DurableObjectDecision {
    /// 业务事务 ID（章节 journal / 变更集 message / 同步轮次）。
    pub(crate) transaction_id: String,
    /// 对象在 target 内的相对路径（identity）。
    pub(crate) identity: String,
    pub(crate) op: DecisionOp,
    /// 提交前的已确认基线摘要（没有基线时为 None）。
    pub(crate) base_hash: Option<String>,
    /// 提交后的内容摘要；delete 时为空。
    pub(crate) final_hash: String,
    /// 对象业务版本（meta revision 等）。
    pub(crate) final_version: Option<u64>,
    /// 对应 mutation 的账本序号（0 表示尚未投影）。
    pub(crate) mutation_seq: u64,
    pub(crate) origin: DecisionOrigin,
    /// 冻结的最终内容；delete 时为 None。
    pub(crate) content: Option<std::sync::Arc<[u8]>>,
    /// 依赖对象（同一事务集合内必须一起发布的 identity）。
    pub(crate) dependencies: Vec<String>,
}

impl DurableObjectDecision {
    /// 本轮是否允许把该对象写进远端批次。
    pub(crate) fn is_publishable(&self) -> bool {
        matches!(self.origin, DecisionOrigin::Local | DecisionOrigin::Merged)
    }
}

/// 从提交阶段冻结的上传/删除清单构造不可变对象决议。
///
/// 内容、版本、基线、mutation 序号都来自统一裁决的冻结结果；这里不再回读
/// live，也不再因读失败静默跳过对象——冻结内容与 final hash 不一致就是提交
/// 之后有人改写了对象，本轮发布失败。
pub(crate) fn publish_decisions(
    uploads: &[crate::sync::staging::PublishObject],
    deletes: &[std::path::PathBuf],
) -> crate::error::Result<Vec<DurableObjectDecision>> {
    let mut decisions = Vec::new();
    for object in uploads {
        let identity = object.rel_path.to_string_lossy().replace('\\', "/");
        if identity.is_empty() {
            continue;
        }
        let final_hash = crate::sync::hash::content_md5(&object.content);
        if !object.final_hash.is_empty() && final_hash != object.final_hash {
            return Err(crate::Error::Other(format!(
                "publish decision content changed after commit: path={identity} \
                 expected={} actual={final_hash}",
                object.final_hash
            )));
        }
        decisions.push(DurableObjectDecision {
            transaction_id: object.transaction_id.clone(),
            identity,
            op: DecisionOp::Upsert,
            base_hash: object.base_hash.clone(),
            final_hash,
            final_version: None,
            mutation_seq: object.mutation_seq,
            origin: DecisionOrigin::Local,
            content: Some(object.content.clone()),
            dependencies: Vec::new(),
        });
    }
    for path in deletes {
        let identity = path.to_string_lossy().replace('\\', "/");
        if identity.is_empty() {
            continue;
        }
        decisions.push(DurableObjectDecision {
            transaction_id: String::new(),
            identity,
            op: DecisionOp::Delete,
            base_hash: None,
            final_hash: String::new(),
            final_version: None,
            mutation_seq: 0,
            origin: DecisionOrigin::Local,
            content: None,
            dependencies: Vec::new(),
        });
    }
    Ok(decisions)
}
