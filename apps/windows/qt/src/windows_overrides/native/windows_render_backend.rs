//! Windows Qt Quick 渲染后端诊断。
//!
//! 实际绘制全部由共享 `SujianEditorItem` / `StarMapEdgeLayer` /
//! `StarMapObjectLayer` / `StarMapGridItem` 的 QSG 路径完成。这里不再创建
//! 空的 `QQuickItem`，只读取 `app_main_cpp::install_graphics_runtime_probe()`
//! 从真实 `QQuickWindow::graphicsApi()` 采到的后端，并核对 D3D11 所需
//! HLSL 着色器是否已编入 QRC。

use serde_json::{json, Value};

/// 真实 RHI 后端与着色器打包状态（只含诊断信息，不含正文）。
pub fn render_backend_json() -> String {
    let sampled: Value = serde_json::from_str(&crate::app_main_cpp::graphics_runtime_info())
        .unwrap_or_else(|_| json!({}));
    let graphics_api = sampled
        .get("graphicsApi")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let shaders = [
        ":/shaders/starmap_edge.vert.qsb",
        ":/shaders/starmap_edge.frag.qsb",
        ":/shaders/starmap_object.vert.qsb",
        ":/shaders/starmap_object.frag.qsb",
        ":/shaders/starmap_grid.vert.qsb",
        ":/shaders/starmap_grid.frag.qsb",
    ];
    let missing: Vec<&str> = shaders
        .into_iter()
        .filter(|path| !crate::app_main_cpp::qrc_resource_exists(path))
        .collect();

    json!({
        "graphicsApi": graphics_api,
        "rhiSample": sampled,
        "hlslShadersBundled": missing.is_empty(),
        "missingShaders": missing,
        "direct3dReady": matches!(graphics_api, "d3d11" | "d3d12") && missing.is_empty(),
    })
    .to_string()
}
