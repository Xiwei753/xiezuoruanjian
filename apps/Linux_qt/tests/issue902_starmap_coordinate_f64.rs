//! Issue #902 — StarMap authored 坐标升级为 f64，Qt 桥接层不得再截断成 f32。
//!
//! 背景：Core 已把 `StarMapPoint` / `StarMapPointDto` 从 f32 升级为 f64，让远离
//! 局部原点后的 authored 坐标不再丢精度。Qt 的 `starmap_bridge` 构造节点/嵌入时
//! 曾把入参 `x as f32` / `y as f32` 再转回去，导致新 DTO 精度在平台入口就被截断，
//! 甚至无法编译。这里做真实运行验证：传入 f32 无法精确表示的坐标，读回的
//! `position.x/y` 必须与 f64 入参逐位相等，且 DTO 字段本身是 f64。

#![allow(clippy::unwrap_used, clippy::expect_used)]
// 本测试刻意把 f64 截成 f32 来证明“没有发生 f32 降级”，这里的转换是有意为之。
#![allow(clippy::cast_possible_truncation, clippy::cast_lossless)]

use writer_core::api::WriterCoreApi;

/// 一个转成 f32 会失真的远处坐标：f32 只有约 24 位有效位。
const FAR_X: f64 = 123_456_789.123_456_79;
const FAR_Y: f64 = -987_654_321.987_654_3;

fn new_api() -> (tempfile::TempDir, WriterCoreApi) {
    let dir = tempfile::tempdir().unwrap();
    let projects_root = dir.path().join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    let api = WriterCoreApi::new(dir.path(), &projects_root);
    (dir, api)
}

fn data_of(raw: &str) -> serde_json::Value {
    let value: serde_json::Value = serde_json::from_str(raw).unwrap();
    assert_eq!(
        value.get("success").and_then(|s| s.as_bool()),
        Some(true),
        "桥接调用必须成功，实际: {raw}"
    );
    value["data"].clone()
}

/// 断言读回的坐标仍是 f64 精度：允许 JSON 解析的若干 ulp 误差，但必须远小于
/// 一次 f32 截断（f32 在这个量级上有 ~2.9 的绝对误差）。
fn assert_f64_precision(got: Option<f64>, expected: f64, label: &str, raw: &str) {
    let got = got.unwrap_or_else(|| panic!("{label} 缺失，实际: {raw}"));
    let tolerance = expected.abs() * f64::EPSILON * 8.0;
    assert!(
        (got - expected).abs() <= tolerance,
        "{label} 必须保留 f64 精度（不得再 as f32），期望≈{expected}，实际 {got}，原始: {raw}"
    );
    let f32_truncated = expected as f32 as f64;
    assert!(
        (got - expected).abs() < (got - f32_truncated).abs(),
        "{label} 读回值更接近 f32 截断值，说明桥接层仍发生了 f32 降级，实际: {raw}"
    );
}

/// f32 截断会改变这个值；用它证明桥接层没有把 f64 降级。
#[test]
fn f32_round_trip_would_lose_precision_for_probe_value() {
    assert_ne!(
        FAR_X as f32 as f64, FAR_X,
        "探针坐标必须能被 f32 截断检验出来"
    );
    assert_ne!(FAR_Y as f32 as f64, FAR_Y);
}

/// 节点：桥接层传入的 f64 坐标必须原样进入 Core DTO。
#[test]
fn create_node_preserves_f64_coordinates() {
    let (_dir, api) = new_api();
    let starmap = api.create_starmap("图", "", None).unwrap().starmap_id;

    let raw = sujian_linux_qt::starmap_bridge::create_starmap_node(
        &api,
        &starmap,
        "远处节点",
        "note",
        FAR_X,
        FAR_Y,
    );
    let node = data_of(&raw);
    assert_f64_precision(
        node["position"]["x"].as_f64(),
        FAR_X,
        "节点 position.x",
        &raw,
    );
    assert_f64_precision(
        node["position"]["y"].as_f64(),
        FAR_Y,
        "节点 position.y",
        &raw,
    );
}

/// 嵌入：桥接层传入的 f64 坐标必须原样进入 Core DTO。
#[test]
fn create_embed_preserves_f64_coordinates() {
    let (_dir, api) = new_api();
    let host = api.create_starmap("宿主", "", None).unwrap().starmap_id;
    let child = api.create_starmap("子图", "", None).unwrap().starmap_id;

    let raw = sujian_linux_qt::starmap_bridge::create_starmap_embed(
        &api,
        &host,
        &child,
        "子图入口",
        FAR_X,
        FAR_Y,
    );
    let embed = data_of(&raw);
    assert_f64_precision(
        embed["position"]["x"].as_f64(),
        FAR_X,
        "嵌入 position.x",
        &raw,
    );
    assert_f64_precision(
        embed["position"]["y"].as_f64(),
        FAR_Y,
        "嵌入 position.y",
        &raw,
    );
}

/// 原子创建子星图并嵌入：桥接层直接构造 `StarMapPointDto { x, y }`，
/// 同样不得把 f64 入参降级。这是 issue #902 修复的第三个写入路径。
#[test]
fn create_child_embed_preserves_f64_coordinates() {
    let (_dir, api) = new_api();
    let host = api.create_starmap("宿主", "", None).unwrap().starmap_id;

    let raw = sujian_linux_qt::starmap_bridge::create_starmap_child_embed(
        &api, &host, "子图", FAR_X, FAR_Y,
    );
    let embed = data_of(&raw)["embed"].clone();
    assert_f64_precision(
        embed["position"]["x"].as_f64(),
        FAR_X,
        "子图嵌入 position.x",
        &raw,
    );
    assert_f64_precision(
        embed["position"]["y"].as_f64(),
        FAR_Y,
        "子图嵌入 position.y",
        &raw,
    );
}
