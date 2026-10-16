//! Issue #902 评论 6211846988 — 导航 API 错误路径与多入口实例身份集成测试。
//!
//! 前几轮已用真实 `WriterCoreApi` 覆盖导航的**成功**闭环
//!（`enter_starmap_embed` / `enter_starmap_portal` / `resolve_starmap_navigation` /
//! `exit_starmap_canvas` / `jump_starmap_ancestor`）以及跨画布连线的引用保护。
//! 本测试补齐同一套对外 API 必须显式区分、不能静默回退的**失败路径**，以及同一
//! 目标星图被多次 Embed 时的导航身份：
//!
//! 1. 同一目标星图 B 被根图 A 的 `e1` / `e2` 两次 Embed；经 `e1` / `e2` 进入得到
//!    的 route 各自保留实例 ID，`resolve_starmap_navigation` 都落在 B，
//!    `traversed_starmap_ids` 都是 `[A, B]`，`exit_starmap_canvas` 都回到根图。
//! 2. `enter_starmap_embed` 按**当前画布**查找实例：在 B（route=`[e2]`）中再传入只
//!    属于 A 的 `e1` 必须报 `MissingEmbed`，不能因为根图里存在 `e1` 就放行。
//! 3. `enter_starmap_portal` 只接受当前画布中带 Portal 的节点：普通节点与不存在
//!    节点都报 `MissingPortal`。
//! 4. A、B 互相 Embed 形成环：`enter_starmap_embed` 与 `resolve_starmap_navigation`
//!    都必须报 `CycleDetected`，不得把重复进入当成有效路径。
//! 5. 路径段超过 32 层：`resolve_starmap_navigation` 报 `TooDeep`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapEmbedDto, StarMapNodeDto, StarMapNodeKindDto, StarMapPathSegmentDto, StarMapPointDto,
    StarMapTargetDetailDto, StarMapTargetPathDto,
};
use writer_core::api::WriterCoreApi;

fn make_api() -> (TempDir, WriterCoreApi) {
    let tmp = tempfile::tempdir().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    let api = WriterCoreApi::new(&app_data_root, &projects_root);
    (tmp, api)
}

fn point(x: f64, y: f64) -> StarMapPointDto {
    StarMapPointDto { x, y }
}

fn node(id: &str) -> StarMapNodeDto {
    StarMapNodeDto {
        id: id.to_string(),
        title: id.to_string(),
        kind: StarMapNodeKindDto::Note,
        payload: None,
        tags: vec![],
        content: Default::default(),
        anchors: vec![],
        portal: None,
        position: point(0.0, 0.0),
        style: Default::default(),
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
    }
}

fn enter_embed(instance_id: &str) -> StarMapPathSegmentDto {
    StarMapPathSegmentDto {
        kind: "enterEmbed".to_string(),
        instance_id: Some(instance_id.to_string()),
        node_id: None,
    }
}

/// 在 `host` 中写入一个指向 `target` 的 Embed 实例，host_path 为本图层。
fn embed_at(host: &str, instance_id: &str, target: &str) -> StarMapEmbedDto {
    StarMapEmbedDto {
        instance_id: instance_id.to_string(),
        target_starmap_id: target.to_string(),
        label: None,
        position: point(0.0, 0.0),
        host_path: StarMapTargetPathDto {
            starmap_id: host.to_string(),
            segments: vec![],
            target: StarMapTargetDetailDto::default(),
        },
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
    }
}

struct Fixture {
    _tmp: TempDir,
    api: WriterCoreApi,
    a: String,
    b: String,
}

/// 根图 A 通过两个 Embed `e1` / `e2` 指向同一子图 B：验证多入口导航身份。
fn make_double_embed_fixture() -> Fixture {
    let (tmp, api) = make_api();
    let a = api.create_starmap("A", "", None).unwrap().starmap_id;
    let b = api.create_starmap("B", "", None).unwrap().starmap_id;
    api.add_starmap_embed(&a, embed_at(&a, "e1", &b)).unwrap();
    api.add_starmap_embed(&a, embed_at(&a, "e2", &b)).unwrap();
    api.flush_all_starmap_stores().unwrap();
    Fixture {
        _tmp: tmp,
        api,
        a,
        b,
    }
}

/// 同一目标星图的多个 Embed 入口保持各自的导航实例身份。
#[test]
fn navigation_keeps_instance_identity_for_repeated_embeds() {
    let f = make_double_embed_fixture();

    let via_e1 = f
        .api
        .enter_starmap_embed(f.a.clone(), vec![], "e1".to_string())
        .unwrap();
    assert_eq!(via_e1.location.current_starmap_id, f.b);
    assert_eq!(via_e1.route.segments, vec![enter_embed("e1")]);
    assert_eq!(
        via_e1.location.traversed_starmap_ids,
        vec![f.a.clone(), f.b.clone()]
    );

    let via_e2 = f
        .api
        .enter_starmap_embed(f.a.clone(), vec![], "e2".to_string())
        .unwrap();
    assert_eq!(via_e2.location.current_starmap_id, f.b);
    assert_eq!(via_e2.route.segments, vec![enter_embed("e2")]);
    assert_eq!(
        via_e2.location.traversed_starmap_ids,
        vec![f.a.clone(), f.b.clone()]
    );

    // 两条 route 都落在 B，但保留的是各自的进入实例，不是同一个身份。
    assert_ne!(via_e1.route, via_e2.route);

    let resolved_e1 = f
        .api
        .resolve_starmap_navigation(f.a.clone(), vec![enter_embed("e1")])
        .unwrap();
    assert_eq!(resolved_e1.current_starmap_id, f.b);
    let resolved_e2 = f
        .api
        .resolve_starmap_navigation(f.a.clone(), vec![enter_embed("e2")])
        .unwrap();
    assert_eq!(resolved_e2.current_starmap_id, f.b);

    // 退出都回到根图 A，且不残留实例段。
    for entered in [&via_e1, &via_e2] {
        let exited = f.api.exit_starmap_canvas(entered.route.clone()).unwrap();
        assert!(exited.segments.is_empty());
        assert_eq!(exited.root_starmap_id, f.a);
    }
}

/// `enter_starmap_embed` 必须按当前画布查找实例，不能靠根图里存在就放行。
#[test]
fn enter_embed_resolves_instance_in_current_canvas_only() {
    let f = make_double_embed_fixture();

    // 当前画布是 B（route=[e2]）；e1 只存在于根图 A，因此必须报 MissingEmbed。
    let error = f
        .api
        .enter_starmap_embed(f.a.clone(), vec![enter_embed("e2")], "e1".to_string())
        .expect_err("e1 不在当前画布 B，不能从 B 直接进入");
    assert!(
        format!("{error:?}").contains("MissingEmbed"),
        "必须是 MissingEmbed，而非其他错误；实际: {error:?}"
    );

    // 根图上不存在的实例同样是 MissingEmbed。
    let error = f
        .api
        .enter_starmap_embed(f.a.clone(), vec![], "no_such_embed".to_string())
        .expect_err("不存在的 Embed 实例必须报错");
    assert!(
        format!("{error:?}").contains("MissingEmbed"),
        "必须是 MissingEmbed；实际: {error:?}"
    );
}

/// `enter_starmap_portal` 只接受当前画布中带 Portal 的节点。
#[test]
fn enter_portal_rejects_non_portal_and_missing_nodes() {
    let f = make_double_embed_fixture();
    // B 中放入一个**没有** Portal 的普通节点 n1。
    f.api.add_starmap_node(&f.b, node("n1"), 0.0, 0.0).unwrap();
    f.api.flush_all_starmap_stores().unwrap();

    // n1 存在但没有 Portal → MissingPortal。
    let error = f
        .api
        .enter_starmap_portal(f.a.clone(), vec![enter_embed("e2")], "n1".to_string())
        .expect_err("普通节点不是 Portal，不能穿行");
    assert!(
        format!("{error:?}").contains("MissingPortal"),
        "必须是 MissingPortal；实际: {error:?}"
    );

    // 完全不存在的节点 → MissingPortal。
    let error = f
        .api
        .enter_starmap_portal(f.a.clone(), vec![enter_embed("e2")], "ghost".to_string())
        .expect_err("不存在的节点不能穿行");
    assert!(
        format!("{error:?}").contains("MissingPortal"),
        "必须是 MissingPortal；实际: {error:?}"
    );
}

/// A、B 互相 Embed 形成环，进出与解析都必须拒绝，不得把重复进入当成有效路径。
#[test]
fn mutual_embed_cycle_is_rejected_by_public_api() {
    let (tmp, api) = make_api();
    let a = api.create_starmap("A", "", None).unwrap().starmap_id;
    let b = api.create_starmap("B", "", None).unwrap().starmap_id;
    api.add_starmap_embed(&a, embed_at(&a, "eAB", &b)).unwrap();
    api.add_starmap_embed(&b, embed_at(&b, "eBA", &a)).unwrap();
    api.flush_all_starmap_stores().unwrap();

    // A -eAB-> B 成功。
    let into_b = api
        .enter_starmap_embed(a.clone(), vec![], "eAB".to_string())
        .unwrap();
    assert_eq!(into_b.location.current_starmap_id, b);

    // B -eBA-> A 会重复进入 A，必须报 CycleDetected。
    let error = api
        .enter_starmap_embed(a.clone(), vec![enter_embed("eAB")], "eBA".to_string())
        .expect_err("重复进入 A 必须被识别为环");
    assert!(
        format!("{error:?}").contains("CycleDetected"),
        "必须是 CycleDetected；实际: {error:?}"
    );

    // 直接解析整条环路径也必须拒绝，而不是返回某个画布位置。
    let error = api
        .resolve_starmap_navigation(a.clone(), vec![enter_embed("eAB"), enter_embed("eBA")])
        .expect_err("环路径不能被解析成有效位置");
    assert!(
        format!("{error:?}").contains("CycleDetected"),
        "必须是 CycleDetected；实际: {error:?}"
    );

    drop(tmp);
}

/// 路径段超过 32 层，`resolve_starmap_navigation` 必须报 TooDeep。
#[test]
fn navigation_depth_limit_is_enforced() {
    let (tmp, api) = make_api();
    let a = api.create_starmap("A", "", None).unwrap().starmap_id;

    let segments: Vec<StarMapPathSegmentDto> =
        (0..33).map(|i| enter_embed(&format!("e{i}"))).collect();
    let error = api
        .resolve_starmap_navigation(a.clone(), segments)
        .expect_err("超过 32 层的路径必须报 TooDeep");
    assert!(
        format!("{error:?}").contains("TooDeep"),
        "必须是 TooDeep；实际: {error:?}"
    );

    drop(tmp);
}
