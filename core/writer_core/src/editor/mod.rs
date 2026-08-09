//! 编辑器模块（Core 层）— 平台无关的编辑器语义。
//!
//! 子模块：
//! - `autocorrect`: 基于 Aho-Corasick 的自动纠错引擎
//! - `kernel`: 正文和业务唯一真相（EditorKernel）及命令/结果类型
//! - `text_edit_session`: 多目标编辑会话注册表
//! - `transaction`: 编辑事务、选区、变更、动画语义和视觉契约
//!
//! 边界：Core 只输出编辑语义、byte range、cause、animation mode 和 cursor 语义；
//! 平台视觉快照、glyph shaping、纹理、RenderNode/QImage 均不属于 Core。

pub mod autocorrect;
pub mod kernel;
pub mod strong_types;
pub mod text_edit_session;
pub mod transaction;

pub use kernel::{
    result::{EditorEditOutcome, EditorEditResult, EditorInputError},
    types::{
        CoordinatedCursor, DisplayPatch, EditorCommand, EditorOperationKind, EditorVisualIntent,
    },
    EditorKernel,
};

pub use strong_types::{
    EditorRevision, EditorSessionGeneration, EditorSessionId, Utf8ByteOffset, Utf8ByteRange,
};

pub use text_edit_session::{TextEditSession, TextEditSessionId, TextEditSessionRegistry};

pub use transaction::{
    build_virtual_text, choose_animation_mode, classify_composition_visual, classify_visual_diff,
    compute_rebase, count_grapheme_clusters, diff_plain_text, is_cjk_code_point,
    is_combining_code_point, is_complex_grapheme_code_point, split_text_into_clusters,
    split_text_into_runs, text_contains_complex_grapheme, transactions_overlap, AnimatedSliceRole,
    AnimationMode, CaretAffinity, ClusterRect, ClusterRun, CompositionCommitOrCancelTransaction,
    CompositionOperationKind, CompositionSession, CompositionUpdateTransaction,
    CompositionVisualClassification, CompositionVisualRevision, CursorPath, CursorRect,
    DecorationSlice, DecorationSliceKind, EditorAnimationKind, EditorChange, EditorCursor,
    EditorEngine, EditorSelection, EditorTransaction, EditorTransactionCause,
    EditorVisualTransaction, GlyphRect, HiddenVisualRange, OffsetMap, OffsetMapEntry,
    OffsetMapKind, PlatformVisualTransaction, PlatformVisualTransactionState, PreeditTextFormat,
    PreeditVisualTransaction, RebaseFrameSnapshot, Rect, ReflowGlyphRect, SnapshotOwner,
    StaticLinePatch, Timeline, TransactionCancelReason, TransactionRebase, UnifiedTransactionKind,
    VisualClassKind, VisualCoordinateMode, VisualLayoutRevision, VisualRevision,
};
