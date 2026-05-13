use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraphScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraphViewMode {
    Flat,
    Planet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraphNodeType {
    Book,
    Character,
    Organization,
    Location,
    Event,
    Concept,
    Chapter,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraphEdgeType {
    Relationship,
    Bloodline,
    Faction,
    Hostile,
    Dependency,
    Causality,
    Foreshadowing,
    Timeline,
    Emotion,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraphArrowType {
    None,
    OneWay,
    TwoWay,
    Dashed,
    Dotted,
    Thick,
    Weak,
    Strong,
    Conflict,
    Hidden,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position2D {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub node_type: GraphNodeType,
    pub position2d: Option<Position2D>,
    pub position3d: Option<Position3D>,
    pub group: Option<String>,
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub linked_project_id: Option<String>,
    pub linked_character_id: Option<String>,
    pub linked_chapter_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub edge_type: GraphEdgeType,
    pub arrow_type: GraphArrowType,
    pub label: Option<String>,
    pub description: Option<String>,
    pub note: Option<String>,
    pub color: Option<String>,
    pub weight: Option<f64>,
    pub direction: Option<String>,
    pub style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphDocument {
    pub version: u32,
    pub scope: GraphScope,
    pub view_mode: GraphViewMode,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl GraphDocument {
    pub fn new(scope: GraphScope) -> Self {
        Self {
            version: 1,
            scope,
            view_mode: GraphViewMode::Flat,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

pub struct GraphService {
    workspace_path: PathBuf,
}

impl GraphService {
    pub fn new<P: AsRef<Path>>(workspace_path: P) -> Self {
        Self {
            workspace_path: workspace_path.as_ref().to_path_buf(),
        }
    }

    fn get_graph_file_path(&self, project_id: Option<&str>) -> PathBuf {
        if let Some(pid) = project_id {
            self.workspace_path
                .join("projects")
                .join(pid)
                .join("graph.json")
        } else {
            self.workspace_path.join("global_graph.json")
        }
    }

    pub fn load_graph(&self, project_id: Option<&str>) -> crate::Result<GraphDocument> {
        let path = self.get_graph_file_path(project_id);
        if !path.exists() {
            let scope = if project_id.is_some() {
                GraphScope::Project
            } else {
                GraphScope::Global
            };
            return Ok(GraphDocument::new(scope));
        }

        let content = fs::read_to_string(&path).map_err(crate::Error::Io)?;

        let doc: GraphDocument = serde_json::from_str(&content).map_err(crate::Error::Json)?;

        Ok(doc)
    }

    pub fn save_graph(&self, project_id: Option<&str>, doc: &GraphDocument) -> crate::Result<()> {
        let path = self.get_graph_file_path(project_id);

        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(crate::Error::Io)?;
            }
        }

        let temp_dir = path.parent().unwrap_or(Path::new(""));
        let mut temp_file = tempfile::Builder::new()
            .prefix("graph_")
            .suffix(".tmp")
            .tempfile_in(temp_dir)
            .map_err(crate::Error::Io)?;

        let json = serde_json::to_string_pretty(doc).map_err(crate::Error::Json)?;

        use std::io::Write;
        temp_file
            .write_all(json.as_bytes())
            .map_err(crate::Error::Io)?;
        temp_file.flush().map_err(crate::Error::Io)?;

        temp_file
            .persist(&path)
            .map_err(|e| crate::Error::Io(e.into()))?;

        Ok(())
    }

    pub fn generate_graph(&self) -> crate::Result<()> {
        Err(crate::Error::NotImplemented)
    }
}
