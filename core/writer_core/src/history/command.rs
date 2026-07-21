use crate::editor::transaction::EditorChange;

/// 可撤销的编辑命令 — forward/inverse 变更对 + 光标位置。
///
/// `cursor_before` / `cursor_after` 均为 UTF-8 byte offset，
/// 由 `clamp_cursor_to_char_boundary` 保证对齐到字符边界。
/// inverse 由 `compute_inverse` 从 forward 自动推导（Insert ↔ Delete 对称），
/// 不需要调用方手动构造。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEditCommand {
    /// 正向变更序列，按顺序应用。
    pub forward: Vec<EditorChange>,
    /// 反向变更序列，逆序应用（从后往前撤销）。
    pub inverse: Vec<EditorChange>,
    /// 执行前的光标位置（UTF-8 byte offset）。
    pub cursor_before: usize,
    /// 执行后的光标位置（UTF-8 byte offset）。
    pub cursor_after: usize,
}

impl TextEditCommand {
    pub fn new(forward: Vec<EditorChange>, cursor_before: usize, cursor_after: usize) -> Self {
        let inverse = compute_inverse(&forward);
        Self {
            forward,
            inverse,
            cursor_before,
            cursor_after,
        }
    }

    pub fn apply_forward(&self, text: &mut String, cursor: &mut usize) {
        for change in &self.forward {
            apply_change(text, change);
        }
        *cursor = clamp_cursor_to_char_boundary(text, self.cursor_after);
    }

    /// 应用反向变更序列（撤销）。
    ///
    /// 关键：inverse 必须逆序应用（`.rev()`），因为多步变更的 offset 依赖于
    /// 前序变更的执行结果。例如 Insert(0, "ab") + Insert(2, "cd") 的逆操作
    /// 必须先删 "cd"（offset=2）再删 "ab"（offset=0），顺序颠倒会导致 offset 错位。
    pub fn apply_inverse(&self, text: &mut String, cursor: &mut usize) {
        for change in self.inverse.iter().rev() {
            apply_change(text, change);
        }
        *cursor = clamp_cursor_to_char_boundary(text, self.cursor_before);
    }
}

/// 从 forward 变更序列推导 inverse。
///
/// 对称规则：Insert → Delete（保留原文），Delete → Insert（恢复原文）。
/// inverse 的应用顺序必须与 forward 相反（`.rev()`），以保证多步变更正确撤销。
fn compute_inverse(changes: &[EditorChange]) -> Vec<EditorChange> {
    changes
        .iter()
        .map(|c| match c {
            EditorChange::Insert { index, text } => EditorChange::Delete {
                index: *index,
                text: text.clone(),
            },
            EditorChange::Delete { index, text } => EditorChange::Insert {
                index: *index,
                text: text.clone(),
            },
        })
        .collect()
}

/// 将单个变更应用到文本。
///
/// 边界验证：如果 `index` 超出文本长度或不在 char boundary 上，静默跳过（不 panic）。
/// 对于 Delete，还会验证 `index..index+text.len()` 的结束位置是否在 char boundary 上。
/// 这种防御策略保证损坏的命令不会破坏文本，但调用方应确保命令由合法编辑产生。
fn apply_change(text: &mut String, change: &EditorChange) {
    match change {
        EditorChange::Insert { index, text: ins } => {
            if *index > text.len() || !text.is_char_boundary(*index) {
                return;
            }
            text.insert_str(*index, ins);
        }
        EditorChange::Delete { index, text: del } => {
            if *index > text.len() || !text.is_char_boundary(*index) {
                return;
            }
            let end = index + del.len();
            if end > text.len() || !text.is_char_boundary(end) {
                return;
            }
            if *index < end {
                text.drain(*index..end);
            }
        }
    }
}

/// 将光标偏移量对齐到最近的合法 UTF-8 char boundary。
///
/// 策略：如果 offset 恰好在 char boundary 上则直接返回；
/// 否则向左逐字节回退直到找到 char boundary。
/// 超出文本长度时回退到文本末尾的 char boundary。
/// 这保证光标不会落在多字节字符的中间字节上。
fn clamp_cursor_to_char_boundary(text: &str, offset: usize) -> usize {
    if offset <= text.len() && text.is_char_boundary(offset) {
        return offset;
    }
    if offset > text.len() {
        let mut clamped = text.len();
        while clamped > 0 && !text.is_char_boundary(clamped) {
            clamped -= 1;
        }
        return clamped;
    }
    let mut clamped = offset;
    while clamped > 0 && !text.is_char_boundary(clamped) {
        clamped -= 1;
    }
    clamped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_undo_redo() {
        let cmd = TextEditCommand::new(
            vec![EditorChange::Insert {
                index: 2,
                text: "XX".to_string(),
            }],
            2,
            4,
        );

        let mut text = "abcd".to_string();
        let mut cursor = 2;

        cmd.apply_forward(&mut text, &mut cursor);
        assert_eq!(text, "abXXcd");
        assert_eq!(cursor, 4);

        cmd.apply_inverse(&mut text, &mut cursor);
        assert_eq!(text, "abcd");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn delete_undo_redo() {
        let cmd = TextEditCommand::new(
            vec![EditorChange::Delete {
                index: 1,
                text: "bc".to_string(),
            }],
            3,
            1,
        );

        let mut text = "abcd".to_string();
        let mut cursor = 3;

        cmd.apply_forward(&mut text, &mut cursor);
        assert_eq!(text, "ad");
        assert_eq!(cursor, 1);

        cmd.apply_inverse(&mut text, &mut cursor);
        assert_eq!(text, "abcd");
        assert_eq!(cursor, 3);
    }

    #[test]
    fn replace_undo_redo() {
        let cmd = TextEditCommand::new(
            vec![
                EditorChange::Delete {
                    index: 1,
                    text: "bc".to_string(),
                },
                EditorChange::Insert {
                    index: 1,
                    text: "XY".to_string(),
                },
            ],
            3,
            3,
        );

        let mut text = "abcd".to_string();
        let mut cursor = 3;

        cmd.apply_forward(&mut text, &mut cursor);
        assert_eq!(text, "aXYd");

        cmd.apply_inverse(&mut text, &mut cursor);
        assert_eq!(text, "abcd");
    }

    #[test]
    fn apply_forward_clamps_cursor_out_of_bounds() {
        let cmd = TextEditCommand::new(
            vec![EditorChange::Insert {
                index: 1,
                text: "X".to_string(),
            }],
            0,
            100,
        );

        let mut text = "a".to_string();
        let mut cursor = 0;

        cmd.apply_forward(&mut text, &mut cursor);
        assert_eq!(text, "aX");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn apply_inverse_clamps_cursor_out_of_bounds() {
        let cmd = TextEditCommand::new(
            vec![EditorChange::Insert {
                index: 1,
                text: "X".to_string(),
            }],
            100,
            2,
        );

        let mut text = "aX".to_string();
        let mut cursor = 2;

        cmd.apply_inverse(&mut text, &mut cursor);
        assert_eq!(text, "a");
        assert_eq!(cursor, 1);
    }

    #[test]
    fn apply_change_skips_invalid_insert_index() {
        let mut text = "abc".to_string();
        apply_change(&mut text, &EditorChange::Insert { index: 100, text: "X".into() });
        assert_eq!(text, "abc");
    }

    #[test]
    fn apply_change_skips_invalid_delete_range() {
        let mut text = "abc".to_string();
        apply_change(&mut text, &EditorChange::Delete { index: 100, text: "abc".into() });
        assert_eq!(text, "abc");
    }

    #[test]
    fn clamp_cursor_to_char_boundary_mid_multibyte() {
        let text = "你好";
        let byte_len = text.len();
        assert!(byte_len > 3);
        let clamped = clamp_cursor_to_char_boundary(text, 1);
        assert_eq!(clamped, 0);
        let clamped2 = clamp_cursor_to_char_boundary(text, byte_len + 10);
        assert_eq!(clamped2, byte_len);
    }
}
