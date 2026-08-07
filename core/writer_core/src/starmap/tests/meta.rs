use crate::starmap::*;
use tempfile::tempdir;

fn setup_workspace() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    dir
}

#[test]
fn test_starmap_graph_path() {
    let app_data_root = std::path::Path::new("/dummy/workspace");
    let starmap_id = "test_starmap_id";

    let path = starmap_graph_path(app_data_root, starmap_id);

    let expected =
        std::path::PathBuf::from("/dummy/workspace/starmaps/test_starmap_id/graph.json");
    assert_eq!(path, expected);
}

#[test]
fn test_create_and_list_starmaps() {
    let dir = setup_workspace();
    let _meta1 = create_starmap(dir.path(), "Star Map 1", "desc1", None).unwrap();
    let _meta2 = create_starmap(dir.path(), "Star Map 2", "desc2", Some("#FF0000")).unwrap();

    let all = list_starmaps(dir.path()).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].title, "Star Map 1");
    assert_eq!(all[1].title, "Star Map 2");
    assert_eq!(all[1].accent_color, "#FF0000");
}

#[test]
fn test_create_child_starmap() {
    let dir = setup_workspace();
    let parent = create_starmap(dir.path(), "Parent", "", None).unwrap();
    let child = create_child_starmap(dir.path(), &parent.starmap_id, "Child 1", "", None).unwrap();

    assert_eq!(
        child.parent_starmap_id.as_deref(),
        Some(parent.starmap_id.as_str())
    );
    assert_eq!(child.project_id, None);

    let refreshed_parent = get_starmap(dir.path(), &parent.starmap_id).unwrap();
    assert_eq!(refreshed_parent.child_starmap_count, 1);
}

#[test]
fn test_bind_and_get_main_starmap() {
    let dir = setup_workspace();
    let sm = create_starmap(dir.path(), "My Map", "", None).unwrap();

    bind_starmap_to_project(dir.path(), &sm.starmap_id, "proj1").unwrap();
    set_main_starmap_for_project(dir.path(), &sm.starmap_id, "proj1").unwrap();

    let main = get_main_starmap_for_project(dir.path(), "proj1").unwrap();
    assert!(main.is_some());
    assert_eq!(main.unwrap().starmap_id, sm.starmap_id);
}

#[test]
fn test_delete_starmap_no_cascade() {
    let dir = setup_workspace();
    let parent = create_starmap(dir.path(), "Parent", "", None).unwrap();
    let child1 = create_child_starmap(dir.path(), &parent.starmap_id, "Child 1", "", None).unwrap();
    let child2 = create_child_starmap(dir.path(), &parent.starmap_id, "Child 2", "", None).unwrap();

    delete_starmap(dir.path(), &parent.starmap_id).unwrap();

    let all = list_starmaps(dir.path()).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|m| m.starmap_id == child1.starmap_id));
    assert!(all.iter().any(|m| m.starmap_id == child2.starmap_id));
}

#[test]
fn test_rename_starmap() {
    let dir = setup_workspace();
    let sm = create_starmap(dir.path(), "Old Name", "", None).unwrap();
    let renamed = rename_starmap(dir.path(), &sm.starmap_id, "New Name").unwrap();
    assert_eq!(renamed.title, "New Name");

    let from_index = list_starmaps(dir.path()).unwrap();
    assert_eq!(from_index[0].title, "New Name");
}

#[test]
fn test_unbind_starmap() {
    let dir = setup_workspace();
    let sm = create_starmap(dir.path(), "Map", "", None).unwrap();
    bind_starmap_to_project(dir.path(), &sm.starmap_id, "proj1").unwrap();
    set_main_starmap_for_project(dir.path(), &sm.starmap_id, "proj1").unwrap();

    unbind_starmap_from_project(dir.path(), &sm.starmap_id).unwrap();

    let main = get_main_starmap_for_project(dir.path(), "proj1").unwrap();
    assert!(main.is_none());
}

#[test]
fn test_delete_starmap_edge_protection() {
    let dir = setup_workspace();
    let parent = create_starmap(dir.path(), "Parent", "", None).unwrap();
    let child = create_starmap(dir.path(), "Child", "", None).unwrap();

    let use_store_get = |ws: &std::path::Path, sid: &str| -> crate::starmap::types::StarMapGraph {
        let mut s = crate::starmap::store::StarMapStore::new(ws, sid);
        s.load_full().unwrap();
        s.to_starmap_graph()
    };
    let use_store_save =
        |ws: &std::path::Path, sid: &str, g: &crate::starmap::types::StarMapGraph| {
            let mut s = crate::starmap::store::StarMapStore::new(ws, sid);
            s.load_full().unwrap();
            for node in &g.nodes {
                s.upsert_node(node.clone());
            }
            for edge in &g.edges {
                s.upsert_edge(edge.clone());
            }
            for embed in &g.embeds {
                s.upsert_embed(embed.clone());
            }
            for link in &g.links {
                s.upsert_link(link.clone());
            }
            s.flush().unwrap();
        };
    let use_store_delete_edge = |ws: &std::path::Path, sid: &str, eid: &str| {
        let mut s = crate::starmap::store::StarMapStore::new(ws, sid);
        s.load_full().unwrap();
        s.delete_edge(eid).unwrap();
        s.flush().unwrap();
    };

    let mut parent_graph = use_store_get(dir.path(), &parent.starmap_id);
    let internal_edge = crate::starmap::types::StarMapEdge {
        id: "internal_e".to_string(),
        from: None,
        to: None,
        kind: crate::starmap::types::StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        from_target: None,
        to_target: None,
        from_endpoint: Some(crate::starmap::types::StarMapEdgeEndpoint::Starmap),
        to_endpoint: Some(crate::starmap::types::StarMapEdgeEndpoint::DeepTarget {
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: parent.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
        }),
        from_endpoint_path: None,
        to_endpoint_path: None,
        created_at: 0,
        updated_at: 0,
    };
    parent_graph.edges.push(internal_edge);
    use_store_save(dir.path(), &parent.starmap_id, &parent_graph);

    let mut parent_graph = use_store_get(dir.path(), &parent.starmap_id);
    let external_edge = crate::starmap::types::StarMapEdge {
        id: "external_e".to_string(),
        from: None,
        to: None,
        kind: crate::starmap::types::StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        from_target: None,
        to_target: None,
        from_endpoint: None,
        to_endpoint: Some(crate::starmap::types::StarMapEdgeEndpoint::DeepTarget {
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: child.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
        }),
        from_endpoint_path: None,
        to_endpoint_path: None,
        created_at: 0,
        updated_at: 0,
    };
    parent_graph.edges.push(external_edge);
    use_store_save(dir.path(), &parent.starmap_id, &parent_graph);

    let refs = find_starmap_references(dir.path(), &child.starmap_id).unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].ref_type, "edge");
    assert_eq!(refs[0].ref_id, "external_e");

    assert!(delete_starmap(dir.path(), &child.starmap_id).is_err());

    use_store_delete_edge(dir.path(), &parent.starmap_id, "external_e");

    let mut parent_graph = use_store_get(dir.path(), &parent.starmap_id);
    let external_edge_2 = crate::starmap::types::StarMapEdge {
        id: "external_e2".to_string(),
        from: None,
        to: None,
        kind: crate::starmap::types::StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        from_target: None,
        to_target: None,
        from_endpoint: Some(crate::starmap::types::StarMapEdgeEndpoint::DeepTarget {
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: parent.starmap_id.clone(),
                path: vec![crate::starmap::semantic::StarMapPathSegment::EnterChild {
                    starmap_id: child.starmap_id.clone(),
                }],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
        }),
        to_endpoint: None,
        from_endpoint_path: None,
        to_endpoint_path: None,
        created_at: 0,
        updated_at: 0,
    };
    parent_graph.edges.push(external_edge_2);
    use_store_save(dir.path(), &parent.starmap_id, &parent_graph);

    let refs2 = find_starmap_references(dir.path(), &child.starmap_id).unwrap();
    assert_eq!(refs2.len(), 1);
    assert_eq!(refs2[0].ref_type, "edge");
    assert_eq!(refs2[0].ref_id, "external_e2");

    assert!(delete_starmap(dir.path(), &child.starmap_id).is_err());

    use_store_delete_edge(dir.path(), &parent.starmap_id, "external_e2");

    assert!(delete_starmap(dir.path(), &child.starmap_id).is_ok());

    assert!(delete_starmap(dir.path(), &parent.starmap_id).is_ok());
}

#[test]
fn test_motion_policy_default() {
    let policy = crate::starmap::types::StarMapMotionPolicyDto::default();
    assert!(policy.enabled);
    assert!(policy.idle_wobble_enabled);
    assert_eq!(policy.idle_amplitude_vp, 2.0);
    assert_eq!(policy.idle_period_ms, 4200);
    assert_eq!(policy.drag_lift_scale, 1.04);
    assert_eq!(policy.settle_duration_ms, 220);
    assert!(!policy.reduce_motion);
}

#[test]
fn test_motion_policy_serialization() {
    let policy = crate::starmap::types::StarMapMotionPolicyDto::default();
    let json = serde_json::to_value(&policy).unwrap();
    let roundtrip: crate::starmap::types::StarMapMotionPolicyDto =
        serde_json::from_value(json).unwrap();
    assert_eq!(policy.enabled, roundtrip.enabled);
    assert_eq!(policy.idle_wobble_enabled, roundtrip.idle_wobble_enabled);
    assert_eq!(policy.reduce_motion, roundtrip.reduce_motion);
}

#[test]
fn test_list_starmaps_for_project_excludes_unbound() {
    let dir = setup_workspace();
    let sm_bound = create_starmap(dir.path(), "Bound", "", None).unwrap();
    let _sm_unbound = create_starmap(dir.path(), "Unbound", "", None).unwrap();
    let _sm_other = create_starmap(dir.path(), "Other", "", None).unwrap();

    bind_starmap_to_project(dir.path(), &sm_bound.starmap_id, "proj1").unwrap();
    bind_starmap_to_project(dir.path(), &_sm_other.starmap_id, "proj2").unwrap();

    let for_proj1 = list_starmaps_for_project(dir.path(), "proj1").unwrap();
    assert_eq!(for_proj1.len(), 1);
    assert_eq!(for_proj1[0].starmap_id, sm_bound.starmap_id);

    let bound_proj1 = list_starmaps_bound_to_project(dir.path(), "proj1").unwrap();
    assert_eq!(bound_proj1.len(), 1);
    assert_eq!(bound_proj1[0].starmap_id, sm_bound.starmap_id);

    let for_proj2 = list_starmaps_for_project(dir.path(), "proj2").unwrap();
    assert_eq!(for_proj2.len(), 1);
    assert_eq!(for_proj2[0].starmap_id, _sm_other.starmap_id);

    let for_nonexistent = list_starmaps_for_project(dir.path(), "no_such_project").unwrap();
    assert!(for_nonexistent.is_empty());
}
