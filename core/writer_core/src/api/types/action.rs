//! # 动作注册 DTO — ActionRegistry 的跨语言边界类型
//!
//! 动作分类语义：
//! - Query：只读查询，不修改任何状态
//! - Preview：预览变更（如查找替换预览），不持久化
//! - Mutation：实际修改状态，需确认
//!
//! 风险等级：
//! - SafeRead：无副作用
//! - SafeWrite：修改非正文状态（设置、UI 偏好）
//! - ContentWrite：修改正文内容，需 undo 支持
//! - Dangerous：不可逆操作（删除项目），需二次确认

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]

pub enum ActionKindDto {
    Query,
    Preview,
    Mutation,
}

impl From<crate::action_registry::ActionKind> for ActionKindDto {
    fn from(k: crate::action_registry::ActionKind) -> Self {
        match k {
            crate::action_registry::ActionKind::Query => Self::Query,
            crate::action_registry::ActionKind::Preview => Self::Preview,
            crate::action_registry::ActionKind::Mutation => Self::Mutation,
        }
    }
}

impl From<ActionKindDto> for crate::action_registry::ActionKind {
    fn from(dto: ActionKindDto) -> Self {
        match dto {
            ActionKindDto::Query => Self::Query,
            ActionKindDto::Preview => Self::Preview,
            ActionKindDto::Mutation => Self::Mutation,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]

pub enum ActionRiskLevelDto {
    SafeRead,
    SafeWrite,
    ContentWrite,
    Dangerous,
}

impl From<crate::action_registry::ActionRiskLevel> for ActionRiskLevelDto {
    fn from(r: crate::action_registry::ActionRiskLevel) -> Self {
        match r {
            crate::action_registry::ActionRiskLevel::SafeRead => Self::SafeRead,
            crate::action_registry::ActionRiskLevel::SafeWrite => Self::SafeWrite,
            crate::action_registry::ActionRiskLevel::ContentWrite => Self::ContentWrite,
            crate::action_registry::ActionRiskLevel::Dangerous => Self::Dangerous,
        }
    }
}

impl From<ActionRiskLevelDto> for crate::action_registry::ActionRiskLevel {
    fn from(dto: ActionRiskLevelDto) -> Self {
        match dto {
            ActionRiskLevelDto::SafeRead => Self::SafeRead,
            ActionRiskLevelDto::SafeWrite => Self::SafeWrite,
            ActionRiskLevelDto::ContentWrite => Self::ContentWrite,
            ActionRiskLevelDto::Dangerous => Self::Dangerous,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionDescriptorDto {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub kind: ActionKindDto,
    pub risk_level: ActionRiskLevelDto,
    pub confirm_required: bool,
    pub undoable: bool,
    pub platforms: Vec<String>,
    pub input_schema: Option<String>,
    pub ui_schema: Option<String>,
}

impl From<crate::action_registry::ActionDescriptor> for ActionDescriptorDto {
    fn from(d: crate::action_registry::ActionDescriptor) -> Self {
        Self {
            id: d.id,
            title: d.title,
            description: d.description,
            category: d.category,
            kind: d.kind.into(),
            risk_level: d.risk_level.into(),
            confirm_required: d.confirm_required,
            undoable: d.undoable,
            platforms: d.platforms,
            input_schema: d
                .input_schema
                .map(|v| serde_json::to_string(&v).unwrap_or_default()),
            ui_schema: d
                .ui_schema
                .map(|v| serde_json::to_string(&v).unwrap_or_default()),
        }
    }
}

impl From<ActionDescriptorDto> for crate::action_registry::ActionDescriptor {
    fn from(dto: ActionDescriptorDto) -> Self {
        Self {
            id: dto.id,
            title: dto.title,
            description: dto.description,
            category: dto.category,
            kind: dto.kind.into(),
            risk_level: dto.risk_level.into(),
            confirm_required: dto.confirm_required,
            undoable: dto.undoable,
            platforms: dto.platforms,
            input_schema: dto
                .input_schema
                .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            ui_schema: dto
                .ui_schema
                .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionResultDto {
    pub success: bool,
    pub message: Option<String>,
    pub data: Option<String>,
    pub proposed_ui: Option<String>,
    pub requires_confirmation: Option<bool>,
}

impl From<crate::action_registry::ActionResult> for ActionResultDto {
    fn from(r: crate::action_registry::ActionResult) -> Self {
        Self {
            success: r.success,
            message: r.message,
            data: r
                .data
                .map(|v| serde_json::to_string(&v).unwrap_or_default()),
            proposed_ui: r
                .proposed_ui
                .map(|v| serde_json::to_string(&v).unwrap_or_default()),
            requires_confirmation: r.requires_confirmation,
        }
    }
}

impl From<ActionResultDto> for crate::action_registry::ActionResult {
    fn from(dto: ActionResultDto) -> Self {
        Self {
            success: dto.success,
            message: dto.message,
            data: dto
                .data
                .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            proposed_ui: dto
                .proposed_ui
                .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            requires_confirmation: dto.requires_confirmation,
        }
    }
}
