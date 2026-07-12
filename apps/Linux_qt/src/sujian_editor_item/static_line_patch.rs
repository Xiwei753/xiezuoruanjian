use super::transaction_key::VisualTransactionKey;

#[derive(Clone, Debug)]
pub(crate) struct StaticLinePatch {
    pub key: VisualTransactionKey,
    pub byte_start: usize,
    pub byte_end: usize,
    pub is_insert: bool,
}

impl StaticLinePatch {
    pub fn insert_patch(key: VisualTransactionKey, byte_start: usize, byte_end: usize) -> Self {
        Self {
            key,
            byte_start,
            byte_end,
            is_insert: true,
        }
    }

    pub fn reflow_patch(key: VisualTransactionKey, byte_start: usize, byte_end: usize) -> Self {
        Self {
            key,
            byte_start,
            byte_end,
            is_insert: false,
        }
    }

    pub fn intersects(&self, byte_start: usize, byte_end: usize) -> bool {
        self.byte_end > byte_start && self.byte_start < byte_end
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StaticLinePatchSet {
    pub patches: Vec<StaticLinePatch>,
}

impl StaticLinePatchSet {
    pub fn merged_byte_ranges(&self) -> Vec<(usize, usize)> {
        let mut all: Vec<(usize, usize)> = self
            .patches
            .iter()
            .map(|p| (p.byte_start, p.byte_end))
            .collect();
        all.sort_by_key(|r| r.0);
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (rs, re) in all {
            if let Some(last) = merged.last_mut() {
                if rs <= last.1 {
                    last.1 = last.1.max(re);
                    continue;
                }
            }
            merged.push((rs, re));
        }
        merged
    }

    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
    }

    pub fn remove_for_key(&mut self, key: VisualTransactionKey) {
        self.patches.retain(|p| p.key != key);
    }
}
