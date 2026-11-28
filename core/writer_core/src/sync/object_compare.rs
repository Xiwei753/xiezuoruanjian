//! 统一对象比较（Issue #1066）。
//!
//! 每个持久对象只由三条版本事实决定：
//!
//! - **共同基线**：上次双方确认的版本（内容摘要 / 已确认删除 / 从未同步）；
//! - **本地版本**：当前内容摘要 / 显式删除 / 本轮不参与；
//! - **远端版本**：固定 HEAD 上的内容摘要 / 显式删除 / 远端没有索引。
//!
//! 节奏只有四种：跳过、传播一侧、双向真实冲突、双侧删除确认。设备时钟、
//! 业务类别和文件类型都不参与胜负判定。
//!
//! 基线、本地、远端使用同一个版本值类型，删除就是"该侧版本为 Deleted"，
//! 不再用 `Option<&str>` 把"没有基线"和"已确认删除"混成一件事。

/// 单个对象在某一侧的版本事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObjectSide {
    /// 该侧存在该对象，携带内容摘要。
    Present(String),
    /// 该侧有显式删除事实（已确认删除 / 删除记录 / tombstone）。
    Deleted,
    /// 该侧本轮没有该对象的事实。
    ///
    /// 远端索引缺失时的"没记录"、稀疏传输中的"没下载"都属于这个状态，
    /// 不能当成删除传播。
    Absent,
}

impl ObjectSide {
    pub(crate) fn hash(&self) -> Option<&str> {
        match self {
            Self::Present(hash) => Some(hash),
            Self::Deleted | Self::Absent => None,
        }
    }

    pub(crate) fn is_present(&self) -> bool {
        matches!(self, Self::Present(_))
    }

    pub(crate) fn is_deleted(&self) -> bool {
        matches!(self, Self::Deleted)
    }
}

/// 真实双端冲突的种类（只描述用户需要看到的事实，不含业务分类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectConflictSide {
    /// 双侧都有不同内容。
    BothChanged,
    /// 远端删除，本地修改。
    RemoteDeleted,
    /// 本地删除，远端修改。
    LocalDeleted,
}

/// 单个对象的同步裁决。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObjectVerdict {
    /// 双侧一致或都无变化：不传输，但要把基线推进到版本事实。
    NoOp,
    /// 只有本地改变：上传本地内容。
    PublishUpsert,
    /// 只有本地删除：把删除发布到远端。
    PublishDelete,
    /// 只有远端改变：把远端内容落到本地。
    ApplyRemoteUpsert,
    /// 只有远端删除：删除本地对象。
    ApplyRemoteDelete,
    /// 双侧都删除：确认删除，不再保留索引记录。
    ConfirmDelete,
    /// 双侧都改变且结果不同：保留双方内容，等用户选择。
    Conflict(ObjectConflictSide),
    /// 两侧都没有该对象的事实：本轮不参与。
    Unparticipated,
}

impl ObjectVerdict {
    pub(crate) fn is_conflict(&self) -> bool {
        matches!(self, Self::Conflict(_))
    }
}

/// 统一对象比较入口。
///
/// `base` 是上次双方确认的版本；删除是一种版本，不是"没有基线"。比较只
/// 使用版本相等关系和显式删除事实，不使用时间戳。
pub(crate) fn compare_object(
    base: &ObjectSide,
    local: &ObjectSide,
    remote: &ObjectSide,
) -> ObjectVerdict {
    // 1. 双侧都没有事实：本轮未参与。
    if matches!(local, ObjectSide::Absent) && matches!(remote, ObjectSide::Absent) {
        return ObjectVerdict::Unparticipated;
    }

    // 2. 双侧版本相同：内容相同 → 无需传输；都删除 → 确认删除。
    if local == remote {
        return match local {
            ObjectSide::Present(_) => ObjectVerdict::NoOp,
            ObjectSide::Deleted => ObjectVerdict::ConfirmDelete,
            ObjectSide::Absent => ObjectVerdict::Unparticipated,
        };
    }

    let local_changed = local != base;
    let remote_changed = remote != base;

    match (local_changed, remote_changed) {
        (false, false) => ObjectVerdict::NoOp,
        // 3. 只有本地相对基线改变：传播本地版本。
        (true, false) => match local {
            ObjectSide::Present(_) => ObjectVerdict::PublishUpsert,
            ObjectSide::Deleted => ObjectVerdict::PublishDelete,
            // 本地没有事实而远端停在基线：没有可传播的内容。
            ObjectSide::Absent => ObjectVerdict::NoOp,
        },
        // 4. 只有远端相对基线改变：应用远端版本。
        (false, true) => match remote {
            ObjectSide::Present(_) => ObjectVerdict::ApplyRemoteUpsert,
            ObjectSide::Deleted => ObjectVerdict::ApplyRemoteDelete,
            // 远端没有事实而本地停在基线：远端索引缺失，不是删除。
            ObjectSide::Absent => ObjectVerdict::NoOp,
        },
        // 5. 双侧都相对基线改变且版本不同：真实冲突。
        (true, true) => match (local, remote) {
            (ObjectSide::Present(_), ObjectSide::Present(_)) => {
                ObjectVerdict::Conflict(ObjectConflictSide::BothChanged)
            }
            (ObjectSide::Present(_), ObjectSide::Deleted) => {
                ObjectVerdict::Conflict(ObjectConflictSide::RemoteDeleted)
            }
            (ObjectSide::Deleted, ObjectSide::Present(_)) => {
                ObjectVerdict::Conflict(ObjectConflictSide::LocalDeleted)
            }
            // 远端没有索引（Absent）时本地修改仍要上传：远端没有可比较的
            // 版本事实，不能把"索引缺失"当成删除或冲突。
            (ObjectSide::Present(_), ObjectSide::Absent) => ObjectVerdict::PublishUpsert,
            // 一侧没有事实，另一侧改变：缺少该侧版本，不能替用户裁决。
            _ => ObjectVerdict::Unparticipated,
        },
    }
}

/// 从 `Option<&[u8]>` 构造对象状态：`Some` = 存在内容，`None` = 没有该侧内容
/// （调用方按是否存在显式删除事实选择 [`ObjectSide::Deleted`] 或
/// [`ObjectSide::Absent`]）。
pub(crate) fn side_from_content(content: Option<&[u8]>) -> ObjectSide {
    match content {
        Some(bytes) => ObjectSide::Present(crate::sync::hash::content_md5(bytes)),
        None => ObjectSide::Absent,
    }
}

/// 内容摘要（MD5）作为版本比较用的规范形式。
pub(crate) fn content_hash(content: &[u8]) -> String {
    crate::sync::hash::content_md5(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn present(hash: &str) -> ObjectSide {
        ObjectSide::Present(hash.to_string())
    }

    const ABSENT: ObjectSide = ObjectSide::Absent;
    const DELETED: ObjectSide = ObjectSide::Deleted;

    #[test]
    fn identical_content_is_noop_even_without_base() {
        assert_eq!(
            compare_object(&ABSENT, &present("a"), &present("a")),
            ObjectVerdict::NoOp
        );
        assert_eq!(
            compare_object(&present("a"), &present("a"), &present("a")),
            ObjectVerdict::NoOp
        );
    }

    #[test]
    fn single_side_change_is_propagated_without_clock() {
        assert_eq!(
            compare_object(&present("a"), &present("b"), &present("a")),
            ObjectVerdict::PublishUpsert
        );
        assert_eq!(
            compare_object(&present("a"), &present("a"), &present("b")),
            ObjectVerdict::ApplyRemoteUpsert
        );
    }

    #[test]
    fn both_changed_differently_is_a_real_conflict() {
        assert_eq!(
            compare_object(&present("a"), &present("b"), &present("c")),
            ObjectVerdict::Conflict(ObjectConflictSide::BothChanged)
        );
    }

    #[test]
    fn delete_versions_participate_in_comparison() {
        // 远端停在基线、本地删除 → 传播删除。
        assert_eq!(
            compare_object(&present("a"), &DELETED, &present("a")),
            ObjectVerdict::PublishDelete
        );
        // 本地停在基线、远端删除 → 本地删除。
        assert_eq!(
            compare_object(&present("a"), &present("a"), &DELETED),
            ObjectVerdict::ApplyRemoteDelete
        );
        // 双方都删除 → 确认删除。
        assert_eq!(
            compare_object(&present("a"), &DELETED, &DELETED),
            ObjectVerdict::ConfirmDelete
        );
        // 删除已是基线：本地重新写入且远端仍删除 → 本地复活，不是冲突。
        assert_eq!(
            compare_object(&DELETED, &present("b"), &DELETED),
            ObjectVerdict::PublishUpsert
        );
        // 删除已是基线：远端重新写入且本地仍删除 → 采用远端版本。
        assert_eq!(
            compare_object(&DELETED, &DELETED, &present("b")),
            ObjectVerdict::ApplyRemoteUpsert
        );
    }

    #[test]
    fn delete_without_base_conflicts_once_until_user_chooses() {
        assert_eq!(
            compare_object(&ABSENT, &present("a"), &DELETED),
            ObjectVerdict::Conflict(ObjectConflictSide::RemoteDeleted)
        );
        // 用户保留本地后基线变成 Deleted，下一轮不再冲突，改为上传本地版本。
        assert_eq!(
            compare_object(&DELETED, &present("a"), &DELETED),
            ObjectVerdict::PublishUpsert
        );
        // 用户保留本地删除后基线是远端版本，下一轮直接传播删除。
        assert_eq!(
            compare_object(&present("a"), &DELETED, &present("a")),
            ObjectVerdict::PublishDelete
        );
    }

    #[test]
    fn absent_side_is_unparticipated_not_delete() {
        assert_eq!(
            compare_object(&present("a"), &present("b"), &ABSENT),
            ObjectVerdict::PublishUpsert
        );
        assert_eq!(
            compare_object(&present("a"), &present("a"), &ABSENT),
            ObjectVerdict::NoOp
        );
        // 远端没有索引（Absent）而本地删除了已确认对象：没有远端事实，不传播。
        assert_eq!(
            compare_object(&present("a"), &DELETED, &ABSENT),
            ObjectVerdict::Unparticipated
        );
        assert_eq!(
            compare_object(&present("a"), &ABSENT, &present("a")),
            ObjectVerdict::NoOp
        );
    }
}
