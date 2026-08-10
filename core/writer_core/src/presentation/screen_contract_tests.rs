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
    // #610 评论二：Save（自动保存）/Sort（未实现）已从共享契约删除，
    // Core 不再声明平台上不存在于当前 UI 的动作。
    let variants = vec![
        ActionRole::Back,
        ActionRole::CreateProject,
        ActionRole::CreateVolume,
        ActionRole::CreateChapter,
        ActionRole::Delete,
        ActionRole::Rename,
        ActionRole::Settings,
        ActionRole::Sync,
        ActionRole::Search,
    ];
    assert_eq!(variants.len(), 9);
}

#[test]
fn test_action_target_variants() {
    // #610 评论二：平台无关的业务目标身份。
    let variants = [
        ActionTarget::App,
        ActionTarget::Project,
        ActionTarget::Volume,
        ActionTarget::Chapter,
    ];
    assert_eq!(variants.len(), 4);
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
    // #610 评论二：正文自动保存，写作区顶栏只保留真实存在的同步 / 设置。
    assert_eq!(header.len(), 2);
    assert_eq!(header[0].role, ActionRole::Sync);
    assert_eq!(header[1].role, ActionRole::Settings);
    assert!(header.windows(2).all(|w| w[0].order < w[1].order));
}

#[test]
fn test_writing_has_no_save_slot() {
    // #610 评论二：Core 不得声明平台上被过滤掉的动作（Save 是第二真相）。
    // 枚举已删除 Save，此处用序列化结果做门禁：任何死动作名不得出现在契约里。
    let json = serde_json::to_string(&resolve_screen_policy(ScreenRole::Writing)).unwrap();
    assert!(
        !json.contains("Save"),
        "Writing 契约不得包含 Save（正文自动保存，动作不存在）"
    );
}

#[test]
fn test_workspace_has_no_sort_slot() {
    // #610 评论二：Sort 未实现，不得在共享契约中声明。
    let json = serde_json::to_string(&resolve_screen_policy(ScreenRole::ProjectWorkspace)).unwrap();
    assert!(
        !json.contains("Sort"),
        "ProjectWorkspace 契约不得包含 Sort（未实现，动作不存在）"
    );
}

#[test]
fn test_workspace_header_actions_product_order() {
    let slots = resolve_screen_policy(ScreenRole::ProjectWorkspace);
    let header: Vec<_> = slots
        .iter()
        .filter(|s| s.region == ActionRegion::HeaderTrailing)
        .collect();
    // #597：作品页顶栏右侧产品顺序（从右往左）为 设置/搜索/同步状态，
    // 代码顺序（order 升序）为 同步 → 搜索 → 设置；Sort 已删除（#610 评论二）。
    assert_eq!(header.len(), 3);
    assert_eq!(header[0].role, ActionRole::Sync);
    assert_eq!(header[1].role, ActionRole::Search);
    assert_eq!(header[2].role, ActionRole::Settings);
    assert!(header.windows(2).all(|w| w[0].order < w[1].order));
}

#[test]
fn test_workspace_context_actions_have_business_targets() {
    // #610 评论二：Delete/Rename 靠 ActionTarget 区分"删卷/删章节"、"重命名卷/重命名章节"。
    let slots = resolve_screen_policy(ScreenRole::ProjectWorkspace);
    let context: Vec<_> = slots
        .iter()
        .filter(|s| s.region == ActionRegion::Context)
        .collect();
    assert_eq!(context.len(), 4);
    assert_eq!(context[0].role, ActionRole::Delete);
    assert_eq!(context[0].target, ActionTarget::Volume);
    assert_eq!(context[1].role, ActionRole::Delete);
    assert_eq!(context[1].target, ActionTarget::Chapter);
    assert_eq!(context[2].role, ActionRole::Rename);
    assert_eq!(context[2].target, ActionTarget::Volume);
    assert_eq!(context[3].role, ActionRole::Rename);
    assert_eq!(context[3].target, ActionTarget::Chapter);
    // 同一 role 的不同业务目标必须可区分（身份不能靠顺序猜）。
    assert_ne!(context[0].target, context[1].target);
    assert_ne!(context[2].target, context[3].target);
}

#[test]
fn test_create_chapter_targets_volume() {
    let slots = resolve_screen_policy(ScreenRole::ProjectWorkspace);
    let create_chapters: Vec<_> = slots
        .iter()
        .filter(|s| s.role == ActionRole::CreateChapter)
        .collect();
    // 两个入口分别位于 ItemTrailing / EmptyState（区域是产品语义）。
    assert_eq!(create_chapters.len(), 2);
    assert_eq!(create_chapters[0].region, ActionRegion::ItemTrailing);
    assert_eq!(create_chapters[1].region, ActionRegion::EmptyState);
    // #610 评论二：CreateChapter + Volume。
    assert!(create_chapters
        .iter()
        .all(|s| s.target == ActionTarget::Volume));
    // 新建卷作用于作品。
    let create_volume = slots
        .iter()
        .find(|s| s.role == ActionRole::CreateVolume)
        .unwrap();
    assert_eq!(create_volume.target, ActionTarget::Project);
}

#[test]
fn test_project_list_targets_project() {
    // #610 评论二：ProjectList 的删除/重命名目标就是 Project。
    let slots = resolve_screen_policy(ScreenRole::ProjectList);
    let delete = slots.iter().find(|s| s.role == ActionRole::Delete).unwrap();
    assert_eq!(delete.target, ActionTarget::Project);
    let rename = slots.iter().find(|s| s.role == ActionRole::Rename).unwrap();
    assert_eq!(rename.target, ActionTarget::Project);
    let create = slots
        .iter()
        .find(|s| s.role == ActionRole::CreateProject)
        .unwrap();
    assert_eq!(create.target, ActionTarget::Project);
}

#[test]
fn test_app_actions_have_app_target() {
    // #610 评论二：Settings/Search/Sync/Back 这类没有业务对象的动作使用 App。
    for role in [
        ScreenRole::Home,
        ScreenRole::ProjectWorkspace,
        ScreenRole::Writing,
        ScreenRole::Sync,
    ] {
        let slots = resolve_screen_policy(role);
        for slot in slots.iter().filter(|s| {
            matches!(
                s.role,
                ActionRole::Settings | ActionRole::Search | ActionRole::Sync | ActionRole::Back
            )
        }) {
            assert_eq!(
                slot.target,
                ActionTarget::App,
                "{role:?} 的 {:?} 应为 App 目标",
                slot.role
            );
        }
    }
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
fn test_action_slot_serialization() {
    let slot = ActionSlot {
        role: ActionRole::Back,
        target: ActionTarget::App,
        region: ActionRegion::HeaderLeading,
        order: 10,
        requires_confirmation: false,
    };
    let json = serde_json::to_string(&slot).unwrap();

    let deserialized: ActionSlot = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.role, ActionRole::Back);
    assert_eq!(deserialized.target, ActionTarget::App);
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
