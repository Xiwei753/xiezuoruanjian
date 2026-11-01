// =============================================================================
// native/starmap_grid_item.rs — 星图背景点阵场景图图层（Issue #1029）
// =============================================================================
//
// 层级：Linux_qt 平台可视组件（QML 原生类型 `Sujian.StarMapGridItem`）。
//
// 旧实现用 QML Canvas 画点阵，并在每次 pan/zoom 时 requestPaint：每帧都要
// CPU 生成一张与视口等大的 QImage，再作为纹理上传 GPU。这里把点阵变成单个
// QSGGeometryNode：只保存屏幕空间的小方块三角形，相机变化时更新顶点，
// 不再创建/上传整幅位图。LOD 规则与旧 Canvas 保持一致（屏幕间距 >= 16px，
// world 间距按 5 倍档抬升）。
// =============================================================================

use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::scenegraph::{ContainerNode, SGNode};
use std::os::raw::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

static GRID_GEOMETRY_UPDATES: AtomicU64 = AtomicU64::new(0);
static GRID_VERTEX_UPLOADS: AtomicU64 = AtomicU64::new(0);

/// 诊断用渲染统计：只含计数，不含星图正文。
pub fn render_statistics_json() -> String {
    serde_json::json!({
        "gridGeometryUpdates": GRID_GEOMETRY_UPDATES.load(Ordering::Relaxed),
        "gridVertexUploads": GRID_VERTEX_UPLOADS.load(Ordering::Relaxed),
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
    #include <QtGui/QColor>
    #include <cstring>

    static void sujian_starmap_grid_update(QSGNode *root,
                                           const float *vertices, int vertex_count,
                                           int r, int g, int b, double alpha)
    {
        QSGNode *first = root ? root->firstChild() : nullptr;
        QSGGeometryNode *node = (first && first->type() == QSGNode::GeometryNodeType)
            ? static_cast<QSGGeometryNode *>(first) : nullptr;
        if (!node) {
            node = new QSGGeometryNode;
            node->setFlag(QSGNode::OwnedByParent, true);
            root->appendChildNode(node);
        }

        QSGGeometry *geometry = node->geometry();
        if (!geometry) {
            geometry = new QSGGeometry(QSGGeometry::defaultAttributes_Point2D(),
                                       vertex_count);
            geometry->setDrawingMode(QSGGeometry::DrawTriangles);
            node->setGeometry(geometry);
            node->setFlag(QSGNode::OwnsGeometry, true);
        } else {
            geometry->allocate(vertex_count);
        }
        if (vertex_count > 0 && vertices)
            std::memcpy(geometry->vertexData(), vertices,
                        size_t(vertex_count) * 2 * sizeof(float));
        geometry->markVertexDataDirty();
        node->markDirty(QSGNode::DirtyGeometry);

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
}}

/// 星图背景点阵（QML 类型 `Sujian.StarMapGridItem`）。
#[derive(QObject, Default)]
pub struct StarMapGridItem {
    base: qt_base_class!(trait QQuickItem),

    zoom_level: qt_property!(f64; ALIAS zoomLevel WRITE set_zoom_level),
    pan_x: qt_property!(f64; ALIAS panX WRITE set_pan_x),
    pan_y: qt_property!(f64; ALIAS panY WRITE set_pan_y),
    dot_color_hex: qt_property!(QString; ALIAS dotColorHex WRITE set_dot_color_hex),
    dot_opacity: qt_property!(f64; ALIAS dotOpacity WRITE set_dot_opacity),
}

impl StarMapGridItem {
    fn request_paint(&mut self) {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return;
        }
        // SAFETY: obj 是有效 QQuickItem 指针；update() 只安排场景图同步。
        cpp!(unsafe [obj as "QQuickItem *"] {
            if (obj)
                obj->update();
        });
    }

    fn set_zoom_level(&mut self, value: f64) {
        if self.zoom_level == value {
            return;
        }
        self.zoom_level = value;
        self.request_paint();
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

    fn set_dot_color_hex(&mut self, value: QString) {
        if self.dot_color_hex == value {
            return;
        }
        self.dot_color_hex = value;
        self.request_paint();
    }

    fn set_dot_opacity(&mut self, value: f64) {
        if self.dot_opacity == value {
            return;
        }
        self.dot_opacity = value;
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

    fn build_vertices(&self) -> Vec<f32> {
        let (width, height) = self.item_size();
        let zoom = self.zoom_level;
        if !(zoom > 0.0) || !zoom.is_finite() || width <= 0.0 || height <= 0.0 {
            return Vec::new();
        }
        let mut world_spacing = 50.0_f64;
        let mut spacing = world_spacing * zoom;
        while spacing < 16.0 {
            world_spacing *= 5.0;
            spacing = world_spacing * zoom;
            if !spacing.is_finite() {
                return Vec::new();
            }
        }
        if spacing <= 0.0 || width > 1.0e6 || height > 1.0e6 {
            return Vec::new();
        }
        let cols = (width / spacing).ceil() as i64 + 2;
        let rows = (height / spacing).ceil() as i64 + 2;
        if cols <= 0 || rows <= 0 || cols.saturating_mul(rows) > 200_000 {
            return Vec::new();
        }
        let start_x = self.pan_x - (self.pan_x / spacing).floor() * spacing;
        let start_y = self.pan_y - (self.pan_y / spacing).floor() * spacing;
        let mut vertices = Vec::with_capacity((cols * rows * 12) as usize);
        for col in 0..cols {
            let cx = start_x + col as f64 * spacing;
            for row in 0..rows {
                let cy = start_y + row as f64 * spacing;
                let x0 = (cx - 1.0) as f32;
                let y0 = (cy - 1.0) as f32;
                let x1 = (cx + 1.0) as f32;
                let y1 = (cy + 1.0) as f32;
                vertices.extend_from_slice(&[x0, y0, x1, y0, x0, y1, x0, y1, x1, y0, x1, y1]);
            }
        }
        vertices
    }
}

impl QQuickItem for StarMapGridItem {
    fn component_complete(&mut self) {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return;
        }
        // SAFETY: obj 是有效 QQuickItem 指针；ItemHasContents 启用 updatePaintNode。
        cpp!(unsafe [obj as "QQuickItem *"] {
            if (obj)
                obj->setFlag(QQuickItem::ItemHasContents, true);
        });
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

        let vertices = self.build_vertices();
        let vertices_ptr = vertices.as_ptr();
        let count = (vertices.len() / 2) as i32;
        let color = qmetaobject::QColor::from_name(&self.dot_color_hex.to_string());
        let alpha = if self.dot_opacity.is_finite() {
            self.dot_opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        GRID_GEOMETRY_UPDATES.fetch_add(1, Ordering::Relaxed);
        GRID_VERTEX_UPLOADS.fetch_add(count.max(0) as u64, Ordering::Relaxed);
        let r = color.red();
        let g = color.green();
        let b = color.blue();
        // SAFETY: raw 是本 item 渲染线程上的 paint node；顶点数组在调用期间有效，
        // C++ 只在本函数内拷贝，不保留 Rust 指针。
        cpp!(unsafe [
            raw as "QSGNode *",
            vertices_ptr as "const float *",
            count as "int",
            r as "int", g as "int", b as "int", alpha as "double"
        ] {
            sujian_starmap_grid_update(raw, vertices_ptr, count, r, g, b, alpha);
        });
        // SAFETY: raw 是刚取回所有权的节点指针，交给 Qt 继续持有。
        unsafe { SGNode::from_raw(raw) }
    }
}
