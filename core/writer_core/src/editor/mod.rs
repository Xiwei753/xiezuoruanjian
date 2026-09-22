//! 编辑器模块（Core 层）— 平台无关的编辑器语义。
//!
//! 子模块：
//! - `autocorrect`: 基于 Aho-Corasick 的自动纠错引擎
//! - `kernel`: 正文和业务唯一真相（EditorKernel）及命令/结果类型
//! - `text_edit_session`: 多目标编辑会话注册表
//! - `transaction`: 编辑事务、选区、变更和 offset map
//!
//! 边界：Core 只输出编辑事实（cause / operation_kind / offset_map）；
//! 平台视觉快照、glyph shaping、纹理、动画时间线、RenderNode/QImage 均不属于 Core。

pub mod autocorrect;
pub mod kernel;
pub mod strong_types;
pub mod text_edit_session;
pub mod transaction;

pub use kernel::{
    result::{EditorContentDelta, EditorEditOutcome, EditorEditResult, EditorInputError},
    types::{DisplayPatch, EditorCommand, EditorOperationKind},
    EditorKernel,
};

pub use strong_types::{
    EditorRevision, EditorSessionGeneration, EditorSessionId, Utf8ByteOffset, Utf8ByteRange,
};

pub use text_edit_session::{TextEditSession, TextEditSessionId, TextEditSessionRegistry};

pub use transaction::{
    EditorChange, EditorCursor, EditorSelection, EditorTransaction, EditorTransactionCause,
    OffsetMap, OffsetMapEntry, OffsetMapKind,
};
