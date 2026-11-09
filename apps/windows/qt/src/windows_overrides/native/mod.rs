//! Windows Qt 原生桥接层。
//!
//! 复用星图 QSG 图层；Windows 只新增系统深浅色应用桥与渲染宿主。

mod kde_color_scheme;
pub mod starmap_circle_clip;
pub mod starmap_edge_layer;
pub mod starmap_grid_item;
pub mod starmap_object_layer;
pub mod windows_render_host;

/// 星图渲染诊断统计（仅计数，不含正文）。
pub fn starmap_render_statistics_json() -> String {
    let edge =
        serde_json::from_str::<serde_json::Value>(&starmap_edge_layer::render_statistics_json())
            .unwrap_or(serde_json::Value::Null);
    let object =
        serde_json::from_str::<serde_json::Value>(&starmap_object_layer::render_statistics_json())
            .unwrap_or(serde_json::Value::Null);
    let grid =
        serde_json::from_str::<serde_json::Value>(&starmap_grid_item::render_statistics_json())
            .unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "edgeLayer": edge,
        "objectLayer": object,
        "grid": grid,
        "renderHost": windows_render_host::render_statistics_json(),
    })
    .to_string()
}

pub(crate) use kde_color_scheme::apply_appearance_mode;
