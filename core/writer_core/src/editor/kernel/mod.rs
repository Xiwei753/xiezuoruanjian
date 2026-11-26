//! Rust EditorKernel — 正文和业务唯一真相。

mod apply;
mod composition;
mod content_digest;
mod history;
mod patch_batch;
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
use crate::editor::transaction::{EditorChange, EditorSelection};
use crop::Rope;

pub(crate) use content_digest::{ContentDigest, CONTENT_DIGEST_VERSION};

/// 编辑 delta：只保存实际删除/插入的局部文本及 old/new ranges。
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

/// Undo 栈条目：保存编辑 delta 列表和编辑前后选区。
///
/// 普通编辑一个 delta；deleteSurrounding 两个；replace-all 多个。
/// Undo 按 new_range 逆序应用 inverse delta，Redo 按 old_range 从后往前（降序）
/// 应用 forward delta（避免先前替换使后面 delta 的旧文本坐标漂移）。
#[derive(Debug, Clone)]
pub(crate) struct UndoEntry {
    pub(crate) edits: Vec<TextEditDelta>,
    /// 编辑前选区（保留 anchor/head 方向）。
    ///
    /// Issue #683：undo/redo 必须恢复原方向，不能用排序后的
    /// Utf8ByteRange 反推 anchor/head——反向选区会被翻成正向。
    pub(crate) old_selection: EditorSelection,
    pub(crate) new_selection: EditorSelection,
}

#[derive(Debug, Clone)]
pub struct EditorKernel {
    text: Rope,
    revision: EditorRevision,
    cursor: Utf8ByteOffset,
    selection_anchor: Utf8ByteOffset,
    next_transaction_id: u64,
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    composition_session: Option<CompositionSessionState>,
    next_composition_session_id: EditorSessionId,
    /// 本次 `apply()` 中实际执行的插入/删除操作（按执行顺序）。
    ///
    /// 只有真正修改了正文的局部操作才记录；composition preedit 不记录，
    /// IME commit 才记录。`apply()` 结束时把这些操作原样交给
    /// [`self::result::EditorEditResult::changes`]，供写作统计/手写证明使用，
    /// 平台端不再按最终字符串猜 diff。
    pending_changes: Vec<EditorChange>,
    /// 与正文同步维护的增量内容摘要；事务前后根值 O(1) 读取。
    content_digest: ContentDigest,
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
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            composition_session: None,
            next_composition_session_id: EditorSessionId::new(1),
            pending_changes: Vec::new(),
            content_digest: ContentDigest::new(),
        }
    }

    pub fn with_text(text: String, cursor: usize) -> Result<Self, EditorInputError> {
        if cursor > text.len() || !text.is_char_boundary(cursor) {
            return Err(EditorInputError::InvalidCursorOffset {
                offset: cursor,
                text_len: text.len(),
            });
        }
        let content_digest = ContentDigest::from_text(&text);
        Ok(Self {
            text: Rope::from(text),
            revision: EditorRevision::initial(),
            cursor: Utf8ByteOffset::unchecked(cursor),
            selection_anchor: Utf8ByteOffset::unchecked(cursor),
            next_transaction_id: 1,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            composition_session: None,
            next_composition_session_id: EditorSessionId::new(1),
            pending_changes: Vec::new(),
            content_digest,
        })
    }

    /// 记录一次实际写入正文的操作（仅供 kernel 内部与证明/统计使用）。
    pub(crate) fn push_change(&mut self, change: EditorChange) {
        self.pending_changes.push(change);
    }

    /// 取走本次事务累积的实际变更列表。
    pub(crate) fn take_pending_changes(&mut self) -> Vec<EditorChange> {
        std::mem::take(&mut self.pending_changes)
    }

    ///   — 热路径访问器：UTF-8 byte 长度，O(1)。
    pub fn byte_len(&self) -> usize {
        self.text.byte_len()
    }

    ///   — 热路径访问器：offset 是否 UTF-8 char 边界（O(1)）。
    ///
    /// 越界 offset 一律返回 false（crop 自身在越界时 panic，调用方需先自查）。
    pub fn is_char_boundary(&self, byte_offset: usize) -> bool {
        byte_offset <= self.text.byte_len() && self.text.is_char_boundary(byte_offset)
    }

    ///   — 热路径访问器：局部 byte slice（不 materialize 全文）。
    ///
    /// `start`/`end` 为 UTF-8 byte offset 半开区间，必须已通过 [Self::is_char_boundary]
    /// 或其它边界验证；越界会 panic（调用方契约保证）。
    pub fn byte_slice(&self, start: usize, end: usize) -> crop::RopeSlice<'_> {
        self.text.byte_slice(start..end)
    }

    ///   — 冷路径访问器：全文 String。
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

    /// #606 / #1063: 返回严格在 `byte_offset` 之前的最近 grapheme cluster 边界
    /// （UTF-8 byte offset）。
    ///
    /// Issue #1063：旧实现只把 `[0, offset)` 前缀当独立字符串做 `next_back()`，
    /// 切断了 UAX #29 需要的**前导上下文**（emoji ZWJ 序列、RI 旗帜奇偶对、
    /// 组合音标），会把 `👨‍👩` 的簇内 byte offset 4 当成簇边界。
    ///
    /// 现在用 [`unicode_segmentation::GraphemeCursor`] 在完整文档上后退：窗口不足时
    /// 按 `GraphemeIncomplete::PreContext` / `PrevChunk` 向前提供真实上下文，
    /// 直到落在一个**整个文档**的 UAX #29 扩展字素边界上。
    ///
    /// 输入若落在簇内（含多字节字符中间），返回该位置所在字素簇的起始边界
    /// （文档真实边界集合中的值，不跳过所在簇）；`byte_offset` 越界时返回文末。
    ///
    /// 平台端 Backspace/Delete/方向键的 grapheme 边界计算由 Core 唯一决定，
    /// 不再依赖 ICU BreakIterator 或平台自己的 char 迭代。
    #[allow(clippy::cast_possible_truncation)]
    pub fn previous_grapheme_boundary(&self, byte_offset: u32) -> u32 {
        use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

        let len = self.text.byte_len();
        // 越界输入沿用既有契约：返回文末（文末也是合法边界）。
        if byte_offset as usize > len {
            return len as u32;
        }
        let offset = byte_offset as usize;
        // 落在多字节字符中间时，先前进到该字符之后的边界再后退：
        // 这样返回的是「所在字素簇的起始边界」，既在文档 UAX #29 边界集合中，
        // 也不会跳过包含该位置的簇（例如 "a😀b" offset=2 → 1，而不是 0）。
        let offset = if self.text.is_char_boundary(offset) {
            offset
        } else {
            let mut forward = offset.saturating_add(1);
            while forward < len && !self.text.is_char_boundary(forward) {
                forward += 1;
            }
            forward.min(len)
        };
        if offset == 0 {
            return 0;
        }

        let mut cursor = GraphemeCursor::new(offset, len, true);
        let mut chunk_end = offset;
        loop {
            let chunk_start = Self::grapheme_chunk_start(&self.text, chunk_end);
            if chunk_start == chunk_end {
                return 0;
            }
            let chunk = Self::grapheme_chunk(&self.text, chunk_start, chunk_end);
            match cursor.prev_boundary(&chunk, chunk_start) {
                Ok(Some(boundary)) => return boundary as u32,
                Ok(None) => return 0,
                Err(GraphemeIncomplete::PreContext(context_end)) => {
                    let context_start = Self::grapheme_chunk_start(&self.text, context_end);
                    let context = Self::grapheme_chunk(&self.text, context_start, context_end);
                    cursor.provide_context(&context, context_start);
                    // 同一个 chunk 重试；上下文已补上。
                }
                Err(GraphemeIncomplete::PrevChunk) => {
                    chunk_end = chunk_start;
                }
                Err(_) => return 0,
            }
        }
    }

    /// #606 / #1063: 返回严格在 `byte_offset` 之后的最近 grapheme cluster 边界
    /// （UTF-8 byte offset）。
    ///
    /// 用 [`unicode_segmentation::GraphemeCursor`] 在完整文档上前进：窗口不足时
    /// 按 `NextChunk` 向后扩展，`PreContext` 时向前提供真实上下文。RI 奇偶配对、
    /// Extend/ZWJ、组合音标、CRLF 都按 UAX #29 扩展字素规则处理。
    ///
    /// 输入若落在簇内（含多字节字符中间），先左对齐到字符起始再求所在簇的结束，
    /// 结果仍是文档真实边界；`byte_offset` 越界时返回文末。
    ///
    /// 平台端 Backspace/Delete/方向键的 grapheme 边界计算由 Core 唯一决定，
    /// 不再依赖 ICU BreakIterator 或平台自己的 char 迭代。
    #[allow(clippy::cast_possible_truncation)]
    pub fn next_grapheme_boundary(&self, byte_offset: u32) -> u32 {
        use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

        let len = self.text.byte_len();
        let offset = (byte_offset as usize).min(len);
        let offset =
            crate::editor::text_coordinates::rope_clamp_to_char_boundary(&self.text, offset);
        if offset >= len {
            return len as u32;
        }

        let mut cursor = GraphemeCursor::new(offset, len, true);
        let mut chunk_start = offset;
        loop {
            let chunk_end = Self::grapheme_chunk_end(&self.text, chunk_start, len);
            if chunk_end <= chunk_start {
                return len as u32;
            }
            let chunk = Self::grapheme_chunk(&self.text, chunk_start, chunk_end);
            match cursor.next_boundary(&chunk, chunk_start) {
                Ok(Some(boundary)) => return boundary as u32,
                Ok(None) => return len as u32,
                Err(GraphemeIncomplete::NextChunk) => {
                    chunk_start = chunk_end;
                }
                Err(GraphemeIncomplete::PreContext(context_end)) => {
                    let context_start = Self::grapheme_chunk_start(&self.text, context_end);
                    let context = Self::grapheme_chunk(&self.text, context_start, context_end);
                    cursor.provide_context(&context, context_start);
                }
                Err(_) => return len as u32,
            }
        }
    }

    /// #1063: 把任意 byte offset 吸附到所在的 UAX #29 扩展字素边界。
    ///
    /// 已经是字素边界时原样返回；落在簇内时返回该簇的起始边界（例如点击
    /// 组合音标/emoji 簇中间，逻辑光标停在整簇起点）。越界钳到文末。
    ///
    /// 平台端点击命中、拖动选区、IME 提交后的逻辑 cursor 统一用它吸附，
    /// 避免「光标停在簇内、删除/方向键却按整簇移动」的分歧。
    #[allow(clippy::cast_possible_truncation)]
    pub fn snap_to_grapheme_boundary(&self, byte_offset: u32) -> u32 {
        let len = self.text.byte_len();
        let offset = (byte_offset as usize).min(len);
        if offset == 0 || offset >= len {
            return offset as u32;
        }
        let previous = self.previous_grapheme_boundary(offset as u32);
        if usize::try_from(self.next_grapheme_boundary(previous)).unwrap_or(len) == offset {
            offset as u32
        } else {
            previous
        }
    }

    /// grapheme 迭代窗口上限：一次最多取 256 byte 的真实文档上下文。    ///
    /// 256 byte 远大于单个 Unicode 标量（≤4 byte），保证窗口内一定能推进；
    /// 超出窗口需要的上下文由 `GraphemeCursor` 的 PreContext/PrevChunk/NextChunk
    /// 逐段补齐，而不是把整章正文复制成 String。
    const GRAPHEME_WINDOW_BYTES: usize = 256;

    /// 以 `end` 结尾、最长 [`Self::GRAPHEME_WINDOW_BYTES`] 的窗口起点（char boundary）。
    fn grapheme_chunk_start(text: &crop::Rope, end: usize) -> usize {
        let start = end.saturating_sub(Self::GRAPHEME_WINDOW_BYTES);
        crate::editor::text_coordinates::rope_clamp_to_char_boundary(text, start)
    }

    /// 从 `start` 开始、最长 [`Self::GRAPHEME_WINDOW_BYTES`] 的窗口终点（char boundary）。
    fn grapheme_chunk_end(text: &crop::Rope, start: usize, len: usize) -> usize {
        let end = (start + Self::GRAPHEME_WINDOW_BYTES).min(len);
        crate::editor::text_coordinates::rope_clamp_to_char_boundary(text, end)
    }

    /// 取窗口文本。窗口 ≤256 byte，只复制这一小段；不退化成全文 materialize。
    fn grapheme_chunk(text: &crop::Rope, start: usize, end: usize) -> String {
        text.byte_slice(start..end).to_string()
    }
}
