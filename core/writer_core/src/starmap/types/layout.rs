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

/// 星图运动策略 DTO — 跨端共享的动画策略参数
///
/// 这只是策略参数，不是逐帧坐标。各端根据此策略在本地计算视觉偏移。
/// idle wobble 只是视觉偏移，不能改 layout.x/y。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapMotionPolicyDto {
    /// 是否启用动画
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 是否启用静置摇晃
    #[serde(default = "default_true")]
    pub idle_wobble_enabled: bool,
    /// 静置摇晃振幅（vp）
    #[serde(default = "default_idle_amplitude_vp")]
    pub idle_amplitude_vp: f32,
    /// 静置摇晃周期（ms）
    #[serde(default = "default_idle_period_ms")]
    pub idle_period_ms: u32,
    /// 拖动抬起缩放
    #[serde(default = "default_drag_lift_scale")]
    pub drag_lift_scale: f32,
    /// 拖动阴影增强
    #[serde(default = "default_drag_shadow_boost")]
    pub drag_shadow_boost: f32,
    /// 放置后归位动画时长（ms）
    #[serde(default = "default_settle_duration_ms")]
    pub settle_duration_ms: u32,
    /// 减少动态效果（无障碍）
    #[serde(default)]
    pub reduce_motion: bool,
}

fn default_true() -> bool { true }
fn default_idle_amplitude_vp() -> f32 { 2.0 }
fn default_idle_period_ms() -> u32 { 4200 }
fn default_drag_lift_scale() -> f32 { 1.04 }
fn default_drag_shadow_boost() -> f32 { 8.0 }
fn default_settle_duration_ms() -> u32 { 220 }

impl Default for StarMapMotionPolicyDto {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_wobble_enabled: true,
            idle_amplitude_vp: 2.0,
            idle_period_ms: 4200,
            drag_lift_scale: 1.04,
            drag_shadow_boost: 8.0,
            settle_duration_ms: 220,
            reduce_motion: false,
        }
    }
}