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
        assert!(actions.iter().any(|a| a.id == "settings.editor.font_size.set"));
    }

    #[test]
    fn test_execute_font_size() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let result = core.execute_action("settings.editor.font_size.set", r#"{"fontSize": 20.0}"#, "").unwrap();
        assert!(result.success);

        let get_result = core.execute_action("settings.editor.font_size.get", "", "").unwrap();
        assert!(get_result.success);
        let val = get_result.data.unwrap().get("fontSize").unwrap().as_f64().unwrap();
        assert_eq!(val, 20.0);

        let fail_result = core.execute_action("settings.editor.font_size.set", r#"{"fontSize": 100.0}"#, "").unwrap();
        assert!(!fail_result.success);
    }

    #[test]
    fn test_execute_unknown_action() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        assert!(core.execute_action("unknown.action", "", "").is_err());
    }
}
