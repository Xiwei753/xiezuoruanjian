//! 版本化对象索引与同步文件原语（Issue #1066 保留的清单存储层）。
//!
//! 同步主链不再有 LWW 子引擎：对象比较在
//! [`crate::sync::object_compare`]，裁决在
//! [`crate::sync::staging::plan`]，提交在
//! [`crate::sync::commit_helpers::commit_object_plan`]。本模块只保留：
//!
//! - [`manifest`]：本地/远端版本化清单存储（root + 两级索引 + 内容寻址对象），
//!   以及一次性读取旧单文件/旧分片格式的迁移入口；
//! - [`transfer`]：冲突副本与 trash 两个文件原语。
//!
//! 远端读写一律通过 [`crate::sync::provider::SyncProvider`]，不绑定具体传输。

mod manifest;
mod transfer;

// re-export move_to_trash，供 conflict.rs 的 resolve_conflict_take_remote
// （RemoteDeleted 语义）复用同一 trash 移动逻辑。
pub(crate) use transfer::move_to_trash;

// re-export save_conflict_copy，供 commit_helpers 在 staging cleanup 前保存远端快照。
pub(crate) use transfer::save_conflict_copy;

// 只读 local record 投影 helper：plan/dry-run 与提交阶段共用。
#[allow(unused_imports)]
pub(crate) use manifest::{snapshot_app_group_records_read_only, snapshot_local_records_read_only};

pub(crate) use manifest::SYNC_MANIFEST_PATH;
#[allow(unused_imports)]
pub(crate) use manifest::SYNC_MANIFEST_SHARDS_DIR;

// 版本化清单存储入口：读取统一 root 优先/旧格式迁移，不再各读各的单文件。
pub(crate) use manifest::{
    fetch_remote_manifest_at, is_manifest_rel_path, is_manifest_storage_path, is_sha256_hex,
    namespace_for_path, object_key, publish_remote_manifest, remote_rel_for, sha256_hex,
    ManifestIndex, ManifestObject, ManifestRoot, ManifestStore, ManifestWritePlan,
    MANIFEST_SCHEMA_VERSION,
};
