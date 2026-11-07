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
// remove_starmap_store 索引失效测试（问题2）
// ---------------------------------------------------------------------------

use crate::starmap::store::PathDependencyIndex;

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

/// 复制根图时，根/嵌套身份按源索引保留：复制起点成为新根图，
/// 其它原本是独立根图的源图仍为根图，原本是嵌套子图的仍为嵌套子图。
#[test]
fn copy_starmap_preserves_explicit_root_identity() {
    let temp = tempdir().unwrap();
    let core = new_core(temp.path());

    let root = create_starmap(&core, "根图");
    let (nested, _embed, _changes, tx_id) = core
        .create_starmap_child_embed(&root, "嵌套子图", StarMapPoint::default())
        .unwrap();
    crate::storage::journal::starmap_child_embed::ack_child_embed_history(temp.path(), &tx_id)
        .unwrap();

    let independent = create_starmap(&core, "独立根图");
    embed_child(&core, &root, &independent, "em_independent_root");

    let copy = core
        .copy_starmap_library_asset(
            &crate::starmap::types::StarMapAssetRef::starmap(root.clone()),
            &crate::starmap::types::StarMapAssetScope::Global,
            None,
            &mut |_| Ok(()),
        )
        .unwrap();
    let copied = match copy {
        crate::facade::StarMapAssetCopyResult::StarMap(copied) => copied,
        crate::facade::StarMapAssetCopyResult::Node { .. } => {
            panic!("复制星图必须返回星图结果")
        }
    };

    assert_eq!(copied.starmaps.len(), 3);
    let copied_root = copied.starmap_map.get(&root).unwrap();
    let copied_nested = copied.starmap_map.get(&nested.starmap_id).unwrap();
    let copied_independent = copied.starmap_map.get(&independent).unwrap();

    let roots = core
        .list_root_starmaps()
        .unwrap()
        .into_iter()
        .map(|meta| meta.starmap_id)
        .collect::<Vec<_>>();
    assert!(roots.contains(&root));
    assert!(roots.contains(&independent));
    assert!(roots.contains(copied_root));
    assert!(roots.contains(copied_independent));
    assert!(
        !roots.contains(&nested.starmap_id),
        "复制前是嵌套子图，复制后不能补登为根图"
    );
    assert!(
        !roots.contains(copied_nested),
        "复制出的嵌套子图不能出现在一级星图列表"
    );
}

/// 复制时 Portal 段必须切换当前源画布，后续段和终点在新目标图中映射：
/// 覆盖 EnterPortal → Node 与 EnterPortal → EnterEmbed → Anchor。
#[test]
fn copy_starmap_remaps_full_paths_through_portals() {
    let temp = tempdir().unwrap();
    let core = new_core(temp.path());

    let a = create_starmap(&core, "A");
    let b = create_starmap(&core, "B");
    let c = create_starmap(&core, "C");
    embed_child(&core, &a, &b, "em_a_b");
    embed_child(&core, &b, &c, "em_b_c");

    let node = |id: &str, anchors: Vec<crate::starmap::semantic::StarMapAnchor>| {
        crate::starmap::types::StarMapNode {
            id: id.to_string(),
            title: id.to_string(),
            kind: crate::starmap::types::StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: crate::starmap::semantic::StarMapNodeContent::Empty,
            anchors,
            portal: None,
            position: StarMapPoint::default(),
            style: crate::starmap::types::StarMapNodeStyle::default(),
            provenance: StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
        }
    };

    let mut c_node = node(
        "n_c",
        vec![crate::starmap::semantic::StarMapAnchor {
            anchor_id: "anchor_1".to_string(),
            target: crate::starmap::semantic::StarMapAnchorTarget::Project {
                project_id: "p1".to_string(),
            },
            label: None,
            role: Default::default(),
        }],
    );
    c_node.title = "锚点目标".to_string();
    core.add_starmap_node(&c, c_node, 0.0, 0.0).unwrap();
    core.add_starmap_node(&b, node("n_b", vec![]), 0.0, 0.0)
        .unwrap();

    let mut portal_node = node("n_portal", vec![]);
    portal_node.portal = Some(crate::starmap::semantic::StarMapPortal {
        destination_starmap_id: b.clone(),
        destination_target: None,
    });
    core.add_starmap_node(&a, portal_node, 0.0, 0.0)
        .unwrap();

    let endpoint = |target: StarMapTargetDetail| StarMapTargetPath {
        starmap_id: a.clone(),
        segments: vec![],
        target,
    };
    core.add_starmap_edge(
        &a,
        crate::starmap::types::StarMapEdge {
            id: "edge_portal_node".to_string(),
            from: endpoint(StarMapTargetDetail::Starmap),
            to: StarMapTargetPath {
                starmap_id: a.clone(),
                segments: vec![StarMapPathSegment::EnterPortal {
                    node_id: "n_portal".to_string(),
                }],
                target: StarMapTargetDetail::Node {
                    node_id: "n_b".to_string(),
                },
            },
            kind: StarMapEdgeKind::References,
            label: Some("portal-node".to_string()),
            payload: None,
            created_at: 0,
            updated_at: 0,
        },
    )
    .unwrap();
    core.add_starmap_edge(
        &a,
        crate::starmap::types::StarMapEdge {
            id: "edge_portal_embed_anchor".to_string(),
            from: endpoint(StarMapTargetDetail::Starmap),
            to: StarMapTargetPath {
                starmap_id: a.clone(),
                segments: vec![
                    StarMapPathSegment::EnterPortal {
                        node_id: "n_portal".to_string(),
                    },
                    StarMapPathSegment::EnterEmbed {
                        instance_id: "em_b_c".to_string(),
                    },
                ],
                target: StarMapTargetDetail::Anchor {
                    node_id: "n_c".to_string(),
                    anchor_id: "anchor_1".to_string(),
                },
            },
            kind: StarMapEdgeKind::References,
            label: Some("portal-embed-anchor".to_string()),
            payload: None,
            created_at: 0,
            updated_at: 0,
        },
    )
    .unwrap();

    let copy = core
        .copy_starmap_library_asset(
            &crate::starmap::types::StarMapAssetRef::starmap(a.clone()),
            &crate::starmap::types::StarMapAssetScope::Global,
            None,
            &mut |_| Ok(()),
        )
        .unwrap();
    let copied = match copy {
        crate::facade::StarMapAssetCopyResult::StarMap(copied) => copied,
        crate::facade::StarMapAssetCopyResult::Node { .. } => {
            panic!("复制星图必须返回星图结果")
        }
    };

    let new_a = copied.starmap_map.get(&a).unwrap();
    let new_portal = copied.node_maps.get(&a).unwrap().get("n_portal").unwrap();
    let new_b = copied.node_maps.get(&b).unwrap().get("n_b").unwrap();
    let new_c = copied.node_maps.get(&c).unwrap().get("n_c").unwrap();
    let new_embed = copied.embed_maps.get(&b).unwrap().get("em_b_c").unwrap();

    let graph = core.get_starmap_graph(new_a).unwrap();
    let portal_node_edge = graph
        .edges
        .iter()
        .find(|edge| edge.label.as_deref() == Some("portal-node"))
        .unwrap();
    assert_eq!(portal_node_edge.to.starmap_id, *new_a);
    assert_eq!(
        portal_node_edge.to.segments,
        vec![StarMapPathSegment::EnterPortal {
            node_id: new_portal.clone()
        }]
    );
    assert_eq!(
        portal_node_edge.to.target,
        StarMapTargetDetail::Node {
            node_id: new_b.clone()
        }
    );

    let portal_embed_edge = graph
        .edges
        .iter()
        .find(|edge| edge.label.as_deref() == Some("portal-embed-anchor"))
        .unwrap();
    assert_eq!(portal_embed_edge.to.starmap_id, *new_a);
    assert_eq!(
        portal_embed_edge.to.segments,
        vec![
            StarMapPathSegment::EnterPortal {
                node_id: new_portal.clone()
            },
            StarMapPathSegment::EnterEmbed {
                instance_id: new_embed.clone()
            },
        ]
    );
    assert_eq!(
        portal_embed_edge.to.target,
        StarMapTargetDetail::Anchor {
            node_id: new_c.clone(),
            anchor_id: "anchor_1".to_string()
        }
    );
}
