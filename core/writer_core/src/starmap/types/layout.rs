use serde::{Deserialize, Serialize};

fn default_scale() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StarMapLayoutKind {
    Freeform,
    AutoRadial,
    #[serde(other)]
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLayout {
    pub kind: StarMapLayoutKind,
    pub nodes: Vec<StarMapLayoutNode>,
}

impl Default for StarMapLayout {
    fn default() -> Self {
        Self {
            kind: StarMapLayoutKind::Freeform,
            nodes: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLayoutNode {
    pub node_id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub collapsed: bool,
    pub z_index: i32,

    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default)]
    pub depth: f32,
    #[serde(default)]
    pub focus_weight: f32,
    #[serde(default)]
    pub orbit_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapViewport {
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default)]
    pub offset_x: f32,
    #[serde(default)]
    pub offset_y: f32,
    #[serde(default)]
    pub width: f32,
    #[serde(default)]
    pub height: f32,
}

impl Default for StarMapViewport {
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            width: 0.0,
            height: 0.0,
        }
    }
}