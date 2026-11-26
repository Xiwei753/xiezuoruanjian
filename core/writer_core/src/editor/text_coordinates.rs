//! 文本坐标的唯一换算与校验入口（Issue #1063）。
//!
//! Core 内部用 UTF-8 byte offset，Qt 的 QTextDocument/IME 用 UTF-16 code unit，
//! ArkTS string 也是 UTF-16。此前各端各写一份 `clamp_to_char_boundary` /
//! `utf16 ↔ utf8` 算法，非法位置有的 panic、有的返回 -1、有的静默左对齐，
//! 结果同一次点击在不同端落到不同位置。
//!
//! 本模块把规则收成一套，并区分两类语义：
//!
//! - **严格校验**：[`utf8_to_utf16`]、[`utf16_to_utf8`]、[`validate_utf8_range`]
//!   遇到非法输入返回可区分的 [`TextOffsetError`]，调用方决定失败还是对齐。
//! - **有说明的方向性钳位**：[`clamp_to_char_boundary`]、[`utf16_to_utf8_clamped`]
//!   与 [`utf8_to_utf16_clamped`]。越界钳到文本末尾，落在多字节字符中间时
//!   回退到该字符起始；UTF-16 落在代理对中间时映射到该码点起始（Qt/QChar 的
//!   既有约定：低代理项与高代理项同属一个字符位置）。
//!
//! 这里不做任何 I/O、不依赖平台类型；`&str` 适配面向平台层热路径，
//! [`crop::Rope`] 适配面向 Core 内部（不 materialize 全文）。

use crop::Rope;

/// 文本坐标非法/越界的可区分错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOffsetError {
    /// UTF-8 byte offset 超过正文 byte 长度。
    Utf8OutOfBounds { offset: usize, text_len: usize },
    /// UTF-8 byte offset 落在某个字符的中间。
    NotCharBoundary { offset: usize },
    /// UTF-16 code unit offset 超过正文 UTF-16 长度。
    Utf16OutOfBounds {
        offset: usize,
        text_utf16_len: usize,
    },
    /// UTF-16 code unit offset 落在代理对中间。
    NotUtf16Boundary { offset: usize },
    /// 半开区间 start > end。
    InvertedRange { start: usize, end: usize },
}

impl std::fmt::Display for TextOffsetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Utf8OutOfBounds { offset, text_len } => write!(
                f,
                "UTF-8 byte offset {offset} 超出正文 byte 长度 {text_len}"
            ),
            Self::NotCharBoundary { offset } => {
                write!(f, "UTF-8 byte offset {offset} 不在字符边界")
            }
            Self::Utf16OutOfBounds {
                offset,
                text_utf16_len,
            } => write!(
                f,
                "UTF-16 code unit offset {offset} 超出正文 UTF-16 长度 {text_utf16_len}"
            ),
            Self::NotUtf16Boundary { offset } => {
                write!(f, "UTF-16 code unit offset {offset} 落在代理对中间")
            }
            Self::InvertedRange { start, end } => {
                write!(f, "半开区间 start={start} 大于 end={end}")
            }
        }
    }
}

impl std::error::Error for TextOffsetError {}

/// 一段文本的 UTF-16 code unit 长度。
pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// 把 UTF-8 byte offset 钳到合法的 char boundary。
///
/// 越界返回 `text.len()`；落在多字节字符中间时向左回退到该字符起始。
/// 这是有说明的方向性钳位（不是错误吞掉），只用于外部输入对齐；
/// 需要失败语义时用 [`validate_utf8_range`] / [`utf8_to_utf16`]。
pub fn clamp_to_char_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    if text.is_char_boundary(offset) {
        return offset;
    }
    let mut clamped = offset;
    while clamped > 0 && !text.is_char_boundary(clamped) {
        clamped -= 1;
    }
    clamped
}

/// Rope 版 char boundary 钳位（不 materialize 全文，O(log n) 定位）。
pub fn rope_clamp_to_char_boundary(rope: &Rope, offset: usize) -> usize {
    if offset >= rope.byte_len() {
        return rope.byte_len();
    }
    if rope.is_char_boundary(offset) {
        return offset;
    }
    let mut clamped = offset;
    while clamped > 0 && !rope.is_char_boundary(clamped) {
        clamped -= 1;
    }
    clamped
}

/// 严格校验半开 byte range：`start <= end`、均在正文内、均落在字符边界。
pub fn validate_utf8_range(text: &str, start: usize, end: usize) -> Result<(), TextOffsetError> {
    validate_utf8_range_with_len(text.len(), start, end, |offset| {
        offset == text.len() || text.is_char_boundary(offset)
    })
}

/// Rope 版严格校验半开 byte range（不 materialize 全文）。
pub fn validate_utf8_range_rope(
    rope: &Rope,
    start: usize,
    end: usize,
) -> Result<(), TextOffsetError> {
    validate_utf8_range_with_len(rope.byte_len(), start, end, |offset| {
        offset == rope.byte_len() || rope.is_char_boundary(offset)
    })
}

fn validate_utf8_range_with_len(
    text_len: usize,
    start: usize,
    end: usize,
    is_char_boundary: impl Fn(usize) -> bool,
) -> Result<(), TextOffsetError> {
    if start > end {
        return Err(TextOffsetError::InvertedRange { start, end });
    }
    if start > text_len {
        return Err(TextOffsetError::Utf8OutOfBounds {
            offset: start,
            text_len,
        });
    }
    if end > text_len {
        return Err(TextOffsetError::Utf8OutOfBounds {
            offset: end,
            text_len,
        });
    }
    if !is_char_boundary(start) {
        return Err(TextOffsetError::NotCharBoundary { offset: start });
    }
    if !is_char_boundary(end) {
        return Err(TextOffsetError::NotCharBoundary { offset: end });
    }
    Ok(())
}

/// UTF-8 byte offset → UTF-16 code unit offset（严格版）。
///
/// `byte_offset` 必须落在字符边界或等于文本 byte 长度；否则返回错误。
pub fn utf8_to_utf16(text: &str, byte_offset: usize) -> Result<usize, TextOffsetError> {
    if byte_offset > text.len() {
        return Err(TextOffsetError::Utf8OutOfBounds {
            offset: byte_offset,
            text_len: text.len(),
        });
    }
    if !text.is_char_boundary(byte_offset) {
        return Err(TextOffsetError::NotCharBoundary {
            offset: byte_offset,
        });
    }
    Ok(text[..byte_offset].encode_utf16().count())
}

/// UTF-8 byte offset → UTF-16 code unit offset（方向性钳位版）。
///
/// 越界钳到文本末尾；落在字符中间时先按 [`clamp_to_char_boundary`] 左对齐，
/// 再换算。调用方要区分失败时用 [`utf8_to_utf16`]。
pub fn utf8_to_utf16_clamped(text: &str, byte_offset: usize) -> usize {
    let aligned = clamp_to_char_boundary(text, byte_offset);
    text[..aligned].encode_utf16().count()
}

/// UTF-16 code unit offset → UTF-8 byte offset（严格版）。
///
/// `utf16_offset` 必须落在码点起始位置或等于文本 UTF-16 长度；
/// 落在代理对中间返回 [`TextOffsetError::NotUtf16Boundary`]。
pub fn utf16_to_utf8(text: &str, utf16_offset: usize) -> Result<usize, TextOffsetError> {
    let mut utf16_seen = 0usize;
    for (byte_pos, ch) in text.char_indices() {
        if utf16_seen == utf16_offset {
            return Ok(byte_pos);
        }
        let ch_utf16_len = ch.len_utf16();
        if utf16_seen + ch_utf16_len > utf16_offset {
            // 偏移落在当前码点内部：只有 len_utf16() == 2 的代理对会走到这里。
            return Err(TextOffsetError::NotUtf16Boundary {
                offset: utf16_offset,
            });
        }
        utf16_seen += ch_utf16_len;
    }
    let text_utf16_len = utf16_seen;
    if utf16_offset == text_utf16_len {
        Ok(text.len())
    } else {
        Err(TextOffsetError::Utf16OutOfBounds {
            offset: utf16_offset,
            text_utf16_len,
        })
    }
}

/// UTF-16 code unit offset → UTF-8 byte offset（方向性钳位版）。
///
/// 越界钳到文本末尾；落在代理对中间时映射到该码点起始（低代理项与高代理项
/// 同属一个字符位置，与 Qt QChar 既有约定一致）。调用方要区分失败时用
/// [`utf16_to_utf8`]。
pub fn utf16_to_utf8_clamped(text: &str, utf16_offset: usize) -> usize {
    let mut utf16_seen = 0usize;
    for (byte_pos, ch) in text.char_indices() {
        if utf16_seen >= utf16_offset {
            return byte_pos;
        }
        let ch_utf16_len = ch.len_utf16();
        if utf16_seen + ch_utf16_len > utf16_offset {
            return byte_pos;
        }
        utf16_seen += ch_utf16_len;
    }
    // 走到这里说明 utf16_offset 超出文本 UTF-16 长度（或正好等于末尾）：钳到末尾。
    text.len()
}

/// UTF-8 byte range 钳位到字符边界（两端都左对齐，保持 `start <= end`）。
pub fn clamp_utf8_range(text: &str, start: usize, end: usize) -> (usize, usize) {
    let start = clamp_to_char_boundary(text, start);
    let end = clamp_to_char_boundary(text, end);
    if start > end {
        (end, end)
    } else {
        (start, end)
    }
}

#[cfg(test)]
#[path = "text_coordinates_tests.rs"]
mod tests;
