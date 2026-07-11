use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VisualTransactionKey {
    pub transaction_id: u64,
    pub generation: u64,
}

impl VisualTransactionKey {
    pub fn new(transaction_id: u64, generation: u64) -> Self {
        Self { transaction_id, generation }
    }

    pub fn zero() -> Self {
        Self { transaction_id: 0, generation: 0 }
    }

    pub fn is_valid(&self) -> bool {
        self.transaction_id > 0
    }
}
