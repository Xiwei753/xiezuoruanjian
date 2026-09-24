pub(crate) mod composition;
pub(crate) mod coordinator;
pub(crate) mod cursor_motion;
pub(crate) mod rebase;
pub(crate) mod render_plan_builder;
pub(crate) mod transaction;
pub(crate) mod transaction_builder;

pub(crate) use coordinator::{find_line_geometry_in_snapshot, LinuxEditorAnimationCoordinator};
pub(crate) use transaction::{
    PreparedCursorVisualTrack, PreparedTextVisualTransaction, PreparedTransactionQueue,
    PreparedVisualUnit, RebaseFrame, TextVisualOperationKind, TextVisualTransactionState,
    TransactionTimeline, VisualUnitTiming,
};
