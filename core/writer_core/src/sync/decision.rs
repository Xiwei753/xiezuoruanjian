//! 本地提交事务产出的对象决议（Issue #1065 复审 #7213234617 第 1 项）。
//!
//! 远端发布只能由这里的事实驱动：每个对象在本地事务里已经确定要发布什么、
//! 是否已经被远端消费、是否属于冲突/被取代。发布前不再从"transfer 阶段
//! 预先生成的缓冲"或"回读 live 猜"构造批次，manifest 也从同一批决议对应的
//! 已落盘 manifest 取字节，保证请求体内的对象与描述互相一致。

use std::path::PathBuf;

/// 单个对象在本轮的处置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublishDisposition {
    /// 需要把本地最终版本上传（内容从 `live_path` 的已落盘字节读取）。
    Upload,
    /// 远端内容已经被本地消费/本轮让位于远端：不能再上传本地旧内容。
    Consumed,
    /// 本地版本与远端/依赖冲突尚未解决：本轮不发布，留给下一轮。
    Conflict,
}

/// 一条发布决议：远端路径 + 本地已落盘路径（仅 `Upload` 需要）。
#[derive(Debug, Clone)]
pub(crate) struct PublishDecision {
    pub remote_path: String,
    pub live_path: Option<PathBuf>,
    pub disposition: PublishDisposition,
}

impl PublishDecision {
    pub(crate) fn upload(remote_path: String, live_path: PathBuf) -> Self {
        Self {
            remote_path,
            live_path: Some(live_path),
            disposition: PublishDisposition::Upload,
        }
    }

    pub(crate) fn consume(remote_path: String) -> Self {
        Self {
            remote_path,
            live_path: None,
            disposition: PublishDisposition::Consumed,
        }
    }

    pub(crate) fn conflict(remote_path: String) -> Self {
        Self {
            remote_path,
            live_path: None,
            disposition: PublishDisposition::Conflict,
        }
    }
}

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
