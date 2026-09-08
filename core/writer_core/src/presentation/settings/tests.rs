//! #628：设置页展示契约测试（从 settings_presentation.rs 迁移）。

use super::*;

#[test]
fn test_default_settings_presentation_not_empty() {
    let presentation = default_settings_presentation();
    assert!(
        !presentation.sections.is_empty(),
        "presentation must have sections"
    );
    for section in &presentation.sections {
        assert!(!section.id.is_empty(), "section id must not be empty");
    }
}

#[test]
fn test_serialization_roundtrip() {
    let presentation = default_settings_presentation();
    let json = serde_json::to_string(&presentation).expect("serialization must succeed");
    let deserialized: SettingsPresentation =
        serde_json::from_str(&json).expect("deserialization must succeed");
    assert_eq!(presentation.sections.len(), deserialized.sections.len());
}

#[test]
fn test_all_items_have_valid_ids() {
    let presentation = default_settings_presentation();
    let mut seen_ids = std::collections::HashSet::new();

    for section in &presentation.sections {
        assert!(!section.id.is_empty(), "section id must not be empty");

        for item in &section.items {
            assert!(
                !item.id.is_empty(),
                "item id in section '{}' must not be empty",
                section.id
            );
            assert!(
                seen_ids.insert(item.id.clone()),
                "duplicate item id: '{}'",
                item.id
            );
        }
    }
}

#[test]
fn test_settings_presentation_json_ffi() {
    let presentation = default_settings_presentation();
    let json = serde_json::to_string(&presentation).expect("serialization must succeed");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must be valid JSON");

    assert!(parsed.get("sections").is_some());
    let sections = parsed.get("sections").unwrap().as_array().unwrap();
    assert!(!sections.is_empty());

    let first = &sections[0];
    assert!(first.get("id").is_some());
    assert!(!first.get("items").unwrap().as_array().unwrap().is_empty());
}

#[test]
fn test_section_order_matches_product_design() {
    // #628：section 顺序为 appearance(10), editor(20), save(30), sync(40),
    // ai(50), stats(60, 空), about(70)。
    let presentation = default_settings_presentation();
    let orders: Vec<u32> = presentation.sections.iter().map(|s| s.order).collect();
    assert_eq!(orders, vec![10, 20, 30, 40, 50, 60, 70]);
    let ids: Vec<&str> = presentation
        .sections
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec![
            "appearance",
            "editor",
            "save",
            "sync",
            "ai",
            "stats",
            "about"
        ]
    );
}

#[test]
fn test_stats_section_is_empty() {
    // #628：stats 是空 section，直接在 settings.rs 组装。
    let presentation = default_settings_presentation();
    let stats = presentation
        .sections
        .iter()
        .find(|s| s.id == "stats")
        .expect("stats section must exist");
    assert!(stats.items.is_empty());
}
