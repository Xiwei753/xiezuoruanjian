mod composition;
mod engine;
mod platform;
mod rebase;
#[cfg(test)]
mod tests;
mod types;
mod visual;

pub use types::{
    AnimationMode, EditorAnimationKind, EditorChange, EditorCursor, EditorSelection,
    EditorTransaction, EditorTransactionCause,
};

pub(crate) use types::clamp_to_char_boundary;

pub use visual::{
    build_virtual_text, AnimatedSliceRole, CaretAffinity, ClusterRect, ClusterRun, CursorPath,
    CursorRect, DecorationSlice, DecorationSliceKind, EditorVisualTransaction, GlyphRect,
    HiddenVisualRange, PlatformVisualTransactionState, PreeditTextFormat, PreeditVisualTransaction,
    Rect, ReflowGlyphRect, StaticLinePatch, Timeline, UnifiedTransactionKind, VisualClassKind,
    VisualCoordinateMode, VisualLayoutRevision, VisualRevision,
};

pub use composition::{
    CompositionSession, CompositionVisualRevision, OffsetMap, OffsetMapEntry, OffsetMapKind,
};

pub use rebase::{
    RebaseContinuation, RebaseFrameSnapshot, RebaseReason, RebaseSliceMapping, SnapshotOwner,
    TransactionCancelReason, TransactionRebase,
};

pub use platform::PlatformVisualTransaction;

pub use engine::{
    choose_animation_mode, classify_composition_visual, classify_visual_diff, compute_rebase,
    compute_rebase_slice_mappings, count_grapheme_clusters, diff_plain_text, is_cjk_code_point,
    is_combining_code_point, is_complex_grapheme_code_point, split_text_into_clusters,
    split_text_into_runs, text_contains_complex_grapheme, transactions_overlap,
    CompositionOperationKind, CompositionVisualClassification, EditorEngine, SliceMatchInput,
};
