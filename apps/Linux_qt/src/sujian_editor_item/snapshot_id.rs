use std::fmt;

use super::layout_revision::CanonicalLayoutRevision;
use super::transaction_key::VisualTransactionKey;

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

    pub fn from_revision(revision: &CanonicalLayoutRevision, paragraph_id: u64, visual_line_ordinal: u32) -> Self {
        Self {
            layout_revision: revision.layout_revision,
            paragraph_id,
            visual_line_ordinal,
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SliceId {
    pub transaction_key: VisualTransactionKey,
    pub snapshot_id: LineSnapshotId,
    pub document_byte_start: usize,
    pub document_byte_end: usize,
    pub slice_ordinal: u32,
}

impl SliceId {
    pub fn new(
        transaction_key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        document_byte_start: usize,
        document_byte_end: usize,
        slice_ordinal: u32,
    ) -> Self {
        Self {
            transaction_key,
            snapshot_id,
            document_byte_start,
            document_byte_end,
            slice_ordinal,
        }
    }
}

impl fmt::Display for SliceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Slice(tid={},snap={},bytes={}-{},ord={})",
            self.transaction_key.transaction_id,
            self.snapshot_id,
            self.document_byte_start,
            self.document_byte_end,
            self.slice_ordinal
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

    #[test]
    fn test_slice_id_uniqueness() {
        let key = VisualTransactionKey::new(1, 1);
        let snap = LineSnapshotId::new(1, 0, 0);
        let s1 = SliceId::new(key, snap, 0, 3, 0);
        let s2 = SliceId::new(key, snap, 0, 3, 1);
        assert_ne!(s1, s2);
    }
}
