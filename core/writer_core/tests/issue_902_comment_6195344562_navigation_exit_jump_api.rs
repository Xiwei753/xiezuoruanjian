//! Issue #902 评论 6195344562 — 星图"返回层级"对外 API 集成测试。
//!
//! #902 要求统一 Core API 覆盖"入口实例路径 / 当前画布解析 / **返回层级** /
//! 跨画布连线 / 关联反查"。此前 `exit_canvas` / `jump_to_ancestor` 只存在于
//! `starmap::navigation` 领域模块，未通过 `WriterCoreApi`、UniFFI(UDL) 或
//! JSON C-ABI 导出，平台端无法经 Core 返回上层画布。
//!
//! 本测试通过真实 `WriterCoreApi` 出口验证：
//! 1. `enter_starmap_embed` 进入子画布后，`exit_starmap_canvas` 弹出最后一段；
//! 2. `jump_starmap_ancestor` 支持跳到任意祖先层级（含 0 = 根图、超过长度 = 原路由）；
//! 3. 返回后的路由仍可被 `resolve_starmap_navigation` 正确解析到对应画布。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::StarMapPointDto;
use writer_core::api::WriterCoreApi;

fn make_api() -> (TempDir, WriterCoreApi) {
    let tmp = tempfile::tempdir().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    let api = WriterCoreApi::new(&app_data_root, &projects_root);
    (tmp, api)
}

fn position() -> StarMapPointDto {
    StarMapPointDto { x: 30.0, y: 40.0 }
}

/// 单层：进入子画布后退出，回到根图。
#[test]
fn exit_starmap_canvas_pops_one_level_back_to_root() {
    let (_tmp, api) = make_api();

    let host = api.create_starmap("根图", "根", None).unwrap();
    let child = api
        .create_starmap_child_embed(&host.starmap_id, "子图", position())
        .unwrap();
    let instance_id = child.embed.instance_id.clone();

    let entered = api
        .enter_starmap_embed(host.starmap_id.clone(), Vec::new(), instance_id)
        .unwrap();
    assert_eq!(entered.route.segments.len(), 1);
    assert_eq!(
        entered.location.current_starmap_id,
        child.starmap.starmap_id
    );

    let exited = api.exit_starmap_canvas(entered.route.clone()).unwrap();
    assert_eq!(exited.root_starmap_id, host.starmap_id);
    assert!(exited.segments.is_empty(), "退出后应回到根图（空路径）");

    // 已在根图时退出返回原路由。
    let exited_again = api.exit_starmap_canvas(exited.clone()).unwrap();
    assert_eq!(exited_again, exited);
}

/// 多层：跳到任意祖先层级，截断后的路由仍可解析到正确画布。
#[test]
fn jump_starmap_ancestor_truncates_and_stays_resolvable() {
    let (_tmp, api) = make_api();

    let host = api.create_starmap("根图", "根", None).unwrap();
    let child = api
        .create_starmap_child_embed(&host.starmap_id, "子图", position())
        .unwrap();
    let grandchild = api
        .create_starmap_child_embed(&child.starmap.starmap_id, "孙图", position())
        .unwrap();

    let entered_child = api
        .enter_starmap_embed(
            host.starmap_id.clone(),
            Vec::new(),
            child.embed.instance_id.clone(),
        )
        .unwrap();
    let entered_grandchild = api
        .enter_starmap_embed(
            host.starmap_id.clone(),
            entered_child.route.segments.clone(),
            grandchild.embed.instance_id.clone(),
        )
        .unwrap();
    assert_eq!(entered_grandchild.route.segments.len(), 2);

    // depth=1：保留第一个段，回到子图。
    let jumped_child = api
        .jump_starmap_ancestor(entered_grandchild.route.clone(), 1)
        .unwrap();
    assert_eq!(jumped_child.segments.len(), 1);
    let resolved_child = api
        .resolve_starmap_navigation(host.starmap_id.clone(), jumped_child.segments.clone())
        .unwrap();
    assert_eq!(resolved_child.current_starmap_id, child.starmap.starmap_id);

    // depth=0：直接回到根图。
    let jumped_root = api
        .jump_starmap_ancestor(entered_grandchild.route.clone(), 0)
        .unwrap();
    assert!(jumped_root.segments.is_empty());
    let resolved_root = api
        .resolve_starmap_navigation(host.starmap_id.clone(), jumped_root.segments.clone())
        .unwrap();
    assert_eq!(resolved_root.current_starmap_id, host.starmap_id);

    // depth 超过路径长度：返回原路由，不做截断。
    let jumped_noop = api
        .jump_starmap_ancestor(entered_grandchild.route.clone(), 10)
        .unwrap();
    assert_eq!(jumped_noop, entered_grandchild.route);

    // 从孙图退出一层，也应落在子图。
    let exited_child = api
        .exit_starmap_canvas(entered_grandchild.route.clone())
        .unwrap();
    assert_eq!(exited_child, jumped_child);
}
