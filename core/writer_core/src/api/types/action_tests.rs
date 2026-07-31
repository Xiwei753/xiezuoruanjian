use super::action::*;
use crate::action_registry::{ActionKind, ActionRiskLevel, ActionDescriptor, ActionResult};
use serde_json::json;

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
    assert_eq!(desc.description, back.description);
    assert_eq!(desc.category, back.category);
    assert_eq!(desc.kind, back.kind);
    assert_eq!(desc.risk_level, back.risk_level);
    assert_eq!(desc.confirm_required, back.confirm_required);
    assert_eq!(desc.undoable, back.undoable);
    assert_eq!(desc.platforms, back.platforms);
    assert_eq!(desc.input_schema, back.input_schema);
    assert_eq!(desc.ui_schema, back.ui_schema);
}

#[test]
fn test_action_descriptor_dto_json_key_contract() {
    let dto = ActionDescriptorDto {
        id: "a1".to_string(),
        title: "T".to_string(),
        description: "D".to_string(),
        category: "C".to_string(),
        kind: ActionKindDto::Query,
        risk_level: ActionRiskLevelDto::SafeRead,
        confirm_required: false,
        undoable: true,
        platforms: vec!["android".to_string()],
        input_schema: None,
        ui_schema: None,
    };
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(json["id"], "a1");
    assert_eq!(json["title"], "T");
    assert_eq!(json["description"], "D");
    assert_eq!(json["category"], "C");
    assert_eq!(json["kind"], "Query");
    assert_eq!(json["riskLevel"], "SafeRead");
    assert_eq!(json["confirmRequired"], false);
    assert_eq!(json["undoable"], true);
    assert_eq!(json["platforms"][0], "android");
    assert!(json["inputSchema"].is_null());
    assert!(json["uiSchema"].is_null());
    let as_object = json.as_object().unwrap();
    assert_eq!(as_object.len(), 11, "ActionDescriptorDto must have exactly 11 JSON keys");
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

#[test]
fn test_action_result_dto_json_contract() {
    let dto = ActionResultDto {
        success: true,
        message: Some("Action succeeded".to_string()),
        data: Some("{\"id\":123}".to_string()),
        proposed_ui: None,
        requires_confirmation: Some(false),
    };
    let json_val = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        json_val,
        json!({
            "success": true,
            "message": "Action succeeded",
            "data": "{\"id\":123}",
            "proposedUi": null,
            "requiresConfirmation": false
        })
    );
    let deserialized: ActionResultDto = serde_json::from_value(json_val).unwrap();
    assert_eq!(dto, deserialized);

    let minimal = ActionResultDto {
        success: false,
        message: None,
        data: None,
        proposed_ui: None,
        requires_confirmation: None,
    };
    let min_json = serde_json::to_value(&minimal).unwrap();
    assert_eq!(
        min_json,
        json!({
            "success": false,
            "message": null,
            "data": null,
            "proposedUi": null,
            "requiresConfirmation": null
        })
    );
    let min_back: ActionResultDto = serde_json::from_value(min_json).unwrap();
    assert_eq!(minimal, min_back);
}
