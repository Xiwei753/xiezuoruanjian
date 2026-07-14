use super::transaction_key::VisualTransactionKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DecorationKind {
    Underline,
    ThickUnderline,
    BackgroundHighlight,
    ImeCursor,
}

#[derive(Clone, Debug)]
pub(crate) struct DecorationSlice {
    pub key: VisualTransactionKey,
    pub byte_start: usize,
    pub byte_end: usize,
    pub kind: DecorationKind,
    pub color: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl DecorationSlice {
    pub fn underline(key: VisualTransactionKey, byte_start: usize, byte_end: usize, x: f64, y: f64, w: f64, h: f64, color: String) -> Self {
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

    pub fn thick_underline(key: VisualTransactionKey, byte_start: usize, byte_end: usize, x: f64, y: f64, w: f64, h: f64, color: String) -> Self {
        Self {
            key,
            byte_start,
            byte_end,
            kind: DecorationKind::ThickUnderline,
            color,
            x,
            y,
            w,
            h,
        }
    }

    pub fn background_highlight(key: VisualTransactionKey, byte_start: usize, byte_end: usize, x: f64, y: f64, w: f64, h: f64, color: String) -> Self {
        Self {
            key,
            byte_start,
            byte_end,
            kind: DecorationKind::BackgroundHighlight,
            color,
            x,
            y,
            w,
            h,
        }
    }

    pub fn ime_cursor(key: VisualTransactionKey, byte_start: usize, x: f64, y: f64, w: f64, h: f64, color: String) -> Self {
        Self {
            key,
            byte_start,
            byte_end: byte_start,
            kind: DecorationKind::ImeCursor,
            color,
            x,
            y,
            w,
            h,
        }
    }
}
