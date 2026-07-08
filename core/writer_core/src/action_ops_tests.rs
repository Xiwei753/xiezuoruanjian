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

#[test]
fn test_execute_action_font_size_get() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    let result = core.execute_action("settings.editor.font_size.get", "", "").unwrap();
    assert!(result.success);
    assert!(result.data.is_some());
    let data = result.data.unwrap();
    assert!(data.get("fontSize").is_some());
}

#[test]
fn test_execute_action_font_size_set_valid() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    let result = core
        .execute_action("settings.editor.font_size.set", r#"{"fontSize": 18.0}"#, "")
        .unwrap();
    assert!(result.success);
}

#[test]
fn test_execute_action_font_size_set_out_of_range() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    let result = core
        .execute_action("settings.editor.font_size.set", r#"{"fontSize": 5.0}"#, "")
        .unwrap();
    assert!(!result.success);
}

#[test]
fn test_execute_action_font_size_set_missing_param() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    let result = core
        .execute_action("settings.editor.font_size.set", "{}", "")
        .unwrap();
    assert!(!result.success);
}

#[test]
fn test_execute_action_not_implemented() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    let result = core.execute_action("some.unknown.action.id", "", "");
    assert!(result.is_err(), "Unknown action should return error");
}

#[test]
fn test_execute_action_invalid_args_json() {
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    let result = core
        .execute_action("settings.editor.font_size.set", "not json", "")
        .unwrap();
    assert!(!result.success);
}
