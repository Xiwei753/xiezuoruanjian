#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapMotionPolicyDto {
    pub enabled: bool,
    pub idle_wobble_enabled: bool,
    pub idle_amplitude_vp: f32,
    pub idle_period_ms: u32,
    pub drag_lift_scale: f32,
    pub drag_shadow_boost: f32,
    pub settle_duration_ms: u32,
    pub reduce_motion: bool,
}

impl From<crate::starmap::types::StarMapMotionPolicyDto> for StarMapMotionPolicyDto {
    fn from(p: crate::starmap::types::StarMapMotionPolicyDto) -> Self {
        Self {
            enabled: p.enabled,
            idle_wobble_enabled: p.idle_wobble_enabled,
            idle_amplitude_vp: p.idle_amplitude_vp,
            idle_period_ms: p.idle_period_ms,
            drag_lift_scale: p.drag_lift_scale,
            drag_shadow_boost: p.drag_shadow_boost,
            settle_duration_ms: p.settle_duration_ms,
            reduce_motion: p.reduce_motion,
        }
    }
}

impl From<StarMapMotionPolicyDto> for crate::starmap::types::StarMapMotionPolicyDto {
    fn from(d: StarMapMotionPolicyDto) -> Self {
        Self {
            enabled: d.enabled,
            idle_wobble_enabled: d.idle_wobble_enabled,
            idle_amplitude_vp: d.idle_amplitude_vp,
            idle_period_ms: d.idle_period_ms,
            drag_lift_scale: d.drag_lift_scale,
            drag_shadow_boost: d.drag_shadow_boost,
            settle_duration_ms: d.settle_duration_ms,
            reduce_motion: d.reduce_motion,
        }
    }
}
