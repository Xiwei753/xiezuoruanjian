use super::transaction_key::VisualTransactionKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DecorationKind {
    Underline,
}

#[derive(Clone, Debug)]
pub(crate) struct DecorationSlice {
    #[cfg_attr(all(), allow(dead_code))]
    pub key: VisualTransactionKey,
    #[cfg_attr(all(), allow(dead_code))]
    pub byte_start: usize,
    #[cfg_attr(all(), allow(dead_code))]
    pub byte_end: usize,
    #[cfg_attr(all(), allow(dead_code))]
    pub kind: DecorationKind,
    #[cfg_attr(all(), allow(dead_code))]
    pub color: String,
    #[cfg_attr(all(), allow(dead_code))]
    pub x: f64,
    #[cfg_attr(all(), allow(dead_code))]
    pub y: f64,
    #[cfg_attr(all(), allow(dead_code))]
    pub w: f64,
    #[cfg_attr(all(), allow(dead_code))]
    pub h: f64,
}

impl DecorationSlice {
    pub fn underline(
        key: VisualTransactionKey,
        byte_start: usize,
        byte_end: usize,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: String,
    ) -> Self {
        Self {
            key,
            byte_start,
            byte_end,
            kind: DecorationKind::Underline,
            color,
            x,
            y,
            w,
            h,
        }
    }
}
