use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LineSnapshotId {
    pub layout_revision: u64,
    pub paragraph_id: u64,
    pub visual_line_ordinal: u32,
}

impl LineSnapshotId {
    pub fn new(layout_revision: u64, paragraph_id: u64, visual_line_ordinal: u32) -> Self {
        Self {
            layout_revision,
            paragraph_id,
            visual_line_ordinal,
        }
    }

    /// 修复点 3 (Issue #658 评论 5627327573): 把 LineSnapshotId 映射为稳定的 u64，
    /// 用作 C++ 侧 GPU texture cache (QHash<quint64, QSGTexture*>) 的 key。
    ///
    /// 用确定性 mixing function 组合三个字段（不依赖随机 seed 的 DefaultHasher），
    /// 保证同一进程内同一 LineSnapshotId 永远映射到同一 u64，且不同字段组合尽量分散。
    /// 常数取自 splitmix64 的黄金比例常量，降低碰撞概率。
    pub fn to_cache_key(&self) -> u64 {
        let mut h = self.layout_revision;
        h = h.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        h = h.wrapping_add(self.paragraph_id.wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
        h = h.wrapping_add(u64::from(self.visual_line_ordinal).wrapping_mul(0x1656_67B1_9E37_79F9));
        h ^= h >> 31;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 29;
        h
    }
}

impl fmt::Display for LineSnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "LineSnapshot(rev={},para={},line={})",
            self.layout_revision, self.paragraph_id, self.visual_line_ordinal
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_snapshot_id_stability() {
        let id1 = LineSnapshotId::new(1, 0, 0);
        let id2 = LineSnapshotId::new(1, 0, 0);
        assert_eq!(id1, id2);

        let id3 = LineSnapshotId::new(2, 0, 0);
        assert_ne!(id1, id3);

        let id4 = LineSnapshotId::new(1, 1, 0);
        assert_ne!(id1, id4);
    }
}
