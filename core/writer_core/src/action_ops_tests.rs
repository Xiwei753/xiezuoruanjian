use crate::facade::WriterCore;
use tempfile::tempdir;

#[test]
fn test_list_registered_actions() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    let actions = core.list_registered_actions().unwrap();
    assert!(!actions.is_empty(), "Action registry should contain actions");
}

#[test]
fn test_get_action() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    let action_id = "settings.editor.font_size.get";
    let action = core.get_action(action_id).unwrap();
    assert!(action.is_some(), "Should find action {}", action_id);
    assert_eq!(action.unwrap().id, action_id);
}

#[test]
fn test_get_action_not_found() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    let action_id = "non.existent.action";
    let action = core.get_action(action_id).unwrap();
    assert!(action.is_none(), "Should not find action {}", action_id);
}
