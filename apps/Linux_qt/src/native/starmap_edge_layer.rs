// =============================================================================
// native/starmap_edge_layer.rs — 星图连线批量场景图图层（Issue #1029）
// =============================================================================
//
// 层级：Linux_qt 平台可视组件（QML 原生类型 `Sujian.StarMapEdgeLayer`）。
//
// 背景：旧实现为每条边创建一个全屏 Item + 两个 ShapePath，254 条边在每次
// 平移/缩放时都会各自重算 PathPolyline 并触发场景图重建。这里把“所有未选中边
// + 少量选中边”合并成两个 QSGGeometryNode。
//
// 评论修正：#1029 要求相机变化不再上传整张图的顶点。现在顶点只保存相对
// `anchorX/anchorY` 的 world 坐标与线段方向；线宽 1~6 屏幕像素、箭头大小全部
// 在 QSGMaterialShader 里按 uniform 计算。平移/缩放只更新 QSGTransformNode
// 矩阵与少量 uniform，不 allocate / memcpy 顶点。顶点只在结构、节点位置、
// 选择或 anchor 改变时重建，并分别记录 geometry/camera 统计。
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

const EDGE_VERTEX_SHADER: &[u8] = include_bytes!("shaders/starmap_edge.vert.qsb");
const EDGE_FRAGMENT_SHADER: &[u8] = include_bytes!("shaders/starmap_edge.frag.qsb");

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
    #include <QtCore/QCoreApplication>
    #include <QtCore/QDir>
    #include <QtCore/QFile>
    #include <QtGui/QColor>
    #include <QtGui/QMatrix4x4>
    #include <QtGui/QVector4D>
    #include <QtQuick/QQuickItem>
    #include <QtQuick/QSGGeometry>
    #include <QtQuick/QSGGeometryNode>
    #include <QtQuick/QSGMaterial>
    #include <QtQuick/QSGMaterialShader>
    #include <QtQuick/QSGNode>
    #include <QtQuick/QSGRendererInterface>
    #include <QtQuick/QSGTransformNode>
    #include <cstring>

    static QString g_sujian_edge_vertex_shader_path;
    static QString g_sujian_edge_fragment_shader_path;

    static void sujian_write_shader_file(const QString &path,
                                         const char *bytes, int length)
    {
        QFile file(path);
        if (file.open(QIODevice::WriteOnly | QIODevice::Truncate))
            file.write(bytes, length);
    }

    static void sujian_set_edge_shaders(const char *vertex, int vertex_len,
                                        const char *fragment, int fragment_len)
    {
        // QSGMaterialShader 只需要 qsb 文件路径；把嵌入字节写到进程临时目录，
        // 避免依赖 Qt 私有 QShader 头文件，也不把源文件路径带进安装包。
        if (!g_sujian_edge_vertex_shader_path.isEmpty())
            return;
        const QString base = QDir::tempPath() + QStringLiteral("/sujian-starmap-")
                + QString::number(QCoreApplication::applicationPid());
        g_sujian_edge_vertex_shader_path = base + QStringLiteral("-edge.vert.qsb");
        g_sujian_edge_fragment_shader_path = base + QStringLiteral("-edge.frag.qsb");
        sujian_write_shader_file(g_sujian_edge_vertex_shader_path, vertex, vertex_len);
        sujian_write_shader_file(g_sujian_edge_fragment_shader_path, fragment, fragment_len);
    }

    class SujianEdgeMaterial : public QSGMaterial
    {
    public:
        SujianEdgeMaterial() { setFlag(QSGMaterial::Blending); }

        QColor color;
        float stroke_half = 1.0f;
        float viewport_x = 1.0f;
        float viewport_y = 1.0f;

        QSGMaterialType *type() const override
        {
            static QSGMaterialType material_type;
            return &material_type;
        }

        int compare(const QSGMaterial *other) const override
        {
            const auto *rhs = static_cast<const SujianEdgeMaterial *>(other);
            if (color.rgba() != rhs->color.rgba())
                return color.rgba() < rhs->color.rgba() ? -1 : 1;
            if (stroke_half != rhs->stroke_half)
                return stroke_half < rhs->stroke_half ? -1 : 1;
            if (viewport_x != rhs->viewport_x)
                return viewport_x < rhs->viewport_x ? -1 : 1;
            if (viewport_y != rhs->viewport_y)
                return viewport_y < rhs->viewport_y ? -1 : 1;
            return 0;
        }

        QSGMaterialShader *createShader(QSGRendererInterface::RenderMode) const override;
    };

    class SujianEdgeShader : public QSGMaterialShader
    {
    public:
        SujianEdgeShader()
        {
            setShaderFileName(QSGMaterialShader::VertexStage,
                              g_sujian_edge_vertex_shader_path);
            setShaderFileName(QSGMaterialShader::FragmentStage,
                              g_sujian_edge_fragment_shader_path);
        }

        bool updateUniformData(RenderState &state, QSGMaterial *new_material,
                               QSGMaterial *old_material) override
        {
            QByteArray *buffer = state.uniformData();
            if (buffer->size() < 112)
                buffer->resize(112);
            char *data = buffer->data();
            bool changed = false;

            if (state.isMatrixDirty()) {
                std::memcpy(data, state.combinedMatrix().constData(), 64);
                changed = true;
            }
            if (state.isOpacityDirty()) {
                const float opacity = state.opacity();
                std::memcpy(data + 64, &opacity, sizeof(float));
                changed = true;
            }

            const auto *material = static_cast<const SujianEdgeMaterial *>(new_material);
            const auto *previous = static_cast<const SujianEdgeMaterial *>(old_material);
            if (changed || !previous
                || material->color.rgba() != previous->color.rgba()
                || material->stroke_half != previous->stroke_half
                || material->viewport_x != previous->viewport_x
                || material->viewport_y != previous->viewport_y) {
                const float color[4] = { material->color.redF(),
                                         material->color.greenF(),
                                         material->color.blueF(),
                                         material->color.alphaF() };
                std::memcpy(data + 80, color, sizeof(color));
                const float viewport[2] = { material->viewport_x, material->viewport_y };
                std::memcpy(data + 96, viewport, sizeof(viewport));
                std::memcpy(data + 104, &material->stroke_half, sizeof(float));
                changed = true;
            }
            return changed;
        }
    };

    QSGMaterialShader *SujianEdgeMaterial::createShader(QSGRendererInterface::RenderMode) const
    {
        return new SujianEdgeShader;
    }

    static const QSGGeometry::Attribute sujian_edge_attributes[] = {
        QSGGeometry::Attribute::create(0, 2, QSGGeometry::FloatType, true),
        QSGGeometry::Attribute::create(1, 2, QSGGeometry::FloatType),
        QSGGeometry::Attribute::create(2, 2, QSGGeometry::FloatType),
        QSGGeometry::Attribute::create(3, 1, QSGGeometry::FloatType),
    };
    static const QSGGeometry::AttributeSet sujian_edge_attribute_set = {
        4, 7 * int(sizeof(float)), sujian_edge_attributes
    };

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

    static SujianEdgeMaterial *sujian_edge_material(QSGGeometryNode *node)
    {
        QSGMaterial *existing = node->material();
        if (existing && existing->type() == SujianEdgeMaterial().type())
            return static_cast<SujianEdgeMaterial *>(existing);
        auto *material = new SujianEdgeMaterial;
        node->setMaterial(material);
        node->setFlag(QSGNode::OwnsMaterial, true);
        return material;
    }

    static void sujian_apply_edge_material(QSGGeometryNode *node,
                                            int r, int g, int b, double alpha,
                                            double stroke_half,
                                            double viewport_x, double viewport_y)
    {
        SujianEdgeMaterial *material = sujian_edge_material(node);
        QColor color(r, g, b);
        color.setAlphaF(alpha);
        material->color = color;
        material->stroke_half = float(stroke_half);
        material->viewport_x = float(viewport_x);
        material->viewport_y = float(viewport_y);
        node->markDirty(QSGNode::DirtyMaterial);
    }

    static void sujian_apply_edge_geometry(QSGGeometryNode *node,
                                           const float *vertices, int vertex_count,
                                           int r, int g, int b, double alpha,
                                           double stroke_half,
                                           double viewport_x, double viewport_y)
    {
        QSGGeometry *geometry = node->geometry();
        if (!geometry) {
            geometry = new QSGGeometry(sujian_edge_attribute_set, vertex_count);
            geometry->setDrawingMode(QSGGeometry::DrawTriangles);
            node->setGeometry(geometry);
            node->setFlag(QSGNode::OwnsGeometry, true);
        } else if (geometry->vertexCount() != vertex_count) {
            geometry->allocate(vertex_count);
        }
        if (vertex_count > 0 && vertices) {
            std::memcpy(geometry->vertexData(), vertices,
                        size_t(vertex_count) * size_t(sujian_edge_attribute_set.stride));
        }
        geometry->markVertexDataDirty();
        node->markDirty(QSGNode::DirtyGeometry);
        sujian_apply_edge_material(node, r, g, b, alpha, stroke_half,
                                   viewport_x, viewport_y);
    }

    static void sujian_starmap_edges_update(QSGNode *root,
                                            double translate_x, double translate_y,
                                            double zoom,
                                            bool geometry_dirty,
                                            const float *normal_vertices,
                                            int normal_vertex_count,
                                            const float *selected_vertices,
                                            int selected_vertex_count,
                                            int nr, int ng, int nb, double na,
                                            int sr, int sg, int sb, double sa,
                                            double stroke_half,
                                            double viewport_x, double viewport_y)
    {
        QSGTransformNode *transform = sujian_edge_transform_node(root);
        if (!transform)
            return;

        QMatrix4x4 matrix;
        matrix.translate(float(translate_x), float(translate_y));
        matrix.scale(float(zoom));
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
        if (geometry_dirty || !normal->geometry() || !selected->geometry()) {
            sujian_apply_edge_geometry(normal, normal_vertices, normal_vertex_count,
                                       nr, ng, nb, na, stroke_half,
                                       viewport_x, viewport_y);
            sujian_apply_edge_geometry(selected, selected_vertices,
                                       selected_vertex_count, sr, sg, sb, sa,
                                       stroke_half, viewport_x, viewport_y);
        } else {
            // 相机帧只更新颜色/线宽 uniform，不触碰顶点或几何脏标记。
            sujian_apply_edge_material(normal, nr, ng, nb, na, stroke_half,
                                       viewport_x, viewport_y);
            sujian_apply_edge_material(selected, sr, sg, sb, sa, stroke_half,
                                       viewport_x, viewport_y);
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
        // 缩放只改 transform 矩阵与 stroke uniform，不重建顶点。
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
        self.request_paint();
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

    /// 顶点布局：pos(2) + dir(2) + offset(2) + kind(1)，共 7 个 float。
    /// 线段 offset=(0,±1) 由 shader 乘屏幕半线宽；箭头 offset 是相对箭尖的
    /// world 偏移，由 shader 按当前变换换算成屏幕像素。缩放 / 平移不重建。
    fn build_vertices(&self) -> (Vec<f32>, Vec<f32>) {
        let anchor_x = self.anchor_x;
        let anchor_y = self.anchor_y;
        if !anchor_x.is_finite() || !anchor_y.is_finite() {
            return (Vec::new(), Vec::new());
        }
        let selected_id = self.selected_edge_id.to_string();
        let mut normal = Vec::with_capacity(self.edges.len() * 63);
        let mut selected = Vec::with_capacity(63);

        let local = |wx: f32, wy: f32| -> (f32, f32) {
            ((f64::from(wx) - anchor_x) as f32, (f64::from(wy) - anchor_y) as f32)
        };

        for edge in &self.edges {
            let target = if !selected_id.is_empty() && edge.edge_id == selected_id {
                &mut selected
            } else {
                &mut normal
            };
            let (sx, sy) = local(edge.start_x, edge.start_y);
            let (ex, ey) = local(edge.end_x, edge.end_y);
            let dx = ex - sx;
            let dy = ey - sy;
            let len = (dx * dx + dy * dy).sqrt();
            if !len.is_finite() || len < 1e-4 {
                continue;
            }
            let ux = dx / len;
            let uy = dy / len;

            let mut push = |x: f32, y: f32, ox: f32, oy: f32, kind: f32| {
                target.extend_from_slice(&[x, y, ux, uy, ox, oy, kind]);
            };

            // 线段：六个顶点，offset.y = ±1，shader 乘屏幕半线宽。
            push(sx, sy, 0.0, -1.0, 0.0);
            push(sx, sy, 0.0, 1.0, 0.0);
            push(ex, ey, 0.0, 1.0, 0.0);
            push(sx, sy, 0.0, -1.0, 0.0);
            push(ex, ey, 0.0, 1.0, 0.0);
            push(ex, ey, 0.0, -1.0, 0.0);

            // 箭头：箭尖 + 左右底角，offset 是相对箭尖的 world 偏移。
            let (tx, ty) = local(edge.arrow_tip_x, edge.arrow_tip_y);
            let (lx, ly) = local(edge.arrow_left_x, edge.arrow_left_y);
            let (rx, ry) = local(edge.arrow_right_x, edge.arrow_right_y);
            let (la, ll) = (lx - tx, ly - ty);
            let (ra, rl) = (rx - tx, ry - ty);
            let left_along = la * ux + ll * uy;
            let left_lateral = -la * uy + ll * ux;
            let right_along = ra * ux + rl * uy;
            let right_lateral = -ra * uy + rl * ux;
            push(tx, ty, 0.0, 0.0, 1.0);
            push(tx, ty, left_along, left_lateral, 1.0);
            push(tx, ty, right_along, right_lateral, 1.0);
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
        let vertex_ptr = EDGE_VERTEX_SHADER.as_ptr();
        let vertex_len = EDGE_VERTEX_SHADER.len() as i32;
        let fragment_ptr = EDGE_FRAGMENT_SHADER.as_ptr();
        let fragment_len = EDGE_FRAGMENT_SHADER.len() as i32;
        // SAFETY: obj 是有效 QQuickItem 指针；ItemHasContents 启用 updatePaintNode。
        // shader 字节是 'static，C++ 立即复制到 QByteArray。
        cpp!(unsafe [
            obj as "QQuickItem *",
            vertex_ptr as "const char *", vertex_len as "int",
            fragment_ptr as "const char *", fragment_len as "int"
        ] {
            if (obj) {
                obj->setFlag(QQuickItem::ItemHasContents, true);
                obj->setClip(true);
            }
            sujian_set_edge_shaders(vertex_ptr, vertex_len, fragment_ptr, fragment_len);
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
        let normal_count = (normal.len() / 7) as i32;
        let selected_count = (selected.len() / 7) as i32;
        let (tx, ty) = self.camera_translation();
        let (width, height) = self.item_size();
        let zoom = if self.zoom_level > 0.0 && self.zoom_level.is_finite() {
            self.zoom_level
        } else {
            1.0
        };
        let stroke_half = (self.line_world_width * zoom).clamp(1.0, 6.0) / 2.0;
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
            tx as "double", ty as "double", zoom as "double",
            geometry_dirty as "bool",
            normal_ptr as "const float *",
            normal_count as "int",
            selected_ptr as "const float *",
            selected_count as "int",
            nr as "int", ng as "int", nb as "int", reveal as "double",
            sr as "int", sg as "int", sb as "int",
            stroke_half as "double", width as "double", height as "double"
        ] {
            sujian_starmap_edges_update(raw, tx, ty, zoom, geometry_dirty,
                                        normal_ptr, normal_count,
                                        selected_ptr, selected_count,
                                        nr, ng, nb, reveal, sr, sg, sb, reveal,
                                        stroke_half, width, height);
        });
        self.geometry_dirty = false;
        // SAFETY: raw 是刚取回所有权的节点指针，交给 Qt 继续持有。
        unsafe { SGNode::from_raw(raw) }
    }
}
