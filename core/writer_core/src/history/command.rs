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
        *cursor = self.cursor_after.min(text.len());
    }

    pub fn apply_inverse(&self, text: &mut String, cursor: &mut usize) {
        for change in self.inverse.iter().rev() {
            apply_change(text, change);
        }
        *cursor = self.cursor_before.min(text.len());
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
            let pos = (*index).min(text.len());
            text.insert_str(pos, ins);
        }
        EditorChange::Delete { index, text: del } => {
            let start = (*index).min(text.len());
            let end = (start + del.len()).min(text.len());
            if start < end {
                text.drain(start..end);
            }
        }
    }
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
}
