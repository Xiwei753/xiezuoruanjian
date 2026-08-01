use super::platform::*;
use serde_json::json;

#[test]
fn test_platform_dto_serialization_roundtrip() {
    let platform = PlatformDto::Android;
    let json_val = serde_json::to_value(&platform).unwrap();
    assert_eq!(json_val, json!("Android"));

    let deserialized: PlatformDto = serde_json::from_value(json_val).unwrap();
    assert_eq!(deserialized, platform);
}

#[test]
fn test_avoid_region_dto_serialization_roundtrip() {
    let avoid_region = AvoidRegionDto {
        left_dp: 10.0,
        top_dp: 20.0,
        right_dp: 30.0,
        bottom_dp: 40.0,
        kind: AvoidRegionKindDto::WindowInset,
    };

    let json_val = serde_json::to_value(&avoid_region).unwrap();
    assert_eq!(
        json_val,
        json!({
            "leftDp": 10.0,
            "topDp": 20.0,
            "rightDp": 30.0,
            "bottomDp": 40.0,
            "kind": "windowInset"
        })
    );

    let deserialized: AvoidRegionDto = serde_json::from_value(json_val).unwrap();
    assert_eq!(deserialized, avoid_region);
}

#[test]
fn test_fold_state_dto_serialization_roundtrip() {
    let state = FoldStateDto::HalfOpened;
    let json_val = serde_json::to_value(&state).unwrap();
    assert_eq!(json_val, json!("HalfOpened"));

    let deserialized: FoldStateDto = serde_json::from_value(json_val).unwrap();
    assert_eq!(deserialized, state);
}
