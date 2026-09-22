mod composition;
mod types;

pub use types::{
    EditorChange, EditorCursor, EditorSelection, EditorTransaction, EditorTransactionCause,
};

pub(crate) use types::clamp_to_char_boundary;

pub use composition::{OffsetMap, OffsetMapEntry, OffsetMapKind};
