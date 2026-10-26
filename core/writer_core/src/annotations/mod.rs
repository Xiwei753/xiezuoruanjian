//! Project-scoped tags, named entries, text anchors, and links.
//!
//! Persisted annotation records are independent JSON objects so additions and
//! tombstones for separate records can converge through the normal file-level
//! sync path. Search indexes and auto-link matches are rebuildable projections.

pub mod matcher;
pub mod model;
pub mod ranges;
pub mod store;

pub use matcher::{AutoLinkMatcher, AutoMatch, AutoMatchSnapshot};
pub use model::{
    AnnotationKind, AnnotationMutation, LinkOrigin, LinkTarget, NamedEntry, ObjectLink, ObjectRef,
    Tag, TagBinding, TextAnchor, DEFAULT_TAG_NAMES,
};
pub use store::{find_backlinks, AnnotationStore, PreparedTextAnchorChanges};
