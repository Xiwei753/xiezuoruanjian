//! # 动作注册表模块 (Action Registry)
//!
//! 本模块实现了应用程序的动作注册和管理系统，用于统一管理所有可执行的操作。
//!
//! ## 主要功能
//!
//! - **动作定义管理**: 定义和注册各种可执行的动作（如设置修改、同步操作等）
//! - **动作分类**: 按类别（settings、sync 等）组织动作
//! - **风险等级标识**: 每个动作都有明确的风险等级（SafeRead、SafeWrite、ContentWrite、Dangerous）
//! - **输入验证**: 通过 JSON Schema 定义动作的输入参数格式
//! - **UI 集成**: 提供 UI Schema 用于自动生成操作界面
//! - **平台适配**: 标记动作支持的平台（android、linux）
//!
//! ## 动作类型
//!
//! - `Query`: 查询类操作，只读不修改
//! - `Preview`: 预览类操作，不产生实际变更
//! - `Mutation`: 修改类操作，会产生实际变更
//!
//! ## 依赖关系
//!
//! - `serde` / `serde_json`: 序列化/反序列化
//! - `crate::facade::WriterCore`: 核心功能实现
//!
//! ## 使用场景
//!
//! - AI 助手调用应用功能
//! - 自动化脚本执行
//! - 统一的操作接口

use crate::platform_interaction::PlatformKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 动态动作提供者 trait
///
/// 实现此 trait 可向 ActionRegistry 注册自定义动作。
/// 未来可用于插件系统、WASM 扩展等场景。
pub trait ActionProvider: Send + Sync {
    /// 提供者名称（用于调试和日志）
    fn name(&self) -> &str;

    /// 返回此提供者注册的动作描述列表
    fn list_actions(&self) -> Vec<ActionDescriptor>;

    /// 执行此提供者注册的动作
    ///
    /// 如果 action_id 不属于此提供者，应返回 `Ok(None)` 交由下一个提供者处理。
    fn execute(&self, action_id: &str, args_json: &str, context: &str) -> Option<ActionResult>;
}

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
    providers: Vec<Box<dyn ActionProvider>>,
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
            providers: Vec::new(),
        };
        registry.register_v1_actions();
        registry
    }

    /// 注册动态动作提供者
    pub fn register_provider(&mut self, provider: Box<dyn ActionProvider>) {
        self.actions.extend(provider.list_actions());
        self.providers.push(provider);
    }

    fn register_v1_actions(&mut self) {
        self.register_editor_settings_actions();
        self.register_sync_actions();
    }

    fn register_editor_settings_actions(&mut self) {
        self.register_font_size_actions();
        self.register_auto_save_actions();
        self.register_line_spacing_actions();
        self.register_auto_indent_actions();
        self.register_typing_animation_actions();
        self.register_smooth_cursor_actions();
    }

    fn register_font_size_actions(&mut self) {
        self.actions.push(ActionDescriptor {
            id: "settings.editor.font_size.get".to_string(),
            title: "获取字号".to_string(),
            description: "获取当前编辑器字号".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
    }

    fn register_auto_save_actions(&mut self) {
        self.actions.push(ActionDescriptor {
            id: "settings.editor.auto_save.get".to_string(),
            title: "获取自动保存状态".to_string(),
            description: "获取自动保存是否开启".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
    }

    fn register_line_spacing_actions(&mut self) {
        self.actions.push(ActionDescriptor {
            id: "settings.editor.line_spacing.get".to_string(),
            title: "获取行距".to_string(),
            description: "获取当前编辑器行距倍数".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
    }

    fn register_auto_indent_actions(&mut self) {
        self.actions.push(ActionDescriptor {
            id: "settings.editor.auto_indent.get".to_string(),
            title: "获取自动缩进".to_string(),
            description: "获取自动缩进是否开启".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
    }

    fn register_typing_animation_actions(&mut self) {
        self.actions.push(ActionDescriptor {
            id: "settings.editor.typing_animation.get".to_string(),
            title: "获取输入动画".to_string(),
            description: "获取输入动画是否开启".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean" },
                    "durationMs": { "type": "integer", "minimum": 0, "maximum": 1000 }
                },
                "required": ["enabled"]
            })),
            ui_schema: Some(serde_json::json!({
                "type": "switch"
            })),
        });
    }

    fn register_smooth_cursor_actions(&mut self) {
        self.actions.push(ActionDescriptor {
            id: "settings.editor.smooth_cursor.get".to_string(),
            title: "获取平滑光标".to_string(),
            description: "获取平滑光标是否开启".to_string(),
            category: "settings".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean" },
                    "durationMs": { "type": "integer", "minimum": 0, "maximum": 1000 }
                },
                "required": ["enabled"]
            })),
            ui_schema: Some(serde_json::json!({
                "type": "switch"
            })),
        });
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting
    )]
    fn register_sync_actions(&mut self) {
        self.actions.push(ActionDescriptor {
            id: "settings.sync.config.get".to_string(),
            title: "获取同步配置".to_string(),
            description: "获取当前同步配置".to_string(),
            category: "sync".to_string(),
            kind: ActionKind::Query,
            risk_level: ActionRiskLevel::SafeRead,
            confirm_required: false,
            undoable: false,
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
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
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
            input_schema: None,
            ui_schema: None,
        });
        self.actions.push(ActionDescriptor {
            id: "sync.conflict.resolve_keep_local".to_string(),
            title: "冲突保留本地".to_string(),
            description: "解决同步冲突：保留本地版本，上传到远端".to_string(),
            category: "sync".to_string(),
            kind: ActionKind::Mutation,
            risk_level: ActionRiskLevel::ContentWrite,
            confirm_required: true,
            undoable: false,
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "冲突文件的作品目录相对路径" }
                },
                "required": ["path"]
            })),
            ui_schema: None,
        });
        self.actions.push(ActionDescriptor {
            id: "sync.conflict.resolve_take_remote".to_string(),
            title: "冲突采用远端".to_string(),
            description: "解决同步冲突：采用远端版本，覆盖本地".to_string(),
            category: "sync".to_string(),
            kind: ActionKind::Mutation,
            risk_level: ActionRiskLevel::ContentWrite,
            confirm_required: true,
            undoable: false,
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "冲突文件的作品目录相对路径" }
                },
                "required": ["path"]
            })),
            ui_schema: None,
        });
        self.actions.push(ActionDescriptor {
            id: "sync.conflict.resolve_mark_merged".to_string(),
            title: "冲突标记已合并".to_string(),
            description: "解决同步冲突：用户已手动合并，上传当前本地版本".to_string(),
            category: "sync".to_string(),
            kind: ActionKind::Mutation,
            risk_level: ActionRiskLevel::ContentWrite,
            confirm_required: true,
            undoable: false,
            platforms: vec![
                PlatformKind::Desktop.to_string(),
                PlatformKind::Android.to_string(),
            ],
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "冲突文件的作品目录相对路径" }
                },
                "required": ["path"]
            })),
            ui_schema: None,
        });
    }

    pub fn list_registered_actions(&self) -> Vec<ActionDescriptor> {
        self.actions.clone()
    }

    pub fn get_action(&self, action_id: &str) -> Option<ActionDescriptor> {
        self.actions.iter().find(|a| a.id == action_id).cloned()
    }

    /// 尝试通过已注册的动态提供者执行动作
    ///
    /// 如果没有提供者能处理此 action_id，返回 None。
    pub fn try_execute_via_providers(
        &self,
        action_id: &str,
        args_json: &str,
        context: &str,
    ) -> Option<ActionResult> {
        for provider in &self.providers {
            if let Some(result) = provider.execute(action_id, args_json, context) {
                return Some(result);
            }
        }
        None
    }

    /// 已注册的动态提供者名称列表（调试用）
    pub fn provider_names(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.name()).collect()
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
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

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
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        assert!(core.execute_action("unknown.action", "", "").is_err());
    }

    #[test]
    fn test_font_size_reads_syncable_settings() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

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
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

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
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

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
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

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
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

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
    let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
    std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

    let result = core
        .execute_action("settings.editor.font_size.set", "{invalid json}", "")
        .unwrap();
    assert!(!result.success);
    assert_eq!(result.message.unwrap(), "invalid args json");
}

#[test]
fn test_action_provider_registration() {
    struct MockProvider;
    impl ActionProvider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }
        fn list_actions(&self) -> Vec<ActionDescriptor> {
            vec![ActionDescriptor {
                id: "mock.test.action".to_string(),
                title: "Test Action".to_string(),
                description: "A test action".to_string(),
                category: "test".to_string(),
                kind: ActionKind::Query,
                risk_level: ActionRiskLevel::SafeRead,
                confirm_required: false,
                undoable: false,
                platforms: vec![PlatformKind::Desktop.to_string()],
                input_schema: None,
                ui_schema: None,
            }]
        }
        fn execute(&self, action_id: &str, _args: &str, _ctx: &str) -> Option<ActionResult> {
            if action_id == "mock.test.action" {
                Some(ActionResult {
                    success: true,
                    message: Some("mock executed".to_string()),
                    data: None,
                    proposed_ui: None,
                    requires_confirmation: None,
                })
            } else {
                None
            }
        }
    }

    let mut registry = ActionRegistry::new();
    let base_count = registry.list_registered_actions().len();

    registry.register_provider(Box::new(MockProvider));
    let actions = registry.list_registered_actions();
    assert_eq!(actions.len(), base_count + 1);
    assert!(actions.iter().any(|a| a.id == "mock.test.action"));
    assert_eq!(registry.provider_names(), vec!["mock"]);

    let result = registry.try_execute_via_providers("mock.test.action", "", "");
    assert!(result.is_some());
    assert!(result.unwrap().success);

    let none_result = registry.try_execute_via_providers("unknown.action", "", "");
    assert!(none_result.is_none());
}
