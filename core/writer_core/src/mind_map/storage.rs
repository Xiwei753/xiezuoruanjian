use crate::mind_map::graph::MindMapGraph;
use std::fs;

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

    // Check V1 compatibility first if no graph_id is specified
    if graph_id.is_none() {
        let v1_path = project_path.join("mind_map.json");
        if v1_path.exists() {
            let json_str = fs::read_to_string(&v1_path)?;
            let graph = crate::mind_map::migration::migrate_graph_schema(&json_str, project_id)?;
            // If we successfully migrated, we should technically save it in V2 structure and maybe delete V1
            // Save it now so subsequent loads use V2 directly
            let _ = save_mind_map_graph(core, &graph);
            // Optionally remove V1, but the prompt says: "读取旧文件后迁移成新结构"
            let _ = fs::remove_file(&v1_path);
            return Ok(graph);
        }
    }

    // Load V2 structures
    if let Some(gid) = graph_id {
        let graph_path = mind_map_dir.join("graphs").join(format!("{}.json", gid));
        if graph_path.exists() {
            let json_str = fs::read_to_string(&graph_path)?;
            let graph: MindMapGraph = serde_json::from_str(&json_str)?;
            if graph.schema_version != 2 {
                return Err(crate::error::Error::Json(serde::de::Error::custom(format!("Unsupported schema version: {}", graph.schema_version))));
            }
            return Ok(graph);
        }
    } else {
        // Find default or first graph in directory if we are not looking for a specific one
        let graphs_dir = mind_map_dir.join("graphs");
        if graphs_dir.exists() {
            for entry in fs::read_dir(graphs_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    let json_str = fs::read_to_string(&path)?;
                    let graph: MindMapGraph = serde_json::from_str(&json_str)?;
                    if graph.schema_version != 2 {
                        continue;
                    }
                    return Ok(graph);
                }
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
    let graphs_dir = project_path.join("mind_map").join("graphs");

    fs::create_dir_all(&graphs_dir)?;

    let graph_path = graphs_dir.join(format!("{}.json", graph.id));
    let json_str = serde_json::to_string_pretty(graph)?;
    fs::write(graph_path, json_str)?;

    Ok(())
}
