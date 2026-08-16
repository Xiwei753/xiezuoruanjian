//! Rust EditorKernel — 正文和业务唯一真相。

mod apply;
mod composition;
mod history;
mod replace;
pub mod result;
mod rope_tests;
mod selection;
mod session;
mod tests;
pub mod types;

use self::result::EditorInputError;
use crate::editor::strong_types::{
    EditorRevision, EditorSessionGeneration, EditorSessionId, Utf16CodeUnitOffset, Utf8ByteOffset,
    Utf8ByteRange,
};
use crop::Rope;

/// #624 评论8 — 编辑 delta：只保存实际删除/插入的局部文本及 old/new ranges。
///
/// `old_range` 是编辑前正文中的半开 byte range，`new_range` 是编辑后正文中的
/// 半开 byte range（同一逻辑编辑位置）；`deleted_text`/`inserted_text` 只保存
/// 被替换区间的局部文本，不再保存两份全文。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextEditDelta {
    pub(crate) old_range: Utf8ByteRange,
    pub(crate) new_range: Utf8ByteRange,
    pub(crate) deleted_text: String,
    pub(crate) inserted_text: String,
}

/// #624 评论8 — Undo 栈条目：编辑 delta 列表 + 编辑前后选区。
///
/// 普通编辑一个 delta；deleteSurrounding 两个；replace-all 多个。
/// Undo 按 new_range 逆序应用 inverse delta，Redo 按 old_range 从后往前（降序）
/// 应用 forward delta（避免先前替换使后面 delta 的旧文本坐标漂移）。
#[derive(Debug, Clone)]
pub(crate) struct UndoEntry {
    pub(crate) edits: Vec<TextEditDelta>,
    pub(crate) old_selection: Utf8ByteRange,
    pub(crate) new_selection: Utf8ByteRange,
}

#[derive(Debug, Clone)]
pub struct EditorKernel {
    text: Rope,
    revision: EditorRevision,
    cursor: Utf8ByteOffset,
    selection_anchor: Utf8ByteOffset,
    next_transaction_id: u64,
    animation_duration_ms: u64,
    animation_enabled: bool,
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    composition_session: Option<CompositionSessionState>,
    next_composition_session_id: EditorSessionId,
}

#[derive(Debug, Clone)]
pub(crate) struct CompositionSessionState {
    pub(crate) session_id: EditorSessionId,
    pub(crate) base_revision: EditorRevision,
    pub(crate) generation: EditorSessionGeneration,
    pub(crate) replace_start: Utf8ByteOffset,
    pub(crate) replace_end_exclusive: Utf8ByteOffset,
    pub(crate) preedit_text: String,
    pub(crate) preedit_cursor_utf16: Utf16CodeUnitOffset,
}

impl Default for EditorKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorKernel {
    pub fn new() -> Self {
        Self {
            text: Rope::new(),
            revision: EditorRevision::initial(),
            cursor: Utf8ByteOffset::unchecked(0),
            selection_anchor: Utf8ByteOffset::unchecked(0),
            next_transaction_id: 1,
            animation_duration_ms: 80,
            animation_enabled: true,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            composition_session: None,
            next_composition_session_id: EditorSessionId::new(1),
        }
    }

    pub fn with_text(text: String, cursor: usize) -> Result<Self, EditorInputError> {
        if cursor > text.len() || !text.is_char_boundary(cursor) {
            return Err(EditorInputError::InvalidCursorOffset {
                offset: cursor,
                text_len: text.len(),
            });
        }
        Ok(Self {
            text: Rope::from(text),
            revision: EditorRevision::initial(),
            cursor: Utf8ByteOffset::unchecked(cursor),
            selection_anchor: Utf8ByteOffset::unchecked(cursor),
            next_transaction_id: 1,
            animation_duration_ms: 80,
            animation_enabled: true,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            composition_session: None,
            next_composition_session_id: EditorSessionId::new(1),
        })
    }

    pub fn set_animation_duration_ms(&mut self, duration_ms: u64) {
        self.animation_duration_ms = duration_ms;
    }

    pub fn set_animation_enabled(&mut self, enabled: bool) {
        self.animation_enabled = enabled;
    }

    /// #624 评论8 — 热路径访问器：UTF-8 byte 长度，O(1)。
    pub fn byte_len(&self) -> usize {
        self.text.byte_len()
    }

    /// #624 评论8 — 热路径访问器：offset 是否 UTF-8 char 边界（O(1)）。
    ///
    /// 越界 offset 一律返回 false（crop 自身在越界时 panic，调用方需先自查）。
    pub fn is_char_boundary(&self, byte_offset: usize) -> bool {
        byte_offset <= self.text.byte_len() && self.text.is_char_boundary(byte_offset)
    }

    /// #624 评论8 — 热路径访问器：局部 byte slice（不 materialize 全文）。
    ///
    /// `start`/`end` 为 UTF-8 byte offset 半开区间，必须已通过 [Self::is_char_boundary]
    /// 或其它边界验证；越界会 panic（调用方契约保证）。
    pub fn byte_slice(&self, start: usize, end: usize) -> crop::RopeSlice<'_> {
        self.text.byte_slice(start..end)
    }

    /// #624 评论8 — 冷路径访问器：全文 String。
    ///
    /// 只在 session snapshot、save/load、global search、replace-all 等明确需要
    /// 全文的边界调用；普通输入热路径不得使用。
    pub fn snapshot_text(&self) -> String {
        self.text.to_string()
    }

    /// 内部只读访问 Rope（供 app_service 边界校验与 Rust 平台适配器
    /// （Linux Qt 等）做零拷贝 clamp/校验使用）。普通输入热路径可用
    /// [Utf8ByteOffset::clamp_rope] 避免 materialize 全文。
    pub fn rope(&self) -> &Rope {
        &self.text
    }

    pub fn revision(&self) -> u64 {
        self.revision.value()
    }

    pub fn cursor(&self) -> usize {
        self.cursor.value()
    }

    pub fn selection_anchor(&self) -> usize {
        self.selection_anchor.value()
    }

    pub fn selection(&self) -> (usize, usize) {
        (self.selection_anchor.value(), self.cursor.value())
    }

    /// #606: 返回严格在 `byte_offset` 之前的最近 grapheme cluster 边界（UTF-8 byte offset）。
    ///
    /// #624 评论8：从光标附近 RopeSlice 迭代 — 取 `[0, offset)` slice 的最后一个
    /// grapheme（`next_back()`），不再从全文开头 `grapheme_indices(true)` 扫到光标。
    ///
    /// 平台端 Backspace/Delete 的 grapheme 边界计算由 Core 唯一决定，
    /// 不再依赖 ICU BreakIterator。
    #[allow(clippy::cast_possible_truncation)]
    pub fn previous_grapheme_boundary(&self, byte_offset: u32) -> u32 {
        let offset = byte_offset as usize;
        let len = self.text.byte_len();
        if offset == 0 {
            return 0;
        }
        if offset > len {
            return len as u32;
        }
        let prefix = self.text.byte_slice(0..offset);
        match prefix.graphemes().next_back() {
            Some(g) => (offset - g.len()) as u32,
            None => 0,
        }
    }

    /// #606: 返回严格在 `byte_offset` 之后的最近 grapheme cluster 边界（UTF-8 byte offset）。
    ///
    /// #624 评论8：从光标附近 RopeSlice 迭代 — 只取光标后一个小窗口（每次最多
    /// 64 个 Unicode scalar）用标准分段规则求第一个 cluster 边界；若 cluster 延伸
    /// 出窗口则自动向后扩展。不从全文开头 `grapheme_indices(true)` 扫描，也不依赖
    /// crop 前向 grapheme 迭代（crop 0.4.3 前向迭代不合并 regional-indicator 对）。
    ///
    /// 平台端 Backspace/Delete 的 grapheme 边界计算由 Core 唯一决定，
    /// 不再依赖 ICU BreakIterator。
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::too_many_lines, clippy::excessive_nesting)]
    pub fn next_grapheme_boundary(&self, byte_offset: u32) -> u32 {
        use unicode_segmentation::UnicodeSegmentation;

        let offset = byte_offset as usize;
        let len = self.text.byte_len();
        if offset >= len {
            return len as u32;
        }

        // 向后扩展窗口（每次 64 个 Unicode scalar 边界），直到窗口内能确定
        // 从 offset 开始（或包含 offset）的第一个完整 grapheme 的结束位置。
        const WINDOW_CHARS: usize = 64;
        let mut window_end = offset;
        loop {
            // 从 window_end 向后取最多 WINDOW_CHARS 个 char 边界。
            let mut chars_taken = 0usize;
            let slice = self.text.byte_slice(window_end..len);
            let mut char_end = window_end;
            for ch in slice.chars() {
                char_end += ch.len_utf8();
                chars_taken += 1;
                if chars_taken >= WINDOW_CHARS {
                    break;
                }
            }
            window_end = char_end;
            let window = self.text.byte_slice(offset..window_end).to_string();
            // 窗口内第一个 grapheme 的结束（相对窗口起点）。
            let first_end = window
                .grapheme_indices(true)
                .next()
                .map_or(window.len(), |(_, g)| g.len());
            if first_end < window.len() || window_end == len {
                return (offset + first_end) as u32;
            }
            // cluster 延伸出窗口：继续向后扩展。
        }
    }
}
