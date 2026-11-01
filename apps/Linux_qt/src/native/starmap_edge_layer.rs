// =============================================================================
// native/starmap_edge_layer.rs — 星图连线批量场景图图层（Issue #1029）
// =============================================================================
//
// 层级：Linux_qt 平台可视组件（QML 原生类型 `Sujian.StarMapEdgeLayer`）。
//
// 背景：旧实现为每条边创建一个全屏 Item + 两个 ShapePath，254 条边在每次
// 平移/缩放时都会各自重算 PathPolyline 并触发场景图重建。这里把“所有未选中边
// + 少量选中边”合并成两个 QSGGeometryNode，线宽/箭头全部用三角形批量绘制。
//
// 评论修正：#1029 要求相机变化不再上传整张图的顶点。现在几何顶点相对
// `anchorX/anchorY`（图内容中心附近）缓存，平移只更新 QSGTransformNode 的
// 矩阵，不 allocate / memcpy；缩放才重算屏幕口径的线宽和箭头大小。顶点数组
// 只在结构/节点位置/选择/缩放变化时重建，并分别记录 geometry/camera 统计。
//
// 线程约束：QML 属性在 GUI 线程写入；顶点数组只在同步阶段（GUI 线程被阻塞）
// 的 update_paint_node() 中读取。QSGNode / QSGGeometry / QSGMaterial 只在
// update_paint_node() 里创建或修改。
// =============================================================================

use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::scenegraph::{ContainerNode, SGNode};
use std::os::raw::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::starmap_view::edge_render::EdgeRender;

static EDGE_GEOMETRY_UPDATES: AtomicU64 = AtomicU64::new(0);
static EDGE_VERTEX_UPLOADS: AtomicU64 = AtomicU64::new(0);
static EDGE_CAMERA_UPDATES: AtomicU64 = AtomicU64::new(0);

/// 诊断用渲染统计：只含计数，不含星图正文。
pub fn render_statistics_json() -> String {
    serde_json::json!({
        "edgeLayerGeometryUpdates": EDGE_GEOMETRY_UPDATES.load(Ordering::Relaxed),
        "edgeLayerVertexUploads": EDGE_VERTEX_UPLOADS.load(Ordering::Relaxed),
        "edgeLayerCameraUpdates": EDGE_CAMERA_UPDATES.load(Ordering::Relaxed),
    })
    .to_string()
}

cpp! {{
    #include <qmetaobject_rust.hpp>
    #include <QtQuick/QQuickItem>
    #include <QtQuick/QSGNode>
    #include <QtQuick/QSGGeometryNode>
    #include <QtQuick/QSGGeometry>
    #include <QtQuick/QSGFlatColorMaterial>
    #include <QtQuick/QSGTransformNode>
    #include <QtGui/QColor>
    #include <QtGui/QMatrix4x4>
    #include <cstring>
    #include <cmath>

    static QSGTransformNode *sujian_edge_transform_node(QSGNode *root)
    {
        if (!root)
            return nullptr;
        QSGNode *child = root->firstChild();
        if (child && child->type() == QSGNode::TransformNodeType)
            return static_cast<QSGTransformNode *>(child);
        QSGTransformNode *transform = new QSGTransformNode;
        transform->setFlag(QSGNode::OwnedByParent, true);
        root->appendChildNode(transform);
        return transform;
    }

    static QSGGeometryNode *sujian_edge_geometry_node(QSGTransformNode *transform, int index)
    {
        if (!transform)
            return nullptr;
        QSGNode *child = transform->firstChild();
        while (child && index > 0) {
            child = child->nextSibling();
            --index;
        }
        if (child && child->type() == QSGNode::GeometryNodeType)
            return static_cast<QSGGeometryNode *>(child);
        return nullptr;
    }

    static void sujian_apply_edge_material(QSGGeometryNode *node,
                                            int r, int g, int b, double alpha)
    {
        QSGMaterial *existing = node->material();
        QSGFlatColorMaterial *material =
            existing ? static_cast<QSGFlatColorMaterial *>(existing) : nullptr;
        if (!material) {
            material = new QSGFlatColorMaterial;
            node->setMaterial(material);
            node->setFlag(QSGNode::OwnsMaterial, true);
        }
        QColor color(r, g, b);
        color.setAlphaF(alpha);
        material->setColor(color);
        node->markDirty(QSGNode::DirtyMaterial);
    }

    static void sujian_apply_edge_geometry(QSGGeometryNode *node,
                                           const float *vertices, int vertex_count,
                                           int r, int g, int b, double alpha)
    {
        QSGGeometry *geometry = node->geometry();
        if (!geometry) {
            geometry = new QSGGeometry(QSGGeometry::defaultAttributes_Point2D(),
                                       vertex_count);
            geometry->setDrawingMode(QSGGeometry::DrawTriangles);
            node->setGeometry(geometry);
            node->setFlag(QSGNode::OwnsGeometry, true);
        } else if (geometry->vertexCount() != vertex_count) {
            geometry->allocate(vertex_count);
        }
        if (vertex_count > 0 && vertices)
            std::memcpy(geometry->vertexData(), vertices,
                        size_t(vertex_count) * 2 * sizeof(float));
        geometry->markVertexDataDirty();
        node->markDirty(QSGNode::DirtyGeometry);

        sujian_apply_edge_material(node, r, g, b, alpha);
    }

    static void sujian_starmap_edges_update(QSGNode *root,
                                            double translate_x, double translate_y,
                                            bool geometry_dirty,
                                            const float *normal_vertices,
                                            int normal_vertex_count,
                                            const float *selected_vertices,
                                            int selected_vertex_count,
                                            int nr, int ng, int nb, double na,
                                            int sr, int sg, int sb, double sa)
    {
        QSGTransformNode *transform = sujian_edge_transform_node(root);
        if (!transform)
            return;

        QMatrix4x4 matrix;
        matrix.translate(float(translate_x), float(translate_y));
        transform->setMatrix(matrix);
        transform->markDirty(QSGNode::DirtyMatrix);

        QSGGeometryNode *normal = sujian_edge_geometry_node(transform, 0);
        if (!normal) {
            normal = new QSGGeometryNode;
            normal->setFlag(QSGNode::OwnedByParent, true);
            transform->appendChildNode(normal);
        }
        QSGGeometryNode *selected = sujian_edge_geometry_node(transform, 1);
        if (!selected) {
            selected = new QSGGeometryNode;
            selected->setFlag(QSGNode::OwnedByParent, true);
            transform->appendChildNode(selected);
        }
        if (geometry_dirty) {
            sujian_apply_edge_geometry(normal, normal_vertices, normal_vertex_count,
                                       nr, ng, nb, na);
            sujian_apply_edge_geometry(selected, selected_vertices,
                                       selected_vertex_count, sr, sg, sb, sa);
        } else {
            // 相机帧只更新颜色透明度（reveal），不触碰顶点或几何脏标记。
            sujian_apply_edge_material(normal, nr, ng, nb, na);
            sujian_apply_edge_material(selected, sr, sg, sb, sa);
        }
    }
}}

/// 星图连线批量图层（QML 类型 `Sujian.StarMapEdgeLayer`）。
#[derive(QObject, Default)]
pub struct StarMapEdgeLayer {
    base: qt_base_class!(trait QQuickItem),

    /// `EdgeRender` 数组 JSON（camelCase，与 Rust edge_render 输出一致）。
    edges_json: qt_property!(QString; ALIAS edgesJson WRITE set_edges_json),
    selected_edge_id: qt_property!(QString; ALIAS selectedEdgeId WRITE set_selected_edge_id),
    zoom_level: qt_property!(f64; ALIAS zoomLevel WRITE set_zoom_level),
    pan_x: qt_property!(f64; ALIAS panX WRITE set_pan_x),
    pan_y: qt_property!(f64; ALIAS panY WRITE set_pan_y),
    anchor_x: qt_property!(f64; ALIAS anchorX WRITE set_anchor_x),
    anchor_y: qt_property!(f64; ALIAS anchorY WRITE set_anchor_y),
    reveal: qt_property!(f64; ALIAS reveal WRITE set_reveal),
    line_world_width: qt_property!(f64; ALIAS lineWorldWidth WRITE set_line_world_width),
    normal_color_hex: qt_property!(QString; ALIAS normalColorHex WRITE set_normal_color_hex),
    selected_color_hex: qt_property!(QString; ALIAS selectedColorHex WRITE set_selected_color_hex),

    edges: Vec<EdgeRender>,
    geometry_dirty: bool,
}

impl StarMapEdgeLayer {
    fn request_paint(&mut self) {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return;
        }
        // SAFETY: obj 是本组件 QQuickItem 的 C++ 指针；update() 只安排场景图同步。
        cpp!(unsafe [obj as "QQuickItem *"] {
            if (obj)
                obj->update();
        });
    }

    fn mark_geometry_dirty(&mut self) {
        self.geometry_dirty = true;
        self.request_paint();
    }

    fn set_edges_json(&mut self, value: QString) {
        if self.edges_json == value {
            return;
        }
        self.edges_json = value.clone();
        let raw = value.to_string();
        self.edges = serde_json::from_str(&raw).unwrap_or_else(|err| {
            log::warn!("StarMapEdgeLayer invalid edges JSON: {err}");
            Vec::new()
        });
        self.mark_geometry_dirty();
    }

    fn set_selected_edge_id(&mut self, value: QString) {
        if self.selected_edge_id == value {
            return;
        }
        self.selected_edge_id = value;
        self.mark_geometry_dirty();
    }

    fn set_zoom_level(&mut self, value: f64) {
        if self.zoom_level == value {
            return;
        }
        self.zoom_level = value;
        self.mark_geometry_dirty();
    }

    fn set_pan_x(&mut self, value: f64) {
        if self.pan_x == value {
            return;
        }
        self.pan_x = value;
        self.request_paint();
    }

    fn set_pan_y(&mut self, value: f64) {
        if self.pan_y == value {
            return;
        }
        self.pan_y = value;
        self.request_paint();
    }

    fn set_anchor_x(&mut self, value: f64) {
        if self.anchor_x == value {
            return;
        }
        self.anchor_x = value;
        self.mark_geometry_dirty();
    }

    fn set_anchor_y(&mut self, value: f64) {
        if self.anchor_y == value {
            return;
        }
        self.anchor_y = value;
        self.mark_geometry_dirty();
    }

    fn set_reveal(&mut self, value: f64) {
        if self.reveal == value {
            return;
        }
        self.reveal = value;
        self.request_paint();
    }

    fn set_line_world_width(&mut self, value: f64) {
        if self.line_world_width == value {
            return;
        }
        self.line_world_width = value;
        self.mark_geometry_dirty();
    }

    fn set_normal_color_hex(&mut self, value: QString) {
        if self.normal_color_hex == value {
            return;
        }
        self.normal_color_hex = value;
        self.request_paint();
    }

    fn set_selected_color_hex(&mut self, value: QString) {
        if self.selected_color_hex == value {
            return;
        }
        self.selected_color_hex = value;
        self.request_paint();
    }

    fn item_size(&self) -> (f64, f64) {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return (0.0, 0.0);
        }
        // SAFETY: obj 是有效的 QQuickItem 指针；只读 width()/height()。
        let w = cpp!(unsafe [obj as "QQuickItem *"] -> f64 as "double" {
            return obj ? obj->width() : 0.0;
        });
        let h = cpp!(unsafe [obj as "QQuickItem *"] -> f64 as "double" {
            return obj ? obj->height() : 0.0;
        });
        (w, h)
    }

    fn color_parts(hex: &str) -> (i32, i32, i32) {
        let color = qmetaobject::QColor::from_name(hex);
        (color.red(), color.green(), color.blue())
    }

    /// 顶点坐标相对 anchor：`(world - anchor) * zoom`。平移不改变这些值，
    /// 只改变 transform 的视口平移；缩放时线宽/箭头按屏幕像素重算。
    fn build_vertices(&self) -> (Vec<f32>, Vec<f32>) {
        let zoom = self.zoom_level;
        if !(zoom > 0.0) || !zoom.is_finite() {
            return (Vec::new(), Vec::new());
        }
        let anchor_x = self.anchor_x;
        let anchor_y = self.anchor_y;
        if !anchor_x.is_finite() || !anchor_y.is_finite() {
            return (Vec::new(), Vec::new());
        }
        let half_w = (self.line_world_width * zoom).clamp(1.0, 6.0) / 2.0;
        let selected_id = self.selected_edge_id.to_string();
        let mut normal = Vec::with_capacity(self.edges.len() * 9);
        let mut selected = Vec::with_capacity(64);

        let project = |wx: f32, wy: f32| -> (f32, f32) {
            (
                ((f64::from(wx) - anchor_x) * zoom) as f32,
                ((f64::from(wy) - anchor_y) * zoom) as f32,
            )
        };

        for edge in &self.edges {
            let target = if !selected_id.is_empty() && edge.edge_id == selected_id {
                &mut selected
            } else {
                &mut normal
            };
            let (sx, sy) = project(edge.start_x, edge.start_y);
            let (ex, ey) = project(edge.end_x, edge.end_y);
            let dx = ex - sx;
            let dy = ey - sy;
            let len = (dx * dx + dy * dy).sqrt();
            if !len.is_finite() || len < 0.5 {
                continue;
            }
            let nx = -dy / len * half_w as f32;
            let ny = dx / len * half_w as f32;
            let p1 = (sx + nx, sy + ny);
            let p2 = (sx - nx, sy - ny);
            let p3 = (ex + nx, ey + ny);
            let p4 = (ex - nx, ey - ny);
            target.extend_from_slice(&[
                p1.0, p1.1, p2.0, p2.1, p3.0, p3.1, p3.0, p3.1, p2.0, p2.1, p4.0, p4.1,
            ]);

            let (tx, ty) = project(edge.arrow_tip_x, edge.arrow_tip_y);
            let (lx, ly) = project(edge.arrow_left_x, edge.arrow_left_y);
            let (rx, ry) = project(edge.arrow_right_x, edge.arrow_right_y);
            target.extend_from_slice(&[tx, ty, lx, ly, rx, ry]);
        }
        (normal, selected)
    }

    /// anchor 当前投影到的屏幕坐标。相机近 anchor 时数值保持视口量级，
    /// 保留双精度换算结果后只把小的平移交给场景图。
    fn camera_translation(&self) -> (f64, f64) {
        let (width, height) = self.item_size();
        let zoom = self.zoom_level;
        if !(zoom > 0.0) || !zoom.is_finite() || width <= 0.0 || height <= 0.0 {
            return (0.0, 0.0);
        }
        let origin_x = (width / 2.0 - self.pan_x) / zoom;
        let origin_y = (height / 2.0 - self.pan_y) / zoom;
        (
            width / 2.0 + (self.anchor_x - origin_x) * zoom,
            height / 2.0 + (self.anchor_y - origin_y) * zoom,
        )
    }
}

impl QQuickItem for StarMapEdgeLayer {
    fn component_complete(&mut self) {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return;
        }
        // SAFETY: obj 是有效 QQuickItem 指针；ItemHasContents 启用 updatePaintNode。
        cpp!(unsafe [obj as "QQuickItem *"] {
            if (obj) {
                obj->setFlag(QQuickItem::ItemHasContents, true);
                obj->setClip(true);
            }
        });
        self.geometry_dirty = true;
        self.request_paint();
    }

    fn geometry_changed(&mut self, _new_geometry: QRectF, _old_geometry: QRectF) {
        self.request_paint();
    }

    fn update_paint_node(&mut self, node: SGNode<ContainerNode>) -> SGNode<ContainerNode> {
        let mut raw = node.into_raw();
        if raw.is_null() {
            // SAFETY: 空占位节点由父节点回收。
            raw = cpp!(unsafe [] -> *mut c_void as "QSGNode *" {
                QSGNode *placeholder = new QSGNode;
                placeholder->setFlag(QSGNode::OwnedByParent);
                return placeholder;
            });
        }

        let geometry_dirty = self.geometry_dirty;
        let (normal, selected) = if geometry_dirty {
            self.build_vertices()
        } else {
            (Vec::new(), Vec::new())
        };
        let normal_ptr = normal.as_ptr();
        let selected_ptr = selected.as_ptr();
        let normal_count = (normal.len() / 2) as i32;
        let selected_count = (selected.len() / 2) as i32;
        let (tx, ty) = self.camera_translation();
        let (nr, ng, nb) = Self::color_parts(&self.normal_color_hex.to_string());
        let (sr, sg, sb) = Self::color_parts(&self.selected_color_hex.to_string());
        let reveal = if self.reveal.is_finite() {
            self.reveal.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if geometry_dirty {
            EDGE_GEOMETRY_UPDATES.fetch_add(1, Ordering::Relaxed);
            EDGE_VERTEX_UPLOADS.fetch_add(
                (normal_count.max(0) + selected_count.max(0)) as u64,
                Ordering::Relaxed,
            );
        } else {
            EDGE_CAMERA_UPDATES.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: raw 是本 item 渲染线程上的 paint node；两个顶点数组在调用期间
        // 保持有效，C++ 只在本函数内拷贝，不保留 Rust 指针。
        cpp!(unsafe [
            raw as "QSGNode *",
            tx as "double", ty as "double", geometry_dirty as "bool",
            normal_ptr as "const float *",
            normal_count as "int",
            selected_ptr as "const float *",
            selected_count as "int",
            nr as "int", ng as "int", nb as "int", reveal as "double",
            sr as "int", sg as "int", sb as "int"
        ] {
            sujian_starmap_edges_update(raw, tx, ty, geometry_dirty,
                                        normal_ptr, normal_count,
                                        selected_ptr, selected_count,
                                        nr, ng, nb, reveal, sr, sg, sb, reveal);
        });
        self.geometry_dirty = false;
        // SAFETY: raw 是刚取回所有权的节点指针，交给 Qt 继续持有。
        unsafe { SGNode::from_raw(raw) }
    }
}
