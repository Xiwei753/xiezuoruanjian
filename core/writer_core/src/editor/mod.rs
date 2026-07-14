//! # 编辑器模块（Core 层）
//!
//! 本模块包含平台无关的编辑器语义，包含自动纠错和统一编辑事务。
//!
//! ## 子模块
//!
//! - `autocorrect`: 自动纠错引擎，基于 Aho-Corasick 算法实现高效文本替换匹配
//! - `transaction`: 编辑事务、选区、变更和动画事件描述
//!
//! ## 功能说明
//!
//! 编辑器模块为写作应用提供文本处理功能，包括：
//! 1. 自动纠错：检测并纠正常见的拼写错误
//! 2. 文本替换：支持批量文本替换操作
//! 3. 编辑事务：统一描述插入、删除、粘贴、加载和格式化造成的正文变化
//! 4. 动画事件：为平台自绘 renderer 提供输入动画事件，不在 Core 内绘制
//!
//! ## 依赖关系
//!
//! - 依赖 `autocorrect` 子模块提供具体的纠错功能
//!
//! ## 使用场景
//!
//! - 写作时的实时拼写检查
//! - 批量文本替换和格式化
//! - 提高写作效率和质量

pub mod autocorrect;
pub mod transaction;

pub use transaction::{
    build_virtual_text, choose_animation_mode, classify_visual_diff, compute_rebase,
    count_grapheme_clusters, diff_plain_text, is_cjk_code_point, is_combining_code_point,
    is_complex_grapheme_code_point, split_text_into_clusters, split_text_into_runs,
    text_contains_complex_grapheme, transactions_overlap, AnimatedSliceRole, AnimationMode,
    CaretAffinity, ClusterRect, ClusterRun, CompositionCommitOrCancelTransaction,
    CompositionSession, CompositionUpdateTransaction, CompositionVisualRevision, CursorPath,
    CursorRect, DecorationSlice, DecorationSliceKind, EditorAnimationKind, EditorChange,
    EditorCursor, EditorEngine, EditorSelection, EditorTransaction, EditorTransactionCause,
    EditorVisualTransaction, GlyphRect, HiddenVisualRange, OffsetMap, OffsetMapEntry,
    OffsetMapKind, PlatformVisualTransaction, PlatformVisualTransactionState,
    PreeditTextFormat, PreeditVisualTransaction, Rect, RebaseFrameSnapshot, ReflowGlyphRect,
    SnapshotOwner, StaticLinePatch, Timeline, TransactionCancelReason, TransactionRebase,
    UnifiedTransactionKind, VisualClassKind, VisualCoordinateMode, VisualLayoutRevision,
    VisualRevision,
};
