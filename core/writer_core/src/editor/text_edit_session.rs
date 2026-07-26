//! TextEditSessionRegistry — 多目标文字编辑会话注册表。
//!
//! 每个可编辑文字目标（项目名、章节名、星图节点标题、正文等）
//! 通过独立会话接入 Editor V2 主链。不同目标之间不共享 revision、
//! composition token 或 Undo 栈。
//!
//! 会话分为两种：
//! - 持久内容会话：章节正文等持续编辑内容，拥有独立长期会话。
//! - 临时草稿会话：项目名、搜索词等短文本，编辑时创建，确认/取消后关闭。

use super::EditorKernel;
use super::kernel::result::EditorInputError;

/// 文字编辑会话 ID — 全局唯一，标识一次独立的文字事务环境。
/// 内部类型 u64 对应平台端 Kotlin ULong，通过 FFI 边界传递时保持无符号语义。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextEditSessionId(pub u64);

impl TextEditSessionId {
    /// 提取内部 u64 值，用于 FFI 边界传递。
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// 文字编辑会话 — 独立持有 EditorKernel、revision、选区、Undo/Redo 和 composition 状态。
///
/// [generation] 在每次 [reset_session] 时单调递增，用于使过期的 composition session
/// 失效——平台端持有的 composition generation 必须与会话当前 generation 匹配，
/// 否则 updateComposition/finishComposition/cancelComposition 会被内核拒绝。
/// 这防止了异步 composition 操作写入已被 reset 的会话。
///
/// [generation] 字段为 u64，对应平台端 Kotlin Long（通过 toULong() 恢复无符号语义）。
pub struct TextEditSession {
    pub kernel: EditorKernel,
    pub session_id: TextEditSessionId,
    pub target_id: String,
    pub generation: u64,
    pub is_persistent: bool,
}

impl TextEditSession {
    /// 创建空文本会话。generation 初始为 0，is_persistent 标识是否为长期会话。
    pub fn new(session_id: TextEditSessionId, target_id: String, is_persistent: bool) -> Self {
        Self {
            kernel: EditorKernel::new(),
            session_id,
            target_id,
            generation: 0,
            is_persistent,
        }
    }

    /// 用初始文本和光标创建会话。cursor 必须为合法 UTF-8 char boundary。
    pub fn with_text(
        session_id: TextEditSessionId,
        target_id: String,
        is_persistent: bool,
        text: String,
        cursor: usize,
    ) -> Result<Self, EditorInputError> {
        let kernel = EditorKernel::with_text(text, cursor)?;
        Ok(Self {
            kernel,
            session_id,
            target_id,
            generation: 0,
            is_persistent,
        })
    }
}

/// 文字编辑会话注册表 — 管理所有活跃编辑会话。
///
/// 会话 ID 从 1 开始单调递增（saturating_add），关闭的 ID 不回收。
/// 注册表本身不持有锁——调用方需保证 `&mut self` 操作的线程安全。
pub struct TextEditSessionRegistry {
    sessions: std::collections::HashMap<u64, TextEditSession>,
    next_session_id: u64,
}

impl TextEditSessionRegistry {
    pub fn new() -> Self {
        Self {
            sessions: std::collections::HashMap::new(),
            next_session_id: 1,
        }
    }

    /// 创建新会话并返回其 ID。空文本使用 `new()`，非空使用 `with_text()`。
    /// `initial_cursor` 仅在 `initial_text` 非空时有效。
    pub fn create_session(
        &mut self,
        target_id: String,
        initial_text: String,
        initial_cursor: usize,
        is_persistent: bool,
    ) -> Result<TextEditSessionId, EditorInputError> {
        let id = self.next_session_id;
        self.next_session_id = self.next_session_id.saturating_add(1);
        let session = if initial_text.is_empty() {
            TextEditSession::new(TextEditSessionId(id), target_id, is_persistent)
        } else {
            TextEditSession::with_text(
                TextEditSessionId(id),
                target_id,
                is_persistent,
                initial_text,
                initial_cursor,
            )?
        };
        self.sessions.insert(id, session);
        Ok(TextEditSessionId(id))
    }

    /// 获取会话的不可变引用。不存在时返回 None。
    pub fn get_session(&self, session_id: TextEditSessionId) -> Option<&TextEditSession> {
        self.sessions.get(&session_id.0)
    }

    /// 获取会话的可变引用。不存在时返回 None。
    pub fn get_session_mut(&mut self, session_id: TextEditSessionId) -> Option<&mut TextEditSession> {
        self.sessions.get_mut(&session_id.0)
    }

    /// 关闭会话并移除。返回 true 表示存在并已移除，false 表示不存在。
    pub fn close_session(&mut self, session_id: TextEditSessionId) -> bool {
        self.sessions.remove(&session_id.0).is_some()
    }

    /// Reset a session's text and cursor, incrementing generation first.
    ///
    /// Note: the FFI entry point (`app_service::text_edit_session_reset`) performs its own
    /// generation increment via `with_session_in_registry` and does NOT call this method.
    /// This method exists for direct registry usage (e.g. tests). Both paths follow the
    /// same invariant: generation is incremented before load_text so in-flight composition
    /// operations with the old generation are rejected.
    pub fn reset_session(
        &mut self,
        session_id: TextEditSessionId,
        text: String,
        cursor: usize,
    ) -> Result<(), EditorInputError> {
        if let Some(session) = self.sessions.get_mut(&session_id.0) {
            // Generation is incremented BEFORE load_text so that any in-flight
            // composition operations with the old generation will be rejected
            // by the kernel — preventing stale composition writes to a reset session.
            session.generation = session.generation.saturating_add(1);
            let _ = session.kernel.load_text(text, cursor);
            Ok(())
        } else {
            Err(EditorInputError::InvalidCursorOffset {
                offset: 0,
                text_len: 0,
            })
        }
    }

    /// 检查会话是否存在。
    pub fn session_exists(&self, session_id: TextEditSessionId) -> bool {
        self.sessions.contains_key(&session_id.0)
    }

    /// 当前活跃会话数量。
    pub fn active_session_count(&self) -> usize {
        self.sessions.len()
    }
}

impl Default for TextEditSessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_close_session() {
        let mut registry = TextEditSessionRegistry::new();
        let id = registry
            .create_session("project-title:1".to_string(), "Hello".to_string(), 5, false)
            .unwrap();
        assert!(registry.session_exists(TextEditSessionId(id.0)));
        assert_eq!(registry.active_session_count(), 1);

        let session = registry.get_session(TextEditSessionId(id.0)).unwrap();
        assert_eq!(session.kernel.text(), "Hello");
        assert_eq!(session.target_id, "project-title:1");
        assert!(!session.is_persistent);

        assert!(registry.close_session(TextEditSessionId(id.0)));
        assert!(!registry.session_exists(TextEditSessionId(id.0)));
        assert_eq!(registry.active_session_count(), 0);
    }

    #[test]
    fn create_persistent_session() {
        let mut registry = TextEditSessionRegistry::new();
        let id = registry
            .create_session("chapter-body:1:1:1".to_string(), "正文".to_string(), 6, true)
            .unwrap();
        let session = registry.get_session(TextEditSessionId(id.0)).unwrap();
        assert!(session.is_persistent);
    }

    #[test]
    fn create_empty_session() {
        let mut registry = TextEditSessionRegistry::new();
        let id = registry
            .create_session("search:1".to_string(), "".to_string(), 0, false)
            .unwrap();
        let session = registry.get_session(TextEditSessionId(id.0)).unwrap();
        assert_eq!(session.kernel.text(), "");
    }

    #[test]
    fn reset_session() {
        let mut registry = TextEditSessionRegistry::new();
        let id = registry
            .create_session("volume-title:1".to_string(), "Old".to_string(), 3, false)
            .unwrap();
        registry
            .reset_session(TextEditSessionId(id.0), "New".to_string(), 3)
            .unwrap();
        let session = registry.get_session(TextEditSessionId(id.0)).unwrap();
        assert_eq!(session.kernel.text(), "New");
        assert_eq!(session.generation, 1);
    }

    #[test]
    fn close_nonexistent_session() {
        let mut registry = TextEditSessionRegistry::new();
        assert!(!registry.close_session(TextEditSessionId(999)));
    }

    #[test]
    fn multiple_sessions_independent() {
        let mut registry = TextEditSessionRegistry::new();
        let id1 = registry
            .create_session("project-title:1".to_string(), "Alpha".to_string(), 5, false)
            .unwrap();
        let id2 = registry
            .create_session("chapter-title:1".to_string(), "Beta".to_string(), 4, false)
            .unwrap();
        assert_ne!(id1.0, id2.0);

        let s1 = registry.get_session(TextEditSessionId(id1.0)).unwrap();
        let s2 = registry.get_session(TextEditSessionId(id2.0)).unwrap();
        assert_eq!(s1.kernel.text(), "Alpha");
        assert_eq!(s2.kernel.text(), "Beta");
    }
}
