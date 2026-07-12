use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LayoutRevision(pub u64);

impl LayoutRevision {
    pub fn next() -> Self {
        LayoutRevision(NEXT_REVISION.fetch_add(1, Ordering::Relaxed))
    }

    pub fn initial() -> Self {
        LayoutRevision(0)
    }
}

impl Default for LayoutRevision {
    fn default() -> Self {
        Self::initial()
    }
}
