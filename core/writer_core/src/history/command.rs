use crate::editor::transaction::EditorChange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEditCommand {
    pub forward: Vec<EditorChange>,
    pub inverse: Vec<EditorChange>,
    pub cursor_before: usize,
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

    pub fn apply_inverse(&self, text: &mut String, cursor: &mut usize) {
        for change in self.inverse.iter().rev() {
            apply_change(text, change);
        }
        *cursor = clamp_cursor_to_char_boundary(text, self.cursor_before);
    }
}

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
