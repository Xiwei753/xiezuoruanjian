//! 统一对象比较（Issue #1066）。
//!
//! 同步不再按业务类别（正文/星图/校对/标签/统计事件…）分别裁决。每个持久
//! 对象只由三条事实决定：
//!
//! - **共同基线**：该对象上次双方确认的内容摘要（没有基线 = 从未同步过）；
//! - **本地状态**：当前内容摘要 / 显式删除事实 / 本轮不参与；
//! - **远端状态**：固定 HEAD 上的内容摘要 / 显式删除 / 本轮不参与。
//!
//! 裁决只有四种结果：跳过、传播一侧、双向真实冲突、双侧删除确认。
//! 设备时钟不参与胜负判定；"哪一侧更新"只由内容与基线的关系得出。
//!
//! ## 语义
//!
//! - 内容相同（含双侧均无基线且内容相同）→ [`ObjectVerdict::NoOp`]；
//! - 只有一侧相对基线改变 → 传播该侧（本地上传 / 远端落盘 / 删除）；
//! - 双侧都改变且内容不同 → [`ObjectVerdict::Conflict`]，交给用户选择；
//! - 双侧都删除 → [`ObjectVerdict::ConfirmDelete`]（删除是版本化操作，
//!   双方独立删除同一对象不需要用户处理）；
//! - 未参与本轮的对象不是删除：远端没有该对象时本地保持原样，本地没有该
//!   对象时远端基线保持不变。

/// 单个对象在某一侧的状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObjectSide {
    /// 该侧存在该对象，携带内容摘要。
    Present(String),
    /// 该侧有显式删除事实（tombstone / 删除记录），且当前不存在该对象。
    Deleted,
    /// 本轮该侧没有该对象的事实：既不是内容，也不是删除。
    ///
    /// 稀疏下载/增量传输中"没下载到"属于这个状态，不能当成删除传播。
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
    /// 双侧内容相同或都无变化：不传输。
    NoOp,
    /// 只有本地改变：上传本地内容。
    PublishUpsert,
    /// 只有本地删除：把删除发布到远端。
    PublishDelete,
    /// 只有远端改变：把远端内容落到本地。
    ApplyRemoteUpsert,
    /// 只有远端删除：删除本地对象。
    ApplyRemoteDelete,
    /// 双侧都删除：确认删除，不再保留基线。
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
/// `base` 是上次双方确认的内容摘要（`None` = 没有共同基线）。比较只使用内容
/// 摘要与显式删除事实，不使用设备时钟、时间戳或业务类型。
pub(crate) fn compare_object(
    base: Option<&str>,
    local: &ObjectSide,
    remote: &ObjectSide,
) -> ObjectVerdict {
    // 1. 双侧都没有事实：本轮未参与。
    if matches!(local, ObjectSide::Absent) && matches!(remote, ObjectSide::Absent) {
        return ObjectVerdict::Unparticipated;
    }

    let local_present = local.is_present();
    let remote_present = remote.is_present();

    // 2. 双侧内容相同 → 无需传输（含双侧都以同一内容出现但没有基线）。
    if let (Some(local_hash), Some(remote_hash)) = (local.hash(), remote.hash()) {
        if local_hash == remote_hash {
            return ObjectVerdict::NoOp;
        }
    }

    // 3. 双侧都删除 → 确认删除。
    if matches!(local, ObjectSide::Deleted) && matches!(remote, ObjectSide::Deleted) {
        return ObjectVerdict::ConfirmDelete;
    }

    // 4. 双侧都明确存在且内容不同：按基线判断是否各自修改。
    if local_present && remote_present {
        let local_changed = local.hash() != base;
        let remote_changed = remote.hash() != base;
        return match (local_changed, remote_changed) {
            (false, false) => ObjectVerdict::NoOp,
            (true, false) => ObjectVerdict::PublishUpsert,
            (false, true) => ObjectVerdict::ApplyRemoteUpsert,
            (true, true) => ObjectVerdict::Conflict(ObjectConflictSide::BothChanged),
        };
    }

    // 5. 修改 vs 删除：哪一侧还有内容，就判断它是否相对基线改变。
    if local_present && matches!(remote, ObjectSide::Deleted) {
        return if local.hash() == base {
            // 本地没改、远端删除 → 跟删除。
            ObjectVerdict::ApplyRemoteDelete
        } else {
            ObjectVerdict::Conflict(ObjectConflictSide::RemoteDeleted)
        };
    }
    if matches!(local, ObjectSide::Deleted) && remote_present {
        return if remote.hash() == base {
            // 远端没改、本地删除 → 传播删除。
            ObjectVerdict::PublishDelete
        } else {
            ObjectVerdict::Conflict(ObjectConflictSide::LocalDeleted)
        };
    }

    // 6. 只有一侧有事实：另一侧是 Absent（未参与），不是删除。
    match (local, remote) {
        (ObjectSide::Present(local_hash), ObjectSide::Absent) => {
            if base == Some(local_hash.as_str()) {
                // 本地仍是已确认版本，远端只是没参与本轮。
                ObjectVerdict::NoOp
            } else {
                ObjectVerdict::PublishUpsert
            }
        }
        (ObjectSide::Deleted, ObjectSide::Absent) => {
            if base.is_some() {
                // 本地显式删除，远端未参与：删除照常传播。
                ObjectVerdict::PublishDelete
            } else {
                ObjectVerdict::NoOp
            }
        }
        (ObjectSide::Absent, ObjectSide::Present(remote_hash)) => {
            if base == Some(remote_hash.as_str()) {
                ObjectVerdict::NoOp
            } else {
                ObjectVerdict::ApplyRemoteUpsert
            }
        }
        (ObjectSide::Absent, ObjectSide::Deleted) => {
            if base.is_some() {
                ObjectVerdict::ApplyRemoteDelete
            } else {
                ObjectVerdict::NoOp
            }
        }
        _ => ObjectVerdict::NoOp,
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

/// 内容摘要（MD5）作为基线比较用的规范形式。
pub(crate) fn content_hash(content: &[u8]) -> String {
    crate::sync::hash::content_md5(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn present(hash: &str) -> ObjectSide {
        ObjectSide::Present(hash.to_string())
    }

    #[test]
    fn identical_content_is_noop_even_without_base() {
        assert_eq!(
            compare_object(None, &present("a"), &present("a")),
            ObjectVerdict::NoOp
        );
        assert_eq!(
            compare_object(Some("a"), &present("a"), &present("a")),
            ObjectVerdict::NoOp
        );
    }

    #[test]
    fn single_side_change_is_propagated_without_clock() {
        assert_eq!(
            compare_object(Some("a"), &present("b"), &present("a")),
            ObjectVerdict::PublishUpsert
        );
        assert_eq!(
            compare_object(Some("a"), &present("a"), &present("b")),
            ObjectVerdict::ApplyRemoteUpsert
        );
    }

    #[test]
    fn both_changed_differently_is_a_real_conflict() {
        assert_eq!(
            compare_object(Some("a"), &present("b"), &present("c")),
            ObjectVerdict::Conflict(ObjectConflictSide::BothChanged)
        );
    }

    #[test]
    fn both_deleted_is_confirmed_not_conflict() {
        assert_eq!(
            compare_object(Some("a"), &ObjectSide::Deleted, &ObjectSide::Deleted),
            ObjectVerdict::ConfirmDelete
        );
    }

    #[test]
    fn modify_against_delete_conflicts_in_both_directions() {
        assert_eq!(
            compare_object(Some("a"), &present("b"), &ObjectSide::Deleted),
            ObjectVerdict::Conflict(ObjectConflictSide::RemoteDeleted)
        );
        assert_eq!(
            compare_object(Some("a"), &ObjectSide::Deleted, &present("b")),
            ObjectVerdict::Conflict(ObjectConflictSide::LocalDeleted)
        );
    }

    #[test]
    fn unchanged_local_delete_follows_remote_delete() {
        assert_eq!(
            compare_object(Some("a"), &present("a"), &ObjectSide::Deleted),
            ObjectVerdict::ApplyRemoteDelete
        );
        assert_eq!(
            compare_object(Some("a"), &ObjectSide::Deleted, &present("a")),
            ObjectVerdict::PublishDelete
        );
    }

    #[test]
    fn absent_side_is_not_a_delete() {
        // 远端没参与本轮，本地未变 → 跳过，不删除。
        assert_eq!(
            compare_object(Some("a"), &present("a"), &ObjectSide::Absent),
            ObjectVerdict::NoOp
        );
        // 远端没参与，本地改了 → 上传。
        assert_eq!(
            compare_object(Some("a"), &present("b"), &ObjectSide::Absent),
            ObjectVerdict::PublishUpsert
        );
        // 本地没参与（未下载/稀疏），远端未变 → 跳过。
        assert_eq!(
            compare_object(Some("a"), &ObjectSide::Absent, &present("a")),
            ObjectVerdict::NoOp
        );
        // 本地没参与，远端有新内容 → 下载。
        assert_eq!(
            compare_object(Some("a"), &ObjectSide::Absent, &present("b")),
            ObjectVerdict::ApplyRemoteUpsert
        );
    }

    #[test]
    fn fresh_object_one_side_only() {
        assert_eq!(
            compare_object(None, &present("a"), &ObjectSide::Absent),
            ObjectVerdict::PublishUpsert
        );
        assert_eq!(
            compare_object(None, &ObjectSide::Absent, &present("a")),
            ObjectVerdict::ApplyRemoteUpsert
        );
        assert_eq!(
            compare_object(None, &ObjectSide::Absent, &ObjectSide::Absent),
            ObjectVerdict::Unparticipated
        );
    }

    #[test]
    fn local_delete_without_base_is_noop() {
        assert_eq!(
            compare_object(None, &ObjectSide::Deleted, &ObjectSide::Absent),
            ObjectVerdict::NoOp
        );
    }
}
