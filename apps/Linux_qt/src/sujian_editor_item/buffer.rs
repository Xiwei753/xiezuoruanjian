/// 编辑器快照 — 旧版 EditorBuffer 的 undo/redo 记录单元。
///
/// cursor 和 selection_anchor 均为 UTF-8 byte offset。
/// 当前主链使用 EditorKernel + CommittedTextMirror，此类型仅用于 legacy 路径。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorSnapshot {
    pub text: String,
    pub cursor: usize,
    pub selection_anchor: usize,
}

/// 旧版编辑器缓冲区 — 不维护独立编辑真相，仅用于 Qt 侧的本地 undo/redo 栈。
///
/// 当前主链：EditorKernel 持有正文真相，CommittedTextMirror 增量同步。
/// EditorBuffer 的 undo/redo 栈在 Editor V2 模式下不再使用。
#[derive(Clone, Debug)]
#[derive(Default)]
pub struct EditorBuffer {
    pub text: String,
    pub cursor: usize,
    pub selection_anchor: usize,
    pub undo_stack: Vec<EditorSnapshot>,
    pub redo_stack: Vec<EditorSnapshot>,
}


impl EditorBuffer {
    pub fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            selection_anchor: self.selection_anchor,
        }
    }

    pub fn restore(&mut self, snapshot: EditorSnapshot) {
        self.text = snapshot.text;
        self.cursor = clamp_to_char_boundary(&self.text, snapshot.cursor);
        self.selection_anchor = clamp_to_char_boundary(&self.text, snapshot.selection_anchor);
    }

    pub fn has_selection(&self) -> bool {
        self.cursor != self.selection_anchor
    }

    /// 返回选区的半开区间 [start, end)（UTF-8 byte offset）。
    /// start ≤ end，无论光标和锚点的相对位置。
    pub fn selection_range(&self) -> (usize, usize) {
        if self.cursor <= self.selection_anchor {
            (self.cursor, self.selection_anchor)
        } else {
            (self.selection_anchor, self.cursor)
        }
    }

    pub fn selected_text(&self) -> String {
        if !self.has_selection() {
            return String::new();
        }
        let (start, end) = self.selection_range();
        self.text[start..end].to_string()
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.cursor = 0;
        self.selection_anchor = self.cursor;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub fn push_undo(&mut self, snapshot: EditorSnapshot) {
        if self.undo_stack.last() != Some(&snapshot) {
            self.undo_stack.push(snapshot);
        }
        if self.undo_stack.len() > 256 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn replace_selection_or_insert(&mut self, inserted: &str) {
        if inserted.is_empty() {
            return;
        }
        let (start, end) = self.selection_range();
        self.text.replace_range(start..end, inserted);
        self.cursor = start + inserted.len();
        self.cursor = clamp_to_char_boundary(&self.text, self.cursor);
        self.selection_anchor = self.cursor;
    }

    pub fn delete_selection(&mut self) -> bool {
        if !self.has_selection() {
            return false;
        }
        let (start, end) = self.selection_range();
        self.text.replace_range(start..end, "");
        self.cursor = start;
        self.selection_anchor = start;
        true
    }

    pub fn delete_backward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let Some(prev) = prev_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.text.replace_range(prev..self.cursor, "");
        self.cursor = prev;
        self.selection_anchor = prev;
        true
    }

    pub fn delete_forward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.text.replace_range(self.cursor..next, "");
        self.selection_anchor = self.cursor;
        true
    }

    pub fn move_cursor(&mut self, index: usize, extend: bool) {
        let next = clamp_to_char_boundary(&self.text, index);
        if !extend {
            self.selection_anchor = next;
        }
        self.cursor = next;
    }

    pub fn select_all(&mut self) {
        self.selection_anchor = 0;
        self.cursor = self.text.len();
    }

    pub fn undo(&mut self) -> Option<(EditorSnapshot, EditorSnapshot)> {
        let previous = self.undo_stack.pop()?;
        let current = self.snapshot();
        self.redo_stack.push(current.clone());
        self.restore(previous.clone());
        Some((current, previous))
    }

    pub fn redo(&mut self) -> Option<(EditorSnapshot, EditorSnapshot)> {
        let next = self.redo_stack.pop()?;
        let current = self.snapshot();
        self.undo_stack.push(current.clone());
        self.restore(next.clone());
        Some((current, next))
    }
}

/// 将富文本/平台文本归一化为编辑器纯文本。
///
/// 规则：段落分隔符 U+2029 → `\n`，CRLF/CR → `\n`，
/// 保留 `\n` 和 `\t`，过滤其余控制字符。
/// 归一化后的文本可直接写入 `chapter.md`（正文永远是纯文本）。
pub fn normalize_plain_text(text: &str) -> String {
    let replaced = text
        .replace('\u{2029}', "\n")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    replaced
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect()
}

/// 返回 `index` 之前最近的 char boundary（UTF-8 byte offset）。
///
/// 用于退格删除：定位前一个字符的起始字节位置。
/// 如果 `index` 为 0 或文本为空则返回 `None`。
pub fn prev_char_boundary(text: &str, index: usize) -> Option<usize> {
    if index == 0 || text.is_empty() {
        return None;
    }
    text.char_indices()
        .map(|(idx, _)| idx)
        .take_while(|idx| *idx < index)
        .last()
}

/// 返回 `index` 之后最近的 char boundary（UTF-8 byte offset）。
///
/// 用于 Delete 键：定位后一个字符的起始字节位置。
/// 如果 `index` 已在文本末尾则返回 `None`；末尾字符之后返回 `text.len()`。
pub fn next_char_boundary(text: &str, index: usize) -> Option<usize> {
    if index >= text.len() {
        return None;
    }
    text.char_indices()
        .map(|(idx, _)| idx)
        .find(|idx| *idx > index)
        .or(Some(text.len()))
}

/// 将 UTF-8 byte offset 对齐到最近的 char boundary。
///
/// 超出文本长度时返回 `text.len()`；落在多字节字符中间时向左回退。
/// 所有外部输入的 offset（来自 IME、光标移动等）都应经过此函数校验。
pub fn clamp_to_char_boundary(text: &str, index: usize) -> usize {
    if index > text.len() {
        return text.len();
    }
    if text.is_char_boundary(index) {
        return index;
    }
    let mut clamped = index;
    while clamped > 0 && !text.is_char_boundary(clamped) {
        clamped -= 1;
    }
    clamped
}

/// UTF-8 byte offset → 字符索引（char count）。
///
/// 先将 byte offset 对齐到 char boundary，再计算 `[0..offset)` 范围内的字符数。
/// 用于需要字符级计数的场景（如 Android StaticLayout）。
pub fn byte_to_char_index(text: &str, byte_index: usize) -> usize {
    text[..clamp_to_char_boundary(text, byte_index)]
        .chars()
        .count()
}

#[cfg(test)]
pub fn byte_index_at_char_offset_in_range(
    text: &str,
    start: usize,
    end: usize,
    char_offset: usize,
) -> usize {
    if char_offset == 0 {
        return start;
    }
    for (offset, (byte, _)) in text[start..end].char_indices().enumerate() {
        if offset == char_offset {
            return start + byte;
        }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_buffer_inserts_and_deletes_utf8_plain_text() {
        let mut buffer = EditorBuffer::default();
        buffer.replace_selection_or_insert("你");
        buffer.replace_selection_or_insert("好");
        assert_eq!(buffer.text, "你好");
        assert_eq!(buffer.cursor, buffer.text.len());
        assert!(buffer.delete_backward());
        assert_eq!(buffer.text, "你");
    }

    #[test]
    fn editor_buffer_replaces_selection_without_html_or_format_state() {
        let mut buffer = EditorBuffer::default();
        buffer.replace_selection_or_insert("abc");
        buffer.selection_anchor = 0;
        buffer.cursor = 3;
        buffer.replace_selection_or_insert("纯文本");
        assert_eq!(buffer.text, "纯文本");
        assert_eq!(buffer.selected_text(), "");
    }

    #[test]
    fn editor_buffer_undo_redo_keeps_plain_text_and_cursor() {
        let mut buffer = EditorBuffer::default();
        let before = buffer.snapshot();
        buffer.push_undo(before);
        buffer.replace_selection_or_insert("第一行\n第二行");

        let Some((_old, restored)) = buffer.undo() else {
            panic!("undo should restore the empty snapshot");
        };
        assert_eq!(restored.text, "");
        assert_eq!(buffer.text, "");
        assert_eq!(buffer.cursor, 0);

        let Some((_old, redone)) = buffer.redo() else {
            panic!("redo should restore inserted plain text");
        };
        assert_eq!(redone.text, "第一行\n第二行");
        assert_eq!(buffer.text, "第一行\n第二行");
        assert_eq!(buffer.cursor, buffer.text.len());
    }

    #[test]
    fn line_char_offset_does_not_jump_to_document_end() {
        let text = "第一行\n第二行";
        let line_end = "第一行".len();

        assert_eq!(
            byte_index_at_char_offset_in_range(text, 0, line_end, 3),
            line_end
        );
    }

    #[test]
    fn test_next_char_boundary_empty() {
        assert_eq!(next_char_boundary("", 0), None);
        assert_eq!(next_char_boundary("", 1), None);
    }

    #[test]
    fn test_next_char_boundary_ascii() {
        let text = "abc";
        assert_eq!(next_char_boundary(text, 0), Some(1));
        assert_eq!(next_char_boundary(text, 1), Some(2));
        assert_eq!(next_char_boundary(text, 2), Some(3));
        assert_eq!(next_char_boundary(text, 3), None);
        assert_eq!(next_char_boundary(text, 10), None);
    }

    #[test]
    fn test_next_char_boundary_utf8() {
        let text = "你好"; // 3 bytes each
        assert_eq!(next_char_boundary(text, 0), Some(3));
        assert_eq!(next_char_boundary(text, 1), Some(3));
        assert_eq!(next_char_boundary(text, 2), Some(3));
        assert_eq!(next_char_boundary(text, 3), Some(6));
        assert_eq!(next_char_boundary(text, 4), Some(6));
        assert_eq!(next_char_boundary(text, 6), None);
        assert_eq!(next_char_boundary(text, 10), None);
    }
}
