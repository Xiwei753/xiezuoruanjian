use super::*;
use tempfile::tempdir;

fn legacy_node_json(node_id: &str) -> Value {
    serde_json::json!({
        "id": node_id,
        "title": "旧节点",
        "kind": "Note",
        "content": {"type": "empty"},
        "anchors": [],
        "portal": null,
        "position": {"x": 12.0, "y": 34.0},
        "style": {},
        "provenance": {},
        "createdAt": 11,
        "updatedAt": 22
    })
}

fn write_legacy_node(root: &Path, starmap_id: &str, node_id: &str, value: &Value) {
    let rel = package_storage::node_rel_path(starmap_id, node_id);
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

#[test]
fn legacy_node_plan_creates_definition_and_instance_idempotently() {
    let dir = tempdir().unwrap();
    let starmap_id = "sm_1";
    let node_id = "n1";
    write_legacy_node(dir.path(), starmap_id, node_id, &legacy_node_json(node_id));
    let node_path = dir
        .path()
        .join(package_storage::node_rel_path(starmap_id, node_id));
    let bytes = std::fs::read(&node_path).unwrap();

    let plan = decode_node_object(dir.path(), starmap_id, node_id, &bytes, None).unwrap();
    assert_eq!(plan.instance.asset_id, node_id);
    assert_eq!(plan.instance.position.x, 12.0);
    assert_eq!(plan.instance.position.y, 34.0);
    assert!(plan.rewrite_instance);
    let definition = plan
        .definition
        .as_ref()
        .expect("definition must be planned");
    assert_eq!(definition.asset_id, node_id);
    assert_eq!(definition.title, "旧节点");
    assert_eq!(definition.updated_at, 22);

    persist_node_object_plan(dir.path(), starmap_id, node_id, &plan).unwrap();
    assert!(dir
        .path()
        .join(crate::starmap::library::node_definition_rel_path(node_id))
        .exists());

    // 第二次解码：节点文件已是实例，计划不再要求任何改写。
    let rewritten = std::fs::read(&node_path).unwrap();
    let again = decode_node_object(dir.path(), starmap_id, node_id, &rewritten, None).unwrap();
    assert!(!again.rewrite_instance);
    assert!(!again.wrote_definition);
    assert_eq!(again.instance.asset_id, node_id);
}

#[test]
fn conflicting_definition_merges_by_object_version() {
    let dir = tempdir().unwrap();
    let starmap_id = "sm_1";
    let node_id = "n1";
    write_legacy_node(dir.path(), starmap_id, node_id, &legacy_node_json(node_id));
    let node_path = dir
        .path()
        .join(package_storage::node_rel_path(starmap_id, node_id));
    let bytes = std::fs::read(&node_path).unwrap();
    let plan = decode_node_object(dir.path(), starmap_id, node_id, &bytes, None).unwrap();
    let mut definition = plan.definition.clone().unwrap();
    definition.title = "被改过的标题".to_string();
    crate::starmap::library::save_node_definition(dir.path(), &definition).unwrap();

    // 同一 revision 的旧节点不再被当成身份冲突：保留已有本体，不覆盖。
    let plan = decode_node_object(dir.path(), starmap_id, node_id, &bytes, None).unwrap();
    assert!(
        plan.definition.is_none(),
        "same-revision existing definition must win"
    );
    assert_eq!(plan.instance.asset_id, "n1");
    assert!(plan.rewrite_instance);

    // 旧节点原文定的 revision 更高时按对象版本发布新本体。
    let mut newer = legacy_node_json(node_id);
    newer["updatedAt"] = serde_json::json!(99);
    newer["title"] = serde_json::json!("更新的标题");
    write_legacy_node(dir.path(), starmap_id, node_id, &newer);
    let newer_bytes = std::fs::read(&node_path).unwrap();
    let plan = decode_node_object(dir.path(), starmap_id, node_id, &newer_bytes, None).unwrap();
    let definition = plan.definition.expect("newer revision must be published");
    assert_eq!(definition.title, "更新的标题");
}

#[test]
fn empty_asset_instance_uses_existing_reference_binding() {
    let dir = tempdir().unwrap();
    let starmap_id = "sm_1";
    let node_id = "n1";
    let asset_id = "asset_shared";
    let source = legacy_node_json(node_id);
    let node: StarMapNode = serde_json::from_value(source).unwrap();
    let definition = StarMapNodeDefinition::from_node(asset_id, &node);
    crate::starmap::library::save_node_definition(dir.path(), &definition).unwrap();

    let mut bindings = NodeObjectBindings::new();
    bindings.insert_instance_reference(starmap_id, node_id, asset_id, Some("p1"));
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "instanceId": node_id,
        "assetId": "",
        "position": {"x": 1.0, "y": 2.0},
        "starmapId": starmap_id,
        "createdAt": 5,
        "updatedAt": 6
    }))
    .unwrap();
    let plan =
        decode_node_object(dir.path(), starmap_id, node_id, &bytes, Some(&bindings)).unwrap();
    assert_eq!(plan.instance.asset_id, asset_id);
    assert_eq!(plan.instance.project_id.as_deref(), Some("p1"));
    assert!(plan.rewrite_instance);
    assert!(plan.definition.is_none());
}

#[test]
fn instance_without_definition_is_not_an_identity_conflict() {
    let dir = tempdir().unwrap();
    let starmap_id = "sm_1";
    let node_id = "n1";
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "instanceId": node_id,
        "assetId": "asset_shared",
        "position": {"x": 1.0, "y": 2.0},
        "starmapId": starmap_id,
        "createdAt": 5,
        "updatedAt": 6
    }))
    .unwrap();

    // 实例可以先于/独立于共享本体存在：缺定义文件不是身份冲突。
    let plan = decode_node_object(dir.path(), starmap_id, node_id, &bytes, None).unwrap();
    assert_eq!(plan.instance.asset_id, "asset_shared");
    assert!(plan.definition.is_none());
    assert!(!plan.wrote_definition);
    assert!(!plan.rewrite_instance);
}

#[test]
fn reference_index_bindings_resolve_empty_asset_identity() {
    let dir = tempdir().unwrap();
    let starmap_id = "sm_1";
    let node_id = "n1";
    crate::starmap::library::record_node_reference(
        dir.path(),
        starmap_id,
        Some("p1"),
        node_id,
        "asset_shared",
    )
    .unwrap();
    let bindings = NodeObjectBindings::load_reference_bindings(dir.path()).unwrap();
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "instanceId": node_id,
        "assetId": "",
        "position": {"x": 1.0, "y": 2.0},
        "starmapId": starmap_id,
        "createdAt": 5,
        "updatedAt": 6
    }))
    .unwrap();

    let plan =
        decode_node_object(dir.path(), starmap_id, node_id, &bytes, Some(&bindings)).unwrap();
    assert_eq!(plan.instance.asset_id, "asset_shared");
    assert_eq!(plan.instance.project_id.as_deref(), Some("p1"));
    assert!(plan.rewrite_instance);
}

#[test]
fn instance_without_asset_id_field_resolves_reference_binding() {
    let dir = tempdir().unwrap();
    let starmap_id = "sm_1";
    let node_id = "n1";
    let asset_id = "asset_shared";
    // 共享本体属于另一作品：真实 assetId 与 instanceId 不同。
    let source: StarMapNode = serde_json::from_value(legacy_node_json(node_id)).unwrap();
    let definition = StarMapNodeDefinition::from_node(asset_id, &source);
    crate::starmap::library::save_node_definition(dir.path(), &definition).unwrap();
    crate::starmap::library::record_node_reference(
        dir.path(),
        starmap_id,
        Some("p_other"),
        node_id,
        asset_id,
    )
    .unwrap();
    let bindings = NodeObjectBindings::load_reference_bindings(dir.path()).unwrap();

    // 旧端写出的实例：只有 `instanceId`/`position`，连 `assetId` 键都没有。
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "instanceId": node_id,
        "position": {"x": 1.0, "y": 2.0}
    }))
    .unwrap();

    let plan =
        decode_node_object(dir.path(), starmap_id, node_id, &bytes, Some(&bindings)).unwrap();
    assert_eq!(plan.instance.asset_id, asset_id);
    assert_eq!(plan.instance.project_id.as_deref(), Some("p_other"));
    assert_eq!(plan.instance.starmap_id, starmap_id);
    assert!(plan.rewrite_instance);
    assert!(plan.definition.is_none());
    assert!(!plan.wrote_definition);

    // 正式输出必须带非空 assetId；持久化后再次解码不再重写。
    persist_node_object_plan(dir.path(), starmap_id, node_id, &plan).unwrap();
    let node_path = dir
        .path()
        .join(package_storage::node_rel_path(starmap_id, node_id));
    let rewritten: Value = serde_json::from_slice(&std::fs::read(&node_path).unwrap()).unwrap();
    assert_eq!(rewritten["instanceId"], node_id);
    assert_eq!(rewritten["assetId"], asset_id);
    assert_eq!(rewritten["projectId"], "p_other");

    let rewritten_bytes = serde_json::to_vec(&rewritten).unwrap();
    let again = decode_node_object(
        dir.path(),
        starmap_id,
        node_id,
        &rewritten_bytes,
        Some(&bindings),
    )
    .unwrap();
    assert!(!again.rewrite_instance);
    assert_eq!(again.instance.asset_id, asset_id);
}

#[test]
fn instance_without_asset_id_or_binding_uses_object_id_and_waits_for_definition() {
    let dir = tempdir().unwrap();
    let starmap_id = "sm_1";
    let node_id = "n1";
    // 素材库只有另一个 asset id 的本体，与节点 id 对不上，也没有反向绑定。
    let source: StarMapNode = serde_json::from_value(legacy_node_json(node_id)).unwrap();
    let definition = StarMapNodeDefinition::from_node("asset_other", &source);
    crate::starmap::library::save_node_definition(dir.path(), &definition).unwrap();

    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "instanceId": node_id,
        "position": {"x": 1.0, "y": 2.0}
    }))
    .unwrap();
    let node_path = dir
        .path()
        .join(package_storage::node_rel_path(starmap_id, node_id));
    std::fs::create_dir_all(node_path.parent().unwrap()).unwrap();
    std::fs::write(&node_path, &bytes).unwrap();

    // 只有位置、本体尚未到达的实例允许持久保存：身份退回对象 id，等待真实本体。
    let plan = decode_node_object(dir.path(), starmap_id, node_id, &bytes, None).unwrap();
    assert_eq!(plan.instance.asset_id, node_id);
    assert!(plan.definition.is_none());
    assert!(plan.rewrite_instance);

    persist_node_object_plan(dir.path(), starmap_id, node_id, &plan).unwrap();
    let rewritten: Value = serde_json::from_slice(&std::fs::read(&node_path).unwrap()).unwrap();
    assert_eq!(rewritten["assetId"], node_id);
}

#[test]
fn instance_asset_id_is_authoritative_over_stale_reference_index() {
    let dir = tempdir().unwrap();
    let starmap_id = "sm_1";
    let node_id = "n1";
    let mut bindings = NodeObjectBindings::new();
    // 派生索引落后：绑定指向旧资产，实例文件已重新绑定到新资产。
    bindings.insert_instance_reference(starmap_id, node_id, "asset_old", Some("p1"));
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "instanceId": node_id,
        "assetId": "asset_new",
        "position": {"x": 1.0, "y": 2.0},
        "starmapId": starmap_id,
        "createdAt": 5,
        "updatedAt": 6
    }))
    .unwrap();

    let plan =
        decode_node_object(dir.path(), starmap_id, node_id, &bytes, Some(&bindings)).unwrap();
    assert_eq!(plan.instance.asset_id, "asset_new");
    assert!(plan.definition.is_none());

    persist_node_object_plan(dir.path(), starmap_id, node_id, &plan).unwrap();
    let repaired = NodeObjectBindings::load_reference_bindings(dir.path()).unwrap();
    assert_eq!(
        repaired.reference_binding_state(&plan.instance),
        ReferenceBindingState::Consistent,
        "reverse index must be rebuilt from the authoritative instance"
    );
}

#[test]
fn legacy_node_tags_are_migrated_before_instance_rewrite() {
    let dir = tempdir().unwrap();
    let starmap_id = "sm_1";
    let node_id = "n1";
    let mut value = legacy_node_json(node_id);
    value["tags"] = serde_json::json!(["人物", "设定"]);
    write_legacy_node(dir.path(), starmap_id, node_id, &value);
    let node_path = dir
        .path()
        .join(package_storage::node_rel_path(starmap_id, node_id));
    let bytes = std::fs::read(&node_path).unwrap();

    let plan = decode_node_object(dir.path(), starmap_id, node_id, &bytes, None).unwrap();
    assert_eq!(plan.legacy_tags, vec!["人物".to_string(), "设定".to_string()]);
    persist_node_object_plan(dir.path(), starmap_id, node_id, &plan).unwrap();

    let rewritten: Value = serde_json::from_slice(&std::fs::read(&node_path).unwrap()).unwrap();
    assert!(rewritten.get("tags").is_none(), "{rewritten}");
}
