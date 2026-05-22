use crate::mind_map::graph::MindMapGraph;
use std::fs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapIndex {
    pub schema_version: u32,
    pub default_graph_id: String,
    pub graph_ids: Vec<String>,
    pub updated_at: u64,
}

pub fn load_mind_map_graph(
    core: &crate::facade::WriterCore,
    project_id: &str,
    graph_id: Option<&str>,
) -> crate::error::Result<MindMapGraph> {
    let project_path = core.workspace_path().join("projects").join(project_id);
    if !project_path.exists() {
        return Err(crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Project not found",
        )));
    }

    let mind_map_dir = project_path.join("mind_map");
    let index_path = mind_map_dir.join("index.json");

    // Check V1 compatibility first if no graph_id is specified
    if graph_id.is_none() {
        let v1_path = project_path.join("mind_map.json");
        if v1_path.exists() {
            let json_str = fs::read_to_string(&v1_path)?;
            let graph = crate::mind_map::migration::migrate_graph_schema(&json_str, project_id)?;
            // Save to V2, propagate any errors
            save_mind_map_graph(core, &graph)?;
            // Rename V1 file to backup, propagate any errors
            let backup_path = project_path.join("mind_map.v1.backup.json");
            fs::rename(&v1_path, &backup_path)?;
            return Ok(graph);
        }
    }

    // Load V2 structures
    if let Some(gid) = graph_id {
        let graph_path = mind_map_dir.join("graphs").join(format!("{}.json", gid));
        if !graph_path.exists() {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Graph not found: {}", gid),
            )));
        }
        let json_str = fs::read_to_string(&graph_path)?;
        let graph: MindMapGraph = serde_json::from_str(&json_str)?;
        if graph.schema_version != 2 {
            return Err(crate::error::Error::Json(serde::de::Error::custom(format!("Unsupported schema version: {}", graph.schema_version))));
        }
        return Ok(graph);
    } else {
        // Find default or first graph in directory if we are not looking for a specific one
        if index_path.exists() {
            let index_str = fs::read_to_string(&index_path)?;
            let index: MindMapIndex = serde_json::from_str(&index_str).map_err(|e| {
                crate::error::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Index file corrupted: {}", e),
                ))
            })?;

            if index.schema_version != 2 {
                return Err(crate::error::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Unsupported index schema version: {}", index.schema_version),
                )));
            }

            let graph_path = mind_map_dir.join("graphs").join(format!("{}.json", index.default_graph_id));
            if !graph_path.exists() {
                return Err(crate::error::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Default graph not found: {}", index.default_graph_id),
                )));
            }
            let json_str = fs::read_to_string(&graph_path)?;
            let graph: MindMapGraph = serde_json::from_str(&json_str)?;
            if graph.schema_version != 2 {
                return Err(crate::error::Error::Json(serde::de::Error::custom(format!("Unsupported schema version: {}", graph.schema_version))));
            }
            return Ok(graph);
        }

        let graphs_dir = mind_map_dir.join("graphs");
        if graphs_dir.exists() {
            let mut json_files = Vec::new();
            for entry in fs::read_dir(graphs_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    json_files.push(path);
                }
            }

            if json_files.len() == 1 {
                let json_str = fs::read_to_string(&json_files[0])?;
                let graph: MindMapGraph = serde_json::from_str(&json_str)?;
                if graph.schema_version != 2 {
                    return Err(crate::error::Error::Json(serde::de::Error::custom(format!("Unsupported schema version: {}", graph.schema_version))));
                }
                return Ok(graph);
            } else if json_files.len() > 1 {
                return Err(crate::error::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Multiple graphs found but no index file exists to select the default one",
                )));
            }
        }
    }

    Err(crate::error::Error::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Graph not found",
    )))
}

pub fn save_mind_map_graph(
    core: &crate::facade::WriterCore,
    graph: &MindMapGraph,
) -> crate::error::Result<()> {
    // Validate first
    crate::mind_map::validation::validate_graph(graph, core).map_err(|e| {
        crate::error::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{:?}", e)))
    })?;

    let project_path = core.workspace_path().join("projects").join(&graph.project_id);
    let mind_map_dir = project_path.join("mind_map");
    let graphs_dir = mind_map_dir.join("graphs");

    fs::create_dir_all(&graphs_dir)?;

    let graph_path = graphs_dir.join(format!("{}.json", graph.id));
    let json_str = serde_json::to_string_pretty(graph)?;
    fs::write(graph_path, json_str)?;

    // Ensure index.json exists/is updated
    let index_path = mind_map_dir.join("index.json");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let index = if index_path.exists() {
        let index_str = fs::read_to_string(&index_path)?;
        let mut idx: MindMapIndex = serde_json::from_str(&index_str).map_err(|e| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Index file corrupted: {}", e),
            ))
        })?;
        idx.updated_at = now;
        if !idx.graph_ids.contains(&graph.id) {
            idx.graph_ids.push(graph.id.clone());
        }
        idx
    } else {
        MindMapIndex {
            schema_version: 2,
            default_graph_id: graph.id.clone(),
            graph_ids: vec![graph.id.clone()],
            updated_at: now,
        }
    };

    let index_str = serde_json::to_string_pretty(&index)?;
    fs::write(index_path, index_str)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::facade::WriterCore;
    use crate::mind_map::graph::{MindMapGraphNode, MindMapNodeKind};

    #[test]
    fn test_v1_migration_success() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let v1_json = r#"{
            "id": "migrated_v1",
            "nodes": [
                {
                    "id": "node_1",
                    "title": "Character A",
                    "kind": "Character",
                    "parentId": null,
                    "depth": 0,
                    "x": 10.0,
                    "y": 20.0,
                    "radius": 10.0,
                    "width": 100.0,
                    "height": 50.0,
                    "collapsed": false
                }
            ],
            "edges": []
        }"#;

        let v1_path = temp_dir.path().join("projects").join(&proj.id).join("mind_map.json");
        fs::write(&v1_path, v1_json).unwrap();

        // Load graph which should trigger V1 migration
        let loaded = load_mind_map_graph(&core, &proj.id, None).unwrap();
        assert_eq!(loaded.id, "migrated_v1");
        assert_eq!(loaded.schema_version, 2);

        // Verify V1 file is renamed/moved to mind_map.v1.backup.json
        assert!(!v1_path.exists());
        let backup_path = temp_dir.path().join("projects").join(&proj.id).join("mind_map.v1.backup.json");
        assert!(backup_path.exists());

        // Verify V2 structure exists
        let mind_map_dir = temp_dir.path().join("projects").join(&proj.id).join("mind_map");
        let graph_path = mind_map_dir.join("graphs").join("migrated_v1.json");
        assert!(graph_path.exists());

        let index_path = mind_map_dir.join("index.json");
        assert!(index_path.exists());

        let index_str = fs::read_to_string(index_path).unwrap();
        let index: MindMapIndex = serde_json::from_str(&index_str).unwrap();
        assert_eq!(index.default_graph_id, "migrated_v1");
        assert!(index.graph_ids.contains(&"migrated_v1".to_string()));
    }

    #[test]
    fn test_v1_migration_failure_preserves_file() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        // Invalid V1 schema (unsupported schema version)
        let future_json = r#"{
            "schemaVersion": 999,
            "id": "future_graph",
            "projectId": "p1",
            "title": "Future",
            "nodes": [],
            "edges": [],
            "anchors": [],
            "links": [],
            "createdAt": 0,
            "updatedAt": 0
        }"#;

        let v1_path = temp_dir.path().join("projects").join(&proj.id).join("mind_map.json");
        fs::write(&v1_path, future_json).unwrap();

        // Load graph which should fail during migration due to unsupported schema version
        let result = load_mind_map_graph(&core, &proj.id, None);
        assert!(result.is_err());

        // Verify V1 file still exists and was not renamed/removed
        assert!(v1_path.exists());
        let backup_path = temp_dir.path().join("projects").join(&proj.id).join("mind_map.v1.backup.json");
        assert!(!backup_path.exists());
    }

    #[test]
    fn test_index_created_on_save() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let graph = MindMapGraph {
            schema_version: 2,
            id: "graph_new".into(),
            project_id: proj.id.clone(),
            title: "New Graph".into(),
            nodes: vec![
                MindMapGraphNode {
                    id: "n1".into(),
                    title: "Node 1".into(),
                    kind: MindMapNodeKind::Note,
                    payload: None,
                    tags: vec![],
                    created_at: 0,
                    updated_at: 0,
                }
            ],
            edges: vec![],
            anchors: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };

        save_mind_map_graph(&core, &graph).unwrap();

        let mind_map_dir = temp_dir.path().join("projects").join(&proj.id).join("mind_map");
        let index_path = mind_map_dir.join("index.json");
        assert!(index_path.exists());

        let index_str = fs::read_to_string(index_path).unwrap();
        let index: MindMapIndex = serde_json::from_str(&index_str).unwrap();
        assert_eq!(index.default_graph_id, "graph_new");
        assert!(index.graph_ids.contains(&"graph_new".to_string()));

        // Loading without graph_id should stably fetch "graph_new"
        let loaded = load_mind_map_graph(&core, &proj.id, None).unwrap();
        assert_eq!(loaded.id, "graph_new");
    }

    #[test]
    fn test_multiple_graphs_no_index_error() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let graph1 = MindMapGraph {
            schema_version: 2,
            id: "g1".into(),
            project_id: proj.id.clone(),
            title: "G1".into(),
            nodes: vec![],
            edges: vec![],
            anchors: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let graph2 = MindMapGraph {
            schema_version: 2,
            id: "g2".into(),
            project_id: proj.id.clone(),
            title: "G2".into(),
            nodes: vec![],
            edges: vec![],
            anchors: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };

        // Write directly to bypass save_mind_map_graph (so no index.json is created)
        let graphs_dir = temp_dir.path().join("projects").join(&proj.id).join("mind_map").join("graphs");
        fs::create_dir_all(&graphs_dir).unwrap();
        fs::write(graphs_dir.join("g1.json"), serde_json::to_string(&graph1).unwrap()).unwrap();
        fs::write(graphs_dir.join("g2.json"), serde_json::to_string(&graph2).unwrap()).unwrap();

        // Loading without specifying graph_id should fail because there is no index but multiple graphs exist
        let result = load_mind_map_graph(&core, &proj.id, None);
        assert!(result.is_err());
        if let Err(crate::error::Error::Io(err)) = result {
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
            assert!(err.to_string().contains("Multiple graphs found"));
        } else {
            panic!("Expected Io error with InvalidInput kind");
        }
    }

    #[test]
    fn test_index_default_graph_id_stable() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let graph1 = MindMapGraph {
            schema_version: 2,
            id: "g1".into(),
            project_id: proj.id.clone(),
            title: "G1".into(),
            nodes: vec![],
            edges: vec![],
            anchors: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let graph2 = MindMapGraph {
            schema_version: 2,
            id: "g2".into(),
            project_id: proj.id.clone(),
            title: "G2".into(),
            nodes: vec![],
            edges: vec![],
            anchors: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };

        save_mind_map_graph(&core, &graph1).unwrap();
        // Saving graph2 shouldn't overwrite the default graph id if index already exists
        save_mind_map_graph(&core, &graph2).unwrap();

        let loaded = load_mind_map_graph(&core, &proj.id, None).unwrap();
        assert_eq!(loaded.id, "g1"); // default graph should still be g1

        // Specifically requesting g2 should load g2 successfully
        let loaded_g2 = load_mind_map_graph(&core, &proj.id, Some("g2")).unwrap();
        assert_eq!(loaded_g2.id, "g2");
    }

    #[test]
    fn test_corrupted_index_returns_error() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let mind_map_dir = temp_dir.path().join("projects").join(&proj.id).join("mind_map");
        fs::create_dir_all(&mind_map_dir).unwrap();
        fs::write(mind_map_dir.join("index.json"), "{ invalid json }").unwrap();

        let result = load_mind_map_graph(&core, &proj.id, None);
        assert!(result.is_err());
        if let Err(crate::error::Error::Io(err)) = result {
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
            assert!(err.to_string().contains("Index file corrupted"));
        } else {
            panic!("Expected Io error with InvalidData kind");
        }
    }

    #[test]
    fn test_unsupported_index_schema_returns_error() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let mind_map_dir = temp_dir.path().join("projects").join(&proj.id).join("mind_map");
        fs::create_dir_all(&mind_map_dir).unwrap();
        let bad_index = r#"{
            "schemaVersion": 999,
            "defaultGraphId": "g1",
            "graphIds": ["g1"],
            "updatedAt": 0
        }"#;
        fs::write(mind_map_dir.join("index.json"), bad_index).unwrap();

        let result = load_mind_map_graph(&core, &proj.id, None);
        assert!(result.is_err());
        if let Err(crate::error::Error::Io(err)) = result {
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
            assert!(err.to_string().contains("Unsupported index schema version"));
        } else {
            panic!("Expected Io error with InvalidData kind");
        }
    }

    #[test]
    fn test_default_graph_not_found_returns_error() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let mind_map_dir = temp_dir.path().join("projects").join(&proj.id).join("mind_map");
        fs::create_dir_all(&mind_map_dir).unwrap();
        let index = r#"{
            "schemaVersion": 2,
            "defaultGraphId": "nonexistent",
            "graphIds": ["nonexistent"],
            "updatedAt": 0
        }"#;
        fs::write(mind_map_dir.join("index.json"), index).unwrap();

        let result = load_mind_map_graph(&core, &proj.id, None);
        assert!(result.is_err());
        if let Err(crate::error::Error::Io(err)) = result {
            assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
            assert!(err.to_string().contains("Default graph not found"));
        } else {
            panic!("Expected Io error with NotFound kind");
        }
    }
}
