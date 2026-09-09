use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VisualTransactionKey {
    pub transaction_id: u64,
    pub generation: u64,
}

impl VisualTransactionKey {
    pub fn new(transaction_id: u64, generation: u64) -> Self {
        Self {
            transaction_id,
            generation,
        }
    }
}
