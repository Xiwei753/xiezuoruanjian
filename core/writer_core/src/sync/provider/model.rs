//! Provider-neutral 远端模型 — 所有 SyncProvider 实现共用的远端对象表示。
//!
//! 本模块定义远端对象的纯数据结构，不携带任何 GitHub/Git 特定语义。
//! - [`RemoteEntry`]：远端对象条目（路径 + 版本），用于 `list` 枚举。
//! - [`RemoteObject`]：远端对象内容（路径 + 字节 + 版本），用于 `read`。
//! - [`RemoteVersion`]：远端版本标识 newtype，GitHub 为 blob SHA，MemoryProvider 可为 UUID。
//! - [`WritePrecondition`] / [`DeletePrecondition`]：写入/删除前置条件，
//!   让调用方表达乐观并发控制（If-Match / CreateNew / Unconditional）。

use serde::{Deserialize, Serialize};

/// 远端版本标识 — provider 无关的 opaque 字符串。
///
/// - GitHub 实现下为 Git blob SHA（40 位十六进制）。
/// - `MemoryProvider` 实现下可为 UUID 或自增字符串。
///
/// 用 String newtype 包装，实现序列化以支持诊断/日志持久化。
/// 不在此处假设任何格式约束（长度、字符集），具体约束由 Provider 实现负责。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteVersion(pub String);

impl RemoteVersion {
    /// 创建一个远端版本标识。
    pub fn new(version: impl Into<String>) -> Self {
        Self(version.into())
    }

    /// 取底层字符串引用。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RemoteVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 远端对象条目 — `list` 枚举返回的轻量记录，仅含路径与版本，不含内容。
///
/// `path` 为相对远端根的完整路径（含前缀），由 Provider 实现决定是否剥前缀。
/// 调用方（LWW engine）按自身前缀语义处理。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteEntry {
    pub path: String,
    pub version: RemoteVersion,
}

/// 远端对象内容 — `read` 返回的完整对象，包含字节内容与版本。
///
/// `content` 为原始字节（GitHub 实现下为文件解码后的 UTF-8 字节）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteObject {
    pub path: String,
    pub content: Vec<u8>,
    pub version: RemoteVersion,
}

/// 写入前置条件 — 乐观并发控制，对应 HTTP `If-Match` 语义。
///
/// - [`WritePrecondition::IfMatch`]：要求远端当前版本与给定版本一致才写入，
///   否则返回 `ProviderError::PreconditionFailed`。用于覆盖已存在对象。
/// - [`WritePrecondition::CreateNew`]：要求对象不存在才写入，
///   否则返回 `ProviderError::PreconditionFailed`。用于创建新对象。
/// - [`WritePrecondition::Unconditional`]：无前置条件，直接覆盖。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WritePrecondition {
    IfMatch(RemoteVersion),
    CreateNew,
    Unconditional,
}

/// 删除前置条件 — 乐观并发控制，对应 HTTP `If-Match` 语义。
///
/// - [`DeletePrecondition::IfMatch`]：要求远端当前版本与给定版本一致才删除，
///   否则返回 `ProviderError::PreconditionFailed`。
/// - [`DeletePrecondition::Unconditional`]：无前置条件，直接删除（若存在）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletePrecondition {
    IfMatch(RemoteVersion),
    Unconditional,
}
