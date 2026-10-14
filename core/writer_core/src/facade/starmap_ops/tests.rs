//! `WriterCore::resolve_starmap_path` 的行为测试。
//!
//! 验证 root starmap + 路径段逐段解析：空路径落在 root，逐段 EnterEmbed 落在
//! 各层子图；路径失效（Embed 被删/不存在）时返回错误，不静默落回裸目标 ID。

use crate::facade::WriterCore;
use crate::starmap::semantic::{StarMapProvenance, StarMapTargetDetail};
use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};
use crate::starmap::types::{StarMapEmbed, StarMapPoint};
use tempfile::tempdir;

fn new_core(temp: &std::path::Path) -> WriterCore {
    let projects_root = temp.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    WriterCore::new(temp, &projects_root)
}

fn create_starmap(core: &WriterCore, title: &str) -> String {
    core.create_starmap(title, "", None).unwrap().starmap_id
}

fn embed_child(core: &WriterCore, host: &str, target: &str, instance_id: &str) {
    let embed = StarMapEmbed {
        instance_id: instance_id.to_string(),
        target_starmap_id: target.to_string(),
        label: None,
        position: StarMapPoint { x: 0.0, y: 0.0 },
        host_path: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    };
    core.add_starmap_embed(host, embed).unwrap();
}

/// root + 路径段逐段解析：空路径落在 root 本身，逐段 EnterEmbed 落在各层子图。
#[test]
fn resolve_starmap_path_walks_embed_segments() {
    let temp = tempdir().unwrap();
    let core = new_core(temp.path());

    let root = create_starmap(&core, "根图");
    let child = create_starmap(&core, "子图");
    let grandchild = create_starmap(&core, "孙图");
    embed_child(&core, &root, &child, "em_root_child");
    embed_child(&core, &child, &grandchild, "em_child_grand");

    let empty = core.resolve_starmap_path(&root, vec![]).unwrap();
    assert_eq!(empty.final_starmap_id, root);

    let one = core
        .resolve_starmap_path(
            &root,
            vec![StarMapPathSegment::EnterEmbed {
                instance_id: "em_root_child".to_string(),
            }],
        )
        .unwrap();
    assert_eq!(one.final_starmap_id, child);

    let two = core.resolve_starmap_path(
        &root,
        vec![
            StarMapPathSegment::EnterEmbed {
                instance_id: "em_root_child".to_string(),
            },
            StarMapPathSegment::EnterPortal {
                node_id: "n_missing".to_string(),
            },
        ],
    );
    assert!(two.is_err(), "不存在的 portal 段必须解析失败");

    let two_embeds = core
        .resolve_starmap_path(
            &root,
            vec![
                StarMapPathSegment::EnterEmbed {
                    instance_id: "em_root_child".to_string(),
                },
                StarMapPathSegment::EnterEmbed {
                    instance_id: "em_child_grand".to_string(),
                },
            ],
        )
        .unwrap();
    assert_eq!(two_embeds.final_starmap_id, grandchild);
}

/// 一级列表读索引里显式 root 身份：原子创建的子星图（嵌套入口）不出现，
/// 普通新建的一级星图出现；一级列表不需要加载图数据。
#[test]
fn list_root_starmaps_reads_explicit_nested_identity() {
    let temp = tempdir().unwrap();
    let core = new_core(temp.path());

    let root = create_starmap(&core, "根图");
    let (child, _embed, _changes, tx_id) = core
        .create_starmap_child_embed(
            &root,
            "嵌套子图",
            crate::starmap::types::StarMapPoint::default(),
        )
        .unwrap();
    crate::storage::journal::starmap_child_embed::ack_child_embed_history(temp.path(), &tx_id)
        .unwrap();

    let roots = core.list_root_starmaps().unwrap();
    assert_eq!(roots.len(), 1, "一级列表只能有普通新建的根星图");
    assert_eq!(roots[0].starmap_id, root);
    assert!(
        !roots.iter().any(|m| m.starmap_id == child.starmap_id),
        "嵌套子星图身份在创建入口就是嵌套，一级列表不得出现"
    );

    let all = core.list_starmaps().unwrap();
    assert_eq!(all.len(), 2, "总表仍包含嵌套子星图");
}

/// 路径失效（Embed 被删/不存在）时返回错误，而不是静默落回裸目标 ID。
#[test]
fn resolve_starmap_path_rejects_missing_embed() {
    let temp = tempdir().unwrap();
    let core = new_core(temp.path());

    let root = create_starmap(&core, "根图");
    let child = create_starmap(&core, "子图");
    embed_child(&core, &root, &child, "em_root_child");

    let missing = core.resolve_starmap_path(
        &root,
        vec![StarMapPathSegment::EnterEmbed {
            instance_id: "em_removed".to_string(),
        }],
    );
    assert!(missing.is_err(), "已删除的 Embed 段必须解析失败");

    let resolved = core
        .resolve_starmap_path(
            &root,
            vec![StarMapPathSegment::EnterEmbed {
                instance_id: "em_root_child".to_string(),
            }],
        )
        .unwrap();
    assert_eq!(resolved.final_starmap_id, child);
}

// ---------------------------------------------------------------------------
// create_cross_canvas_edge 和跨图删除保护测试
// ---------------------------------------------------------------------------

use crate::starmap::connections::StarMapEndpointSelection;
use crate::starmap::navigation::StarMapNavigationRoute;
use crate::starmap::types::StarMapEdgeKind;

fn add_node(core: &WriterCore, starmap_id: &str, node_id: &str) {
    let node = crate::starmap::types::StarMapNode {
        id: node_id.to_string(),
        title: node_id.to_string(),
        kind: crate::starmap::types::StarMapNodeKind::Character,
        payload: None,
        tags: vec![],
        content: crate::starmap::semantic::StarMapNodeContent::Empty,
        anchors: vec![],
        portal: None,
        position: StarMapPoint { x: 0.0, y: 0.0 },
        style: crate::starmap::types::StarMapNodeStyle::default(),
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    };
    core.add_starmap_node(starmap_id, node, 0.0, 0.0).unwrap();
}

/// `create_cross_canvas_edge` 真正写入宿主图 Store：创建后重新读 graph 能看到这条边。
#[test]
fn create_cross_canvas_edge_writes_to_store() {
    let temp = tempdir().unwrap();
    let core = new_core(temp.path());

    let root = create_starmap(&core, "根图");
    let child = create_starmap(&core, "子图");
    embed_child(&core, &root, &child, "em_root_child");
    add_node(&core, &root, "n_root");
    add_node(&core, &child, "n_child");

    // from = 根图的 n_root，to = 子图的 n_child（经 em_root_child 进入）
    let from = StarMapEndpointSelection {
        route: StarMapNavigationRoute {
            root_starmap_id: root.clone(),
            segments: vec![],
        },
        target: StarMapTargetDetail::Node {
            node_id: "n_root".to_string(),
        },
    };
    let to = StarMapEndpointSelection {
        route: StarMapNavigationRoute {
            root_starmap_id: root.clone(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "em_root_child".to_string(),
            }],
        },
        target: StarMapTargetDetail::Node {
            node_id: "n_child".to_string(),
        },
    };

    let result = core
        .create_cross_canvas_edge(
            from,
            to,
            StarMapEdgeKind::References,
            Some("test edge".to_string()),
            None,
        )
        .unwrap();

    // 宿主图是根图（最长公共前缀对应的图）
    assert_eq!(result.owner_starmap_id, root);

    // 重新读 graph，应该能看到这条边
    let graph = core.get_starmap_graph(&root).unwrap();
    assert!(
        graph.edges.iter().any(|e| e.id == result.edge.id),
        "创建的边必须出现在宿主图 Store 中"
    );
    assert_eq!(result.edge.label.as_deref(), Some("test edge"));
}

/// 有跨图 Edge 引用时拒绝删除被引用的 Node。
#[test]
fn delete_node_blocked_by_cross_canvas_edge_reference() {
    let temp = tempdir().unwrap();
    let core = new_core(temp.path());

    let root = create_starmap(&core, "根图");
    let child = create_starmap(&core, "子图");
    embed_child(&core, &root, &child, "em_root_child");
    add_node(&core, &root, "n_root");
    add_node(&core, &child, "n_child");

    // 创建跨画布边：根图 n_root → 子图 n_child
    let from = StarMapEndpointSelection {
        route: StarMapNavigationRoute {
            root_starmap_id: root.clone(),
            segments: vec![],
        },
        target: StarMapTargetDetail::Node {
            node_id: "n_root".to_string(),
        },
    };
    let to = StarMapEndpointSelection {
        route: StarMapNavigationRoute {
            root_starmap_id: root.clone(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "em_root_child".to_string(),
            }],
        },
        target: StarMapTargetDetail::Node {
            node_id: "n_child".to_string(),
        },
    };
    core.create_cross_canvas_edge(from, to, StarMapEdgeKind::References, None, None)
        .unwrap();

    // 删除子图的 n_child 应该失败（根图的边引用了它）
    let err = core.delete_starmap_node(&child, "n_child");
    assert!(err.is_err(), "有跨图边引用时必须拒绝删除被引用的节点");
    let err_msg = format!("{:?}", err.unwrap_err());
    assert!(
        err_msg.contains("cross-canvas references"),
        "错误消息应说明跨图引用存在: {err_msg}"
    );
}

/// 有跨图 Link 引用时拒绝删除被引用的 Node（全类型扫描）。
#[test]
fn delete_node_blocked_by_cross_canvas_link_reference() {
    let temp = tempdir().unwrap();
    let core = new_core(temp.path());

    let root = create_starmap(&core, "根图");
    let child = create_starmap(&core, "子图");
    embed_child(&core, &root, &child, "em_root_child");
    add_node(&core, &root, "n_root");
    add_node(&core, &child, "n_child");

    // 在根图创建 Link，source 指向子图的 n_child
    let link = crate::starmap::types::StarMapLink {
        link_id: "lk_test".to_string(),
        source: StarMapTargetPath {
            starmap_id: root.clone(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "em_root_child".to_string(),
            }],
            target: StarMapTargetDetail::Node {
                node_id: "n_child".to_string(),
            },
        },
        target: StarMapTargetPath {
            starmap_id: root.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n_root".to_string(),
            },
        },
        label: None,
        created_at: 0,
        updated_at: 0,
    };
    core.add_starmap_link(&root, link).unwrap();

    // 删除子图的 n_child 应该失败（根图的 Link 引用了它）
    let err = core.delete_starmap_node(&child, "n_child");
    assert!(err.is_err(), "有跨图 Link 引用时必须拒绝删除被引用的节点");
}

// ---------------------------------------------------------------------------
// evict_starmap_store / remove_starmap_store 行为差异测试（问题2）
// ---------------------------------------------------------------------------

use crate::starmap::store::PathDependencyIndex;

/// `evict_starmap_store` 只移除 Store，不失效路径依赖索引。
///
/// 对比 `remove_starmap_store`：后者同时失效索引。同步后批量协调用
/// `evict_starmap_store` 避免每个 target 都失效一次（问题2）。
#[test]
fn evict_starmap_store_removes_store_but_keeps_index() {
    let temp = tempdir().unwrap();
    let core = new_core(temp.path());

    let root = create_starmap(&core, "根图");
    add_node(&core, &root, "n_root");

    // 访问 graph 会把 Store 加载进 starmap_stores 缓存。
    let _ = core.get_starmap_graph(&root).unwrap();
    assert!(
        core.starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&root),
        "访问 graph 后 Store 应在缓存中"
    );

    // 模拟已初始化的路径依赖索引（Some）。
    *core
        .path_dependency_index
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(PathDependencyIndex::default());

    // evict 只移除 Store，不失效索引。
    core.evict_starmap_store(&root);

    assert!(
        !core
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&root),
        "evict 后 Store 应从缓存移除"
    );
    assert!(
        core.path_dependency_index
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some(),
        "evict 不应失效路径依赖索引（由调用方统一失效）"
    );
}

/// `remove_starmap_store` 同时移除 Store 并失效索引（整图删除场景）。
#[test]
fn remove_starmap_store_invalidates_index() {
    let temp = tempdir().unwrap();
    let core = new_core(temp.path());

    let root = create_starmap(&core, "根图");
    add_node(&core, &root, "n_root");

    let _ = core.get_starmap_graph(&root).unwrap();
    *core
        .path_dependency_index
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(PathDependencyIndex::default());

    core.remove_starmap_store(&root);

    assert!(
        !core
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&root),
        "remove 后 Store 应从缓存移除"
    );
    assert!(
        core.path_dependency_index
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none(),
        "remove 应失效路径依赖索引"
    );
}
