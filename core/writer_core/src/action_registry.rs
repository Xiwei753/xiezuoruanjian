use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ActionKind {
    Query,
    Preview,
    Mutation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ActionRiskLevel {
    SafeRead,
    SafeWrite,
    ContentWrite,
    Dangerous,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionDescriptor {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub kind: ActionKind,
    pub risk_level: ActionRiskLevel,
    pub confirm_required: bool,
    pub undoable: bool,
    pub platforms: Vec<String>,
    pub input_schema: Option<Value>,
    pub ui_schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub success: bool,
    pub message: Option<String>,
    pub data: Option<Value>,
    pub proposed_ui: Option<Value>,
    pub requires_confirmation: Option<bool>,
}

pub struct ActionRegistry {
    actions: Vec<ActionDescriptor>,
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            actions: Vec::new(),
        };
        registry.register_v1_actions();
        registry
    }

    fn register_v1_actions(&mut self) {
        self.actions.push(ActionDescriptor {
            id: "settings.editor.font_size.get".to_string(),
            title: "获取字号".to_string(),
            description: "获取当前编辑器字号".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: None,
            ui_schema: None,
        });

        self.actions.push(ActionDescriptor {
            id: "settings.editor.font_size.set".to_string(),
            title: "设置字号".to_string(),
            description: "设置编辑器字号（10-72）".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Mutation,
            risk_level: ActionRiskLevel::SafeWrite,
            confirm_required: true,
            undoable: true,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "fontSize": { "type": "number", "minimum": 10, "maximum": 72 }
                },
                "required": ["fontSize"]
            })),
            ui_schema: Some(serde_json::json!({
                "type": "slider",
                "min": 10,
                "max": 72,
                "step": 1
            })),
        });

        self.actions.push(ActionDescriptor {
            id: "settings.editor.auto_save.get".to_string(),
            title: "获取自动保存状态".to_string(),
            description: "获取自动保存是否开启".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: None,
            ui_schema: None,
        });

        self.actions.push(ActionDescriptor {
            id: "settings.editor.auto_save.set".to_string(),
            title: "设置自动保存".to_string(),
            description: "开启或关闭自动保存".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Mutation,
            risk_level: ActionRiskLevel::SafeWrite,
            confirm_required: true,
            undoable: true,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean" }
                },
                "required": ["enabled"]
            })),
            ui_schema: Some(serde_json::json!({
                "type": "switch"
            })),
        });

        self.actions.push(ActionDescriptor {
            id: "settings.editor.auto_save_delay.set".to_string(),
            title: "设置自动保存延迟".to_string(),
            description: "设置自动保存的延迟时间（毫秒，500-10000）".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Mutation,
            risk_level: ActionRiskLevel::SafeWrite,
            confirm_required: true,
            undoable: true,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "delayMs": { "type": "integer", "minimum": 500, "maximum": 10000 }
                },
                "required": ["delayMs"]
            })),
            ui_schema: Some(serde_json::json!({
                "type": "slider",
                "min": 500,
                "max": 10000,
                "step": 500
            })),
        });

        self.actions.push(ActionDescriptor {
            id: "settings.sync.config.get".to_string(),
            title: "获取同步配置".to_string(),
            description: "获取当前同步配置".to_string(),
            category: "sync".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: None,
            ui_schema: None,
        });

        self.actions.push(ActionDescriptor {
            id: "sync.diagnostics.run".to_string(),
            title: "运行同步诊断".to_string(),
            description: "检查同步连接状态，不会实际同步数据".to_string(),
            category: "sync".to_string(),
            kind: ActionKind::Preview,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: None,
            ui_schema: None,
        });

        self.actions.push(ActionDescriptor {
            id: "sync.plan.preview".to_string(),
            title: "预览同步计划".to_string(),
            description: "计算将要同步的文件变更，不会实际同步".to_string(),
            category: "sync".to_string(),
            kind: ActionKind::Preview,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: None,
            ui_schema: None,
        });

        self.actions.push(ActionDescriptor {
            id: "settings.editor.line_spacing.get".to_string(),
            title: "获取行距".to_string(),
            description: "获取当前编辑器行距倍数".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: None,
            ui_schema: None,
        });

        self.actions.push(ActionDescriptor {
            id: "settings.editor.line_spacing.set".to_string(),
            title: "设置行距".to_string(),
            description: "设置编辑器行距倍数（1.0-3.0）".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Mutation,
            risk_level: ActionRiskLevel::SafeWrite,
            confirm_required: true,
            undoable: true,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "multiplier": { "type": "number", "minimum": 1.0, "maximum": 3.0 }
                },
                "required": ["multiplier"]
            })),
            ui_schema: Some(serde_json::json!({
                "type": "slider",
                "min": 100,
                "max": 300,
                "step": 10,
                "displayScale": 0.01
            })),
        });

        self.actions.push(ActionDescriptor {
            id: "settings.editor.auto_indent.get".to_string(),
            title: "获取自动缩进".to_string(),
            description: "获取自动缩进是否开启".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: None,
            ui_schema: None,
        });

        self.actions.push(ActionDescriptor {
            id: "settings.editor.auto_indent.set".to_string(),
            title: "设置自动缩进".to_string(),
            description: "开启或关闭自动缩进".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Mutation,
            risk_level: ActionRiskLevel::SafeWrite,
            confirm_required: true,
            undoable: true,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean" },
                    "widthChars": { "type": "number", "minimum": 0.0, "maximum": 8.0 }
                },
                "required": ["enabled"]
            })),
            ui_schema: Some(serde_json::json!({
                "type": "switch"
            })),
        });

        self.actions.push(ActionDescriptor {
            id: "settings.editor.typing_animation.get".to_string(),
            title: "获取输入动画".to_string(),
            description: "获取输入动画是否开启".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: None,
            ui_schema: None,
        });

        self.actions.push(ActionDescriptor {
            id: "settings.editor.typing_animation.set".to_string(),
            title: "设置输入动画".to_string(),
            description: "开启或关闭输入动画".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Mutation,
            risk_level: ActionRiskLevel::SafeWrite,
            confirm_required: true,
            undoable: true,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean" },
                    "durationMs": { "type": "integer", "minimum": 0, "maximum": 500 }
                },
                "required": ["enabled"]
            })),
            ui_schema: Some(serde_json::json!({
                "type": "switch"
            })),
        });

        self.actions.push(ActionDescriptor {
            id: "settings.editor.smooth_cursor.get".to_string(),
            title: "获取平滑光标".to_string(),
            description: "获取平滑光标是否开启".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: None,
            ui_schema: None,
        });

        self.actions.push(ActionDescriptor {
            id: "settings.editor.smooth_cursor.set".to_string(),
            title: "设置平滑光标".to_string(),
            description: "开启或关闭平滑光标".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Mutation,
            risk_level: ActionRiskLevel::SafeWrite,
            confirm_required: true,
            undoable: true,
            platforms: vec!["android".to_string(), "linux".to_string()],
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean" },
                    "durationMs": { "type": "integer", "minimum": 0, "maximum": 500 }
                },
                "required": ["enabled"]
            })),
            ui_schema: Some(serde_json::json!({
                "type": "switch"
            })),
        });
    }

    pub fn list_registered_actions(&self) -> Vec<ActionDescriptor> {
        self.actions.clone()
    }

    pub fn get_action(&self, action_id: &str) -> Option<ActionDescriptor> {
        self.actions.iter().find(|a| a.id == action_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facade::WriterCore;
    use tempfile::tempdir;

    #[test]
    fn test_list_actions() {
        let registry = ActionRegistry::new();
        let actions = registry.list_registered_actions();
        assert!(!actions.is_empty());
        assert!(actions
            .iter()
            .any(|a| a.id == "settings.editor.font_size.set"));
    }

    #[test]
    fn test_execute_font_size() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let result = core
            .execute_action("settings.editor.font_size.set", r#"{"fontSize": 20.0}"#, "")
            .unwrap();
        assert!(result.success);

        let get_result = core
            .execute_action("settings.editor.font_size.get", "", "")
            .unwrap();
        assert!(get_result.success);
        let data = get_result.data.unwrap();
        let val = data.get("fontSize").unwrap().as_f64().unwrap();
        assert_eq!(val, 20.0);
        let source = data.get("source").unwrap().as_str().unwrap();
        assert_eq!(source, "syncable");

        let fail_result = core
            .execute_action(
                "settings.editor.font_size.set",
                r#"{"fontSize": 100.0}"#,
                "",
            )
            .unwrap();
        assert!(!fail_result.success);
    }

    #[test]
    fn test_execute_unknown_action() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        assert!(core.execute_action("unknown.action", "", "").is_err());
    }

    #[test]
    fn test_font_size_reads_syncable_settings() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let mut syncable = core.load_syncable_settings().unwrap();
        syncable.font_size = 24.0;
        core.save_syncable_settings(&syncable).unwrap();

        let result = core
            .execute_action("settings.editor.font_size.get", "", "")
            .unwrap();
        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data.get("fontSize").unwrap().as_f64().unwrap(), 24.0);
        assert_eq!(data.get("source").unwrap().as_str().unwrap(), "syncable");
    }

    #[test]
    fn test_font_size_fallback_to_local_when_syncable_zero() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let mut local = core.load_local_settings().unwrap();
        local.editor_font_size = 18.0;
        core.save_local_settings(&local).unwrap();

        let result = core
            .execute_action("settings.editor.font_size.get", "", "")
            .unwrap();
        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data.get("fontSize").unwrap().as_f64().unwrap(), 18.0);
        assert_eq!(data.get("source").unwrap().as_str().unwrap(), "syncable");
    }

    #[test]
    fn test_font_size_set_writes_syncable_settings() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        core.execute_action("settings.editor.font_size.set", r#"{"fontSize": 22.0}"#, "")
            .unwrap();

        let syncable = core.load_syncable_settings().unwrap();
        assert_eq!(syncable.font_size, 22.0);

        let local = core.load_local_settings().unwrap();
        assert_eq!(local.editor_font_size, 16.0);
    }

    #[test]
    fn test_font_size_set_does_not_overwrite_local() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let mut local = core.load_local_settings().unwrap();
        local.editor_font_size = 14.0;
        core.save_local_settings(&local).unwrap();

        core.execute_action("settings.editor.font_size.set", r#"{"fontSize": 28.0}"#, "")
            .unwrap();

        let local_after = core.load_local_settings().unwrap();
        assert_eq!(local_after.editor_font_size, 14.0);

        let syncable = core.load_syncable_settings().unwrap();
        assert_eq!(syncable.font_size, 28.0);
    }

    #[test]
    fn test_font_size_set_returns_source_syncable() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let result = core
            .execute_action("settings.editor.font_size.set", r#"{"fontSize": 16.0}"#, "")
            .unwrap();
        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data.get("source").unwrap().as_str().unwrap(), "syncable");
        assert_eq!(data.get("fontSize").unwrap().as_f64().unwrap(), 16.0);
    }
}

#[test]
fn test_execute_invalid_json() {
    use crate::facade::WriterCore;
    use tempfile::tempdir;
    let temp_dir = tempdir().unwrap();
    let core = WriterCore::new(temp_dir.path());
    core.create_workspace().unwrap();

    let result = core
        .execute_action("settings.editor.font_size.set", "{invalid json}", "")
        .unwrap();
    assert!(!result.success);
    assert_eq!(result.message.unwrap(), "invalid args json");
}
