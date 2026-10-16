//! Issue #902 评论 6211246189 — authored 文档坐标升级为 f64 后的真实往返验证。
//!
//! 实现路线第 4 条要求把 `StarMapPoint` / `StarMapPointDto` 的 `x`/`y` 从早期
//! 的 `f32` 截断升级为 `f64`，"去掉中途转回 `f32` 的截断，仍拒绝 NaN/Inf"，
//! 让远离局部原点的画布坐标保持精度。此前各轮的集成测试都只使用 `0.0, 0.0`
//! 坐标，没有验证这一条。
//!
//! 本测试经真实 `WriterCoreApi` 覆盖：
//! 1. 节点超大的、带多位小数的 authored 坐标在 `add_starmap_node` 后再经
//!    `close_starmap_store` 触发 flush + 清内存 Store + 从磁盘重新
//!    `get_starmap_graph` 的完整往返，仍逐 bit 与输入一致（若中途退回 `f32`
//!    必然失配）。
//! 2. 通过 `update_starmap_node` 的 position patch 更新到另一个远方坐标，同样
//!    经磁盘往返保持 `f64` 精度。
//! 3. Embed 入口实例的 authored 坐标同样以 `f64` 落盘并读回。
//! 4. NaN / ±Inf 坐标仍被校验拒绝，且不落地任何节点。

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation
)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapNodeDto, StarMapNodeKindDto, StarMapNodePatchDto, StarMapPointDto,
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

fn position_patch(position: StarMapPointDto) -> StarMapNodePatchDto {
    StarMapNodePatchDto {
        title: None,
        kind: None,
        payload: None,
        tags: None,
        content: None,
        anchors: None,
        portal: None,
        position: Some(position),
        style: None,
        provenance: None,
    }
}

/// 断言该 `f64` 无法用 `f32` 精确表示——否则精度断言没有鉴别力。
fn assert_not_f32_representable(value: f64) {
    assert_ne!(
        value,
        f64::from(value as f32),
        "测试取值 {value} 必须不是 f32 可精确表示的，否则无法探测截断"
    );
}

#[test]
fn authored_node_position_round_trips_as_f64_through_disk() {
    let (_tmp, api) = make_api();
    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;

    // 典型无限画布远处坐标：既大又带足够多小数位。
    let x = 12_345_678_901.234_567_f64;
    let y = -7_654_321.987_654_321_f64;
    assert_not_f32_representable(x);
    assert_not_f32_representable(y);

    let created = api.add_starmap_node(&root, node("n_far"), x, y).unwrap();
    assert_eq!(created.position, point(x, y));

    // 写入端同时去掉中途转回 f32 的截断：Store 读回也不应失真。
    let graph = api.get_starmap_graph(&root).unwrap();
    let stored = graph.nodes.iter().find(|n| n.id == "n_far").unwrap();
    assert_eq!(stored.position, point(x, y));

    // 磁盘往返：close 会先 flush 再清掉内存 Store，随后 get 从磁盘真正加载。
    api.close_starmap_store(&root).unwrap();
    let reloaded = api.get_starmap_graph(&root).unwrap();
    let reloaded_node = reloaded
        .nodes
        .iter()
        .find(|n| n.id == "n_far")
        .expect("节点应已落盘并可重新加载");
    assert_eq!(reloaded_node.position, point(x, y));
}

#[test]
fn node_position_update_keeps_f64_precision_through_disk() {
    let (_tmp, api) = make_api();
    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;

    api.add_starmap_node(&root, node("n1"), 0.0, 0.0).unwrap();

    let x = -98_765_432_109.876_54_f64;
    let y = 1_234.0_f64 / 7.0;
    assert_not_f32_representable(x);
    assert_not_f32_representable(y);

    let updated = api
        .update_starmap_node(&root, "n1", position_patch(point(x, y)))
        .unwrap();
    assert_eq!(updated.position, point(x, y));

    api.close_starmap_store(&root).unwrap();
    let reloaded = api.get_starmap_graph(&root).unwrap();
    let reloaded_node = reloaded
        .nodes
        .iter()
        .find(|n| n.id == "n1")
        .expect("节点应已落盘并可重新加载");
    assert_eq!(reloaded_node.position, point(x, y));
}

#[test]
fn embed_instance_position_round_trips_as_f64_through_disk() {
    let (_tmp, api) = make_api();
    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;

    let x = 6_000_000_000.000_123_f64;
    let y = 42.0_f64 / 7.0;
    assert_not_f32_representable(x);

    let child = api
        .create_starmap_child_embed(&root, "远处子图", point(x, y))
        .unwrap();
    let child_id = child.starmap.starmap_id.clone();
    let instance_id = child.embed.instance_id.clone();
    assert_eq!(child.embed.position, point(x, y));

    api.close_starmap_store(&root).unwrap();
    api.close_starmap_store(&child_id).unwrap();

    let reloaded = api.get_starmap_graph(&root).unwrap();
    let reloaded_embed = reloaded
        .embeds
        .iter()
        .find(|e| e.instance_id == instance_id)
        .expect("Embed 入口应已落盘并可重新加载");
    assert_eq!(reloaded_embed.position, point(x, y));
}

#[test]
fn non_finite_coordinates_are_rejected_and_do_not_land() {
    let (_tmp, api) = make_api();
    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;

    assert!(
        api.add_starmap_node(&root, node("n_nan"), f64::NAN, 0.0)
            .is_err(),
        "NaN x 必须被拒绝"
    );
    assert!(
        api.add_starmap_node(&root, node("n_inf"), 0.0, f64::INFINITY)
            .is_err(),
        "+Inf y 必须被拒绝"
    );
    assert!(
        api.add_starmap_node(&root, node("n_neg_inf"), f64::NEG_INFINITY, 0.0)
            .is_err(),
        "-Inf x 必须被拒绝"
    );

    let graph = api.get_starmap_graph(&root).unwrap();
    assert!(graph.nodes.is_empty(), "非法坐标不得落地任何节点");
}
