//! Provider 能力声明 — 让 LWW engine 根据远端能力调整同步策略。
//!
//! 不同 Provider 支持的能力不同：
//! - GitHub：支持条件写入、服务端时间戳、远端历史，但不支持原子批量/移动。
//! - MemoryProvider：支持原子批量、原子移动、目录语义，但无远端历史。
//!
//! engine 通过 `capabilities()` 查询后决定：
//! - `conditional_write` 为真时使用 `IfMatch` 前置条件，否则降级为 `Unconditional`。
//! - `batch` 为真时批量上传，否则逐个上传。
//! - `server_timestamp` 为真时用远端时间戳做 LWW 比较，否则用本地时钟。

/// Provider 能力集合 — 由各 Provider 实现静态返回。
///
/// 所有字段为 `bool`，表示该能力是否可用。能力不可用时 engine 降级处理，
/// 不返回错误（除非该能力是某操作的硬性前提，由 engine 自行决定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncCapabilities {
    /// 是否支持条件写入（If-Match / CreateNew 前置条件）。
    pub conditional_write: bool,
    /// 是否支持原子写入（单次写入多个对象要么全成功要么全失败）。
    pub atomic_write: bool,
    /// 是否支持原子移动（重命名在远端是原子操作）。
    pub atomic_move: bool,
    /// 是否支持批量操作（一次请求处理多个对象）。
    pub batch: bool,
    /// 是否提供可信的服务端时间戳（用于 LWW 比较）。
    pub server_timestamp: bool,
    /// 是否有目录语义（远端区分文件和目录，支持空目录）。
    pub directory_semantics: bool,
    /// 是否支持查询远端历史（版本历史/提交日志）。
    pub remote_history: bool,
}

impl SyncCapabilities {
    /// GitHub REST API 能力。
    ///
    /// GitHub Contents API 支持条件写入（通过 `sha` 参数实现 If-Match），
    /// 提供服务端时间戳（commit author/committer date），有远端历史，
    /// 但不支持原子批量、原子移动（需 delete + create 两步）、目录语义（无空目录）。
    pub fn github() -> Self {
        Self {
            conditional_write: true,
            atomic_write: false,
            atomic_move: false,
            batch: false,
            server_timestamp: true,
            directory_semantics: false,
            remote_history: true,
        }
    }

    /// 内存 Provider 能力。
    ///
    /// MemoryProvider 在单进程内操作，天然支持原子批量、原子移动、目录语义，
    /// 但不提供跨进程可信时间戳（用本地时钟），无远端历史。
    pub fn memory() -> Self {
        Self {
            conditional_write: true,
            atomic_write: true,
            atomic_move: true,
            batch: true,
            server_timestamp: false,
            directory_semantics: true,
            remote_history: false,
        }
    }
}

impl Default for SyncCapabilities {
    fn default() -> Self {
        Self::memory()
    }
}
