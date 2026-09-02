//! 仓库迁移模块
//!
//! 本模块包含旧格式迁移到新格式的逻辑：
//! - `legacy_migration.rs` - 旧迁移代码
//! - `legacy_migration/` - 旧迁移子模块

pub mod legacy_migration;

pub use legacy_migration::*;
