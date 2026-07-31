use super::action::*;
use crate::action_registry::{ActionKind, ActionRiskLevel, ActionDescriptor, ActionResult};

#[test]
fn test_action_kind_dto_roundtrip() {
    let kinds = vec![ActionKind::Query, ActionKind::Preview, ActionKind::Mutation];
    for kind in kinds {
        let dto: ActionKindDto = kind.clone().into();
        let back: ActionKind = dto.into();
        assert_eq!(kind, back);
    }
}

#[test]
fn test_action_risk_level_dto_roundtrip() {
    let levels = vec![
        ActionRiskLevel::SafeRead,
        ActionRiskLevel::SafeWrite,
        ActionRiskLevel::ContentWrite,
        ActionRiskLevel::Dangerous,
    ];
    for level in levels {
        let dto: ActionRiskLevelDto = level.clone().into();
        let back: ActionRiskLevel = dto.into();
        assert_eq!(level, back);
    }
}

#[test]
fn test_action_descriptor_dto_roundtrip() {
    let desc = ActionDescriptor {
        id: "test_action".to_string(),
        title: "Test Action".to_string(),
        description: "A test action".to_string(),
        category: "Test".to_string(),
        kind: ActionKind::Mutation,
        risk_level: ActionRiskLevel::SafeWrite,
        confirm_required: true,
        undoable: false,
        platforms: vec!["windows".to_string(), "linux".to_string()],
        input_schema: Some(serde_json::json!({"type": "string"})),
        ui_schema: None,
    };
    let dto: ActionDescriptorDto = desc.clone().into();
    let back: ActionDescriptor = dto.into();
    assert_eq!(desc.id, back.id);
    assert_eq!(desc.title, back.title);
    assert_eq!(desc.input_schema, back.input_schema);
    assert_eq!(desc.ui_schema, back.ui_schema);
}

#[test]
fn test_action_result_dto_roundtrip() {
    let res = ActionResult {
        success: true,
        message: Some("Action succeeded".to_string()),
        data: Some(serde_json::json!({"id": 123})),
        proposed_ui: None,
        requires_confirmation: Some(false),
    };
    let dto: ActionResultDto = res.clone().into();
    let back: ActionResult = dto.into();
    assert_eq!(res.success, back.success);
    assert_eq!(res.message, back.message);
    assert_eq!(res.data, back.data);
    assert_eq!(res.proposed_ui, back.proposed_ui);
    assert_eq!(res.requires_confirmation, back.requires_confirmation);
}
