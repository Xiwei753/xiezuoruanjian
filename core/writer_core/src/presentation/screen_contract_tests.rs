//! #610：页面契约纯函数测试（从 screen_contract.rs 拆出，避免生产文件测试膨胀）。
//!
//! 覆盖：页面角色/面板角色/动作角色/动作区域枚举、各页面动作槽位表、
//! 产品顺序（#597）、共享契约不含平台控件名、序列化往返。
//! 纯函数测试，不触碰 UI / 平台 API / 文件系统。

use super::screen_contract::*;

#[test]
fn test_screen_role_variants() {
    let variants = [
        ScreenRole::Home,
        ScreenRole::ProjectList,
        ScreenRole::ProjectWorkspace,
        ScreenRole::Writing,
        ScreenRole::StarMap,
        ScreenRole::Stats,
        ScreenRole::Settings,
        ScreenRole::Sync,
    ];
    assert_eq!(variants.len(), 8);
}

#[test]
fn test_pane_role_variants() {
    let variants = [
        PaneRole::PrimaryList,
        PaneRole::Detail,
        PaneRole::Editor,
        PaneRole::Inspector,
        PaneRole::Drawer,
        PaneRole::Supporting,
    ];
    assert_eq!(variants.len(), 6);
}

#[test]
fn test_action_role_variants() {
    let variants = vec![
        ActionRole::Back,
        ActionRole::Save,
        ActionRole::CreateProject,
        ActionRole::CreateVolume,
        ActionRole::CreateChapter,
        ActionRole::Delete,
        ActionRole::Rename,
        ActionRole::Settings,
        ActionRole::Sync,
        ActionRole::Search,
        ActionRole::Sort,
    ];
    assert_eq!(variants.len(), 11);
}

#[test]
fn test_action_region_variants() {
    let variants = [
        ActionRegion::HeaderLeading,
        ActionRegion::HeaderTrailing,
        ActionRegion::ListHeader,
        ActionRegion::ItemTrailing,
        ActionRegion::Context,
        ActionRegion::EmptyState,
    ];
    assert_eq!(variants.len(), 6);
}

#[test]
fn test_contract_has_no_platform_widget_names() {
    // #610：共享契约不得出现平台控件名。
    let all_json =
        serde_json::to_string(&resolve_screen_policy(ScreenRole::ProjectWorkspace)).unwrap();
    for platform_name in [
        "BottomBar",
        "NavigationRail",
        "PermanentDrawer",
        "Floating",
        "SidePanel",
        "TopLeading",
        "TopTrailing",
        "ContextMenu",
    ] {
        assert!(
            !all_json.contains(platform_name),
            "共享契约不得包含平台控件名 {platform_name}"
        );
    }
}

#[test]
fn test_writing_header_actions_order() {
    let slots = resolve_screen_policy(ScreenRole::Writing);
    let header: Vec<_> = slots
        .iter()
        .filter(|s| s.region == ActionRegion::HeaderTrailing)
        .collect();
    // 写作区只保留：保存 / 同步 / 设置（#597）。
    assert_eq!(header.len(), 3);
    assert_eq!(header[0].role, ActionRole::Save);
    assert_eq!(header[1].role, ActionRole::Sync);
    assert_eq!(header[2].role, ActionRole::Settings);
    assert!(header.windows(2).all(|w| w[0].order < w[1].order));
}

#[test]
fn test_workspace_header_actions_product_order() {
    let slots = resolve_screen_policy(ScreenRole::ProjectWorkspace);
    let header: Vec<_> = slots
        .iter()
        .filter(|s| s.region == ActionRegion::HeaderTrailing)
        .collect();
    // #597：作品页顶栏右侧产品顺序（从右往左）为 设置/搜索/同步状态，
    // 代码顺序（order 升序）为 同步 → 搜索 → 设置 → 排序。
    assert_eq!(header.len(), 4);
    assert_eq!(header[0].role, ActionRole::Sync);
    assert_eq!(header[1].role, ActionRole::Search);
    assert_eq!(header[2].role, ActionRole::Settings);
    assert_eq!(header[3].role, ActionRole::Sort);
    assert!(header.windows(2).all(|w| w[0].order < w[1].order));
}

#[test]
fn test_writing_has_back_leading() {
    let slots = resolve_screen_policy(ScreenRole::Writing);
    let back = slots.iter().find(|s| s.role == ActionRole::Back).unwrap();
    assert_eq!(back.region, ActionRegion::HeaderLeading);
}

#[test]
fn test_settings_policy_only_back() {
    let slots = resolve_screen_policy(ScreenRole::Settings);
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].role, ActionRole::Back);
    assert_eq!(slots[0].region, ActionRegion::HeaderLeading);
}

#[test]
fn test_starmap_and_stats_have_no_slots() {
    // #597 正文四：星图根页没有返回动作；统计根页是独立一级入口。
    assert!(resolve_screen_policy(ScreenRole::StarMap).is_empty());
    assert!(resolve_screen_policy(ScreenRole::Stats).is_empty());
}

#[test]
fn test_home_policy() {
    let slots = resolve_screen_policy(ScreenRole::Home);
    assert_eq!(slots.len(), 2);
    assert_eq!(slots[0].role, ActionRole::Search);
    assert_eq!(slots[0].region, ActionRegion::HeaderTrailing);
    assert_eq!(slots[1].role, ActionRole::Settings);
    assert_eq!(slots[1].region, ActionRegion::HeaderTrailing);
}

#[test]
fn test_sync_policy() {
    let slots = resolve_screen_policy(ScreenRole::Sync);
    assert_eq!(slots.len(), 2);
    assert_eq!(slots[0].role, ActionRole::Back);
    assert_eq!(slots[1].role, ActionRole::Sync);
    assert_eq!(slots[1].region, ActionRegion::HeaderTrailing);
}

#[test]
fn test_delete_requires_confirmation() {
    for role in [ScreenRole::ProjectList, ScreenRole::ProjectWorkspace] {
        let slots = resolve_screen_policy(role);
        for slot in slots.iter().filter(|s| s.role == ActionRole::Delete) {
            assert!(slot.requires_confirmation);
        }
    }
}

#[test]
fn test_create_chapter_slots_distinguished_by_region() {
    let slots = resolve_screen_policy(ScreenRole::ProjectWorkspace);
    let create_chapters: Vec<_> = slots
        .iter()
        .filter(|s| s.role == ActionRole::CreateChapter)
        .collect();
    assert_eq!(create_chapters.len(), 2);
    assert_eq!(create_chapters[0].region, ActionRegion::ItemTrailing);
    assert_eq!(create_chapters[1].region, ActionRegion::EmptyState);
}

#[test]
fn test_action_slot_serialization() {
    let slot = ActionSlot {
        role: ActionRole::Back,
        region: ActionRegion::HeaderLeading,
        order: 10,
        requires_confirmation: false,
    };
    let json = serde_json::to_string(&slot).unwrap();

    let deserialized: ActionSlot = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.role, ActionRole::Back);
    assert_eq!(deserialized.region, ActionRegion::HeaderLeading);
    assert_eq!(deserialized.order, 10);
    assert!(!deserialized.requires_confirmation);
}

#[test]
fn test_slot_order_is_product_level_not_shell_dependent() {
    // #610：同一页面同一区域的槽位不随壳层变化 — 平台呈现差异由平台端决定。
    let slots = resolve_screen_policy(ScreenRole::ProjectList);
    let create = slots
        .iter()
        .find(|s| s.role == ActionRole::CreateProject)
        .unwrap();
    assert_eq!(create.region, ActionRegion::HeaderTrailing);
    assert_eq!(create.order, 10);
}
