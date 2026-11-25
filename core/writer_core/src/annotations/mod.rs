//! Project-scoped tags, named entries, text anchors, and links.
//!
//! Persisted annotation records are independent JSON objects so additions and
//! tombstones for separate records can converge through the normal file-level
//! sync path. Search indexes and auto-link matches are rebuildable projections.
//!
//! Issue #1060：标签与导航链接的唯一持久化所有者是这里。作品域与全局素材域
//! 各有独立的标签命名空间；旧格式只通过 [`migration`] 读取一次。

pub mod matcher;
pub mod migration;
pub mod model;
pub mod ranges;
pub mod store;

pub use crate::object_ref::ObjectRef;
pub use matcher::{AutoLinkMatcher, AutoMatch, AutoMatchSnapshot};
pub use migration::{
    finish_link_migration, migrate_legacy_links, migrate_starmap_node_tags,
    AnnotationMigrationReport, LegacyNodeTags, LegacyStarMapLink, MigrationConflict,
    CURRENT_ANNOTATION_SCHEMA_VERSION,
};
pub use model::{
    AnnotationKind, AnnotationMutation, LinkOrigin, LinkTarget, NamedEntry, ObjectLink, Tag,
    TagBinding, TextAnchor, DEFAULT_TAG_NAMES,
};
pub use store::{find_backlinks, AnnotationStore, PreparedTextAnchorChanges};
