// =============================================================================
// native/mod.rs — Linux_qt 原生（C++/KF6）桥接层
// =============================================================================
//
// 集中存放必须走 C++/KF6 的原生调用，避免把 KF6 依赖散落到业务模块。
// 当前包含 KDE 应用配色方案切换桥（KColorSchemeManager）与星图穿越过渡的
// 平台侧圆形裁剪容器（Issue #949 复核）。
// =============================================================================

mod kde_color_scheme;
/// Issue #949 复核：过渡期把子场景 Loader 的可见区域裁到过渡圆环内的平台组件。
pub mod starmap_circle_clip;
/// Issue #1029：批量连线场景图图层与背景点阵图层。
pub mod starmap_edge_layer;
pub mod starmap_grid_item;

/// 星图渲染诊断统计（仅计数，不含正文）。
pub fn starmap_render_statistics_json() -> String {
    let edge =
        serde_json::from_str::<serde_json::Value>(&starmap_edge_layer::render_statistics_json())
            .unwrap_or(serde_json::Value::Null);
    let grid =
        serde_json::from_str::<serde_json::Value>(&starmap_grid_item::render_statistics_json())
            .unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "edgeLayer": edge,
        "grid": grid,
    })
    .to_string()
}

pub(crate) use kde_color_scheme::apply_appearance_mode;
