use super::command::TextEditCommand;

const DEFAULT_MAX_STACK: usize = 256;

#[derive(Debug)]
pub struct HistoryManager {
    undo_stack: Vec<TextEditCommand>,
    redo_stack: Vec<TextEditCommand>,
    max_stack: usize,
}

impl HistoryManager {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_stack: DEFAULT_MAX_STACK,
        }
    }

    pub fn with_limit(max_stack: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_stack: max_stack.max(1),
        }
    }

    pub fn push(&mut self, command: TextEditCommand) {
        if command.forward.is_empty() {
            return;
        }
        self.redo_stack.clear();
        self.undo_stack.push(command);
        if self.undo_stack.len() > self.max_stack {
            self.undo_stack.remove(0);
        }
    }

    pub fn undo(&mut self, text: &mut String, cursor: &mut usize) -> bool {
        if let Some(cmd) = self.undo_stack.pop() {
            cmd.apply_inverse(text, cursor);
            self.redo_stack.push(cmd);
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self, text: &mut String, cursor: &mut usize) -> bool {
        if let Some(cmd) = self.redo_stack.pop() {
            cmd.apply_forward(text, cursor);
            self.undo_stack.push(cmd);
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }
}

impl Default for HistoryManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::transaction::EditorChange;

    fn insert_cmd(
        index: usize,
        text: &str,
        cursor_before: usize,
        cursor_after: usize,
    ) -> TextEditCommand {
        TextEditCommand::new(
            vec![EditorChange::Insert {
                index,
                text: text.to_string(),
            }],
            cursor_before,
            cursor_after,
        )
    }

    #[test]
    fn undo_restores_text() {
        let mut mgr = HistoryManager::new();
        let mut text = "hello".to_string();
        let mut cursor = 5usize;

        let cmd = insert_cmd(5, " world", 5, 11);
        cmd.apply_forward(&mut text, &mut cursor);
        assert_eq!(text, "hello world");
        mgr.push(cmd);

        assert!(mgr.undo(&mut text, &mut cursor));
        assert_eq!(text, "hello");
        assert_eq!(cursor, 5);
    }

    #[test]
    fn redo_reapplies() {
        let mut mgr = HistoryManager::new();
        let mut text = "hello".to_string();
        let mut cursor = 5usize;

        let cmd = insert_cmd(5, " world", 5, 11);
        cmd.apply_forward(&mut text, &mut cursor);
        mgr.push(cmd);

        mgr.undo(&mut text, &mut cursor);
        assert_eq!(text, "hello");

        assert!(mgr.redo(&mut text, &mut cursor));
        assert_eq!(text, "hello world");
    }

    #[test]
    fn push_clears_redo() {
        let mut mgr = HistoryManager::new();
        let mut text = "a".to_string();
        let mut cursor = 1usize;

        let cmd1 = insert_cmd(1, "b", 1, 2);
        cmd1.apply_forward(&mut text, &mut cursor);
        mgr.push(cmd1);
        mgr.undo(&mut text, &mut cursor);

        let cmd2 = insert_cmd(1, "c", 1, 2);
        cmd2.apply_forward(&mut text, &mut cursor);
        mgr.push(cmd2);
        assert_eq!(text, "ac");
        assert!(!mgr.can_redo());
    }

    #[test]
    fn stack_limit_evicts_oldest() {
        let mut mgr = HistoryManager::with_limit(3);
        let mut text = String::new();
        let mut cursor = 0usize;

        for i in 0..5 {
            let cmd = insert_cmd(i, "x", i, i + 1);
            cmd.apply_forward(&mut text, &mut cursor);
            mgr.push(cmd);
        }
        assert_eq!(mgr.undo_count(), 3);
    }

    #[test]
    fn clear_resets_stacks() {
        let mut mgr = HistoryManager::new();
        let mut text = "a".to_string();
        let mut cursor = 1usize;

        let cmd = insert_cmd(1, "b", 1, 2);
        cmd.apply_forward(&mut text, &mut cursor);
        mgr.push(cmd);
        mgr.undo(&mut text, &mut cursor);

        mgr.clear();
        assert!(!mgr.can_undo());
        assert!(!mgr.can_redo());
    }

    #[test]
    fn empty_command_not_pushed() {
        let mut mgr = HistoryManager::new();
        mgr.push(TextEditCommand::new(vec![], 0, 0));
        assert_eq!(mgr.undo_count(), 0);
    }

    #[test]
    fn undo_empty_stack_returns_false() {
        let mut mgr = HistoryManager::new();
        let mut text = "initial".to_string();
        let mut cursor = 0usize;

        assert!(!mgr.undo(&mut text, &mut cursor));
        assert_eq!(text, "initial");
        assert_eq!(cursor, 0);
    }
}
