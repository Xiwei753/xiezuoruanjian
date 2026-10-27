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
//! 4. A、B 互相 Embed 形成环：A→B→A 是合法导航路径，route 保留完整实例序列，
//!    退出一次回到 B、再退出回到根图 A；重复的 starmap_id 不构成错误。
//! 5. 33 层与 100 层链式 Embed：`resolve_starmap_navigation` 均成功，不再有固定深度上限。
//! 6. A 自嵌入 A：进入后仍停留在 A，但 route 必须追加一个入口实例段，退出回到根图。

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

/// A、B 互相 Embed 形成环：循环是合法内容，进入、解析和逐层退出都保留实例身份。
#[test]
fn mutual_embed_cycle_is_navigable_by_public_api() {
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
    assert_eq!(into_b.route.segments, vec![enter_embed("eAB")]);

    // B -eBA-> A：重复进入 A 合法，完整 route 和穿越顺序都要保留。
    let back_to_a = api
        .enter_starmap_embed(a.clone(), vec![enter_embed("eAB")], "eBA".to_string())
        .unwrap();
    assert_eq!(back_to_a.location.current_starmap_id, a);
    assert_eq!(
        back_to_a.route.segments,
        vec![enter_embed("eAB"), enter_embed("eBA")]
    );
    assert_eq!(
        back_to_a.location.traversed_starmap_ids,
        vec![a.clone(), b.clone(), a.clone()]
    );

    // 直接解析整条环路径同样成功，并能继续沿着循环再走一圈。
    let resolved = api
        .resolve_starmap_navigation(a.clone(), vec![enter_embed("eAB"), enter_embed("eBA")])
        .unwrap();
    assert_eq!(resolved.current_starmap_id, a);
    let round_trip = api
        .enter_starmap_embed(
            a.clone(),
            vec![enter_embed("eAB"), enter_embed("eBA")],
            "eAB".to_string(),
        )
        .unwrap();
    assert_eq!(round_trip.location.current_starmap_id, b);
    assert_eq!(round_trip.route.segments.len(), 3);

    // 退出只移除最后一段：先回到 B 的 eAB 入口身份，再回到根图。
    let exited_b = api.exit_starmap_canvas(back_to_a.route.clone()).unwrap();
    assert_eq!(exited_b.segments, vec![enter_embed("eAB")]);
    let exited_root = api.exit_starmap_canvas(exited_b).unwrap();
    assert!(exited_root.segments.is_empty());
    assert_eq!(exited_root.root_starmap_id, a);

    drop(tmp);
}

/// A 自嵌入 A：进入后当前画布仍是 A，但入口段必须追加，退出回到根图。
#[test]
fn self_embed_is_navigable_by_public_api() {
    let (tmp, api) = make_api();
    let a = api.create_starmap("A", "", None).unwrap().starmap_id;
    api.add_starmap_embed(&a, embed_at(&a, "eAA", &a)).unwrap();
    api.flush_all_starmap_stores().unwrap();

    let entered = api
        .enter_starmap_embed(a.clone(), vec![], "eAA".to_string())
        .unwrap();
    assert_eq!(entered.location.current_starmap_id, a);
    assert_eq!(entered.route.segments, vec![enter_embed("eAA")]);
    assert_eq!(
        entered.location.traversed_starmap_ids,
        vec![a.clone(), a.clone()]
    );

    let resolved = api
        .resolve_starmap_navigation(a.clone(), vec![enter_embed("eAA"), enter_embed("eAA")])
        .unwrap();
    assert_eq!(resolved.current_starmap_id, a);
    assert_eq!(resolved.traversed_starmap_ids.len(), 3);

    let exited = api.exit_starmap_canvas(entered.route).unwrap();
    assert!(exited.segments.is_empty());

    drop(tmp);
}

/// 构造 `depth` 层链式 Embed 的 100 层测试夹具。
fn make_chain_fixture(depth: usize) -> (TempDir, WriterCoreApi, String, String) {
    let (tmp, api) = make_api();
    let root = api.create_starmap("S0", "", None).unwrap().starmap_id;
    let mut previous = root.clone();
    for i in 0..depth {
        let next = api
            .create_starmap(&format!("S{}", i + 1), "", None)
            .unwrap()
            .starmap_id;
        api.add_starmap_embed(&previous, embed_at(&previous, &format!("e{i}"), &next))
            .unwrap();
        previous = next;
    }
    api.flush_all_starmap_stores().unwrap();
    let final_starmap_id = previous;
    (tmp, api, root, final_starmap_id)
}

/// 33 层链式导航成功。
#[test]
fn navigation_supports_33_layers() {
    let depth = 33;
    let (tmp, api, root, final_starmap_id) = make_chain_fixture(depth);
    let segments: Vec<StarMapPathSegmentDto> =
        (0..depth).map(|i| enter_embed(&format!("e{i}"))).collect();

    let location = api
        .resolve_starmap_navigation(root, segments)
        .expect("33 层是合法有限路径");
    assert_eq!(location.current_starmap_id, final_starmap_id);
    assert_eq!(location.traversed_starmap_ids.len(), depth + 1);

    drop(tmp);
}

/// 100 层链式导航成功，不再存在任意深度天花板。
#[test]
fn navigation_supports_100_layers() {
    let depth = 100;
    let (tmp, api, root, final_starmap_id) = make_chain_fixture(depth);
    let mut segments: Vec<StarMapPathSegmentDto> =
        (0..depth).map(|i| enter_embed(&format!("e{i}"))).collect();

    let location = api
        .resolve_starmap_navigation(root.clone(), segments.clone())
        .expect("100 层是合法有限路径");
    assert_eq!(location.current_starmap_id, final_starmap_id);
    assert_eq!(location.traversed_starmap_ids.len(), depth + 1);

    // 通过导航 API 逐层进入最后一段，确认返回的 route 也保留完整长度。
    let entered = api
        .enter_starmap_embed(
            root,
            segments.drain(..depth - 1).collect(),
            format!("e{}", depth - 1),
        )
        .unwrap();
    assert_eq!(entered.route.segments.len(), depth);
    assert_eq!(entered.location.current_starmap_id, final_starmap_id);

    drop(tmp);
}
