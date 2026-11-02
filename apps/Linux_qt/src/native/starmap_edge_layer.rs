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
// 锚点的 world 坐标与线段方向；线宽固定为屏幕物理像素、箭头大小也全部在
// QSGMaterialShader 里按 uniform 计算。平移/缩放只更新 QSGTransformNode
// 矩阵与少量 uniform，不 allocate / memcpy 顶点。顶点只在结构、节点位置、
// 选择、锚点或极端镜头跨档时重建，并分别记录 geometry/camera 统计。
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
use std::sync::Mutex;

use crate::starmap_view::edge_render::EdgeRender;

static EDGE_GEOMETRY_UPDATES: AtomicU64 = AtomicU64::new(0);
static EDGE_VERTEX_UPLOADS: AtomicU64 = AtomicU64::new(0);
static EDGE_CAMERA_UPDATES: AtomicU64 = AtomicU64::new(0);

// Issue #1031 第四轮：只有一条绘制路径。世界端点始终保留 f64，先在屏幕
// 空间投影并裁剪到视口附近，再把有限屏幕局部坐标转成 f32 顶点。边数规模
// 当前只有数百条，每帧只处理视口可见线段；不需要 zoom 档位或第二套模式。
const EDGE_CULL_MARGIN_PX: f64 = 64.0;
/// 箭头固定为屏幕像素尺寸，不随 zoom 放大。
const EDGE_ARROW_LENGTH_PX: f64 = 7.0;
const EDGE_ARROW_HALF_WIDTH_PX: f64 = 4.0;

/// 最近一次同步/渲染状态。只记录数量、坐标与资源状态，不包含正文。
/// update_paint_node 在渲染线程写，诊断导出线程读取，用 Mutex 串行化。
#[derive(Clone, Default)]
struct EdgeDiagnostics {
    edge_render_count: usize,
    normal_vertex_count: i32,
    selected_vertex_count: i32,
    normal_geometry_vertex_count: i32,
    selected_geometry_vertex_count: i32,
    normal_geometry_present: bool,
    selected_geometry_present: bool,
    normal_material_present: bool,
    selected_material_present: bool,
    viewport_x: f64,
    viewport_y: f64,
    stroke_half: f64,
    zoom: f64,
    normal_width_px: f64,
    selected_width_px: f64,
    translate_x: f64,
    translate_y: f64,
    normal_color: String,
    normal_alpha: f64,
    selected_alpha: f64,
    reveal: f64,
    sample_edge_id: String,
    sample_start_x: f64,
    sample_start_y: f64,
    sample_end_x: f64,
    sample_end_y: f64,
    sample_start_screen_x: f64,
    sample_start_screen_y: f64,
    sample_end_screen_x: f64,
    sample_end_screen_y: f64,
    clip_width: f64,
    clip_height: f64,
    shader_resource_exists: bool,
}

static EDGE_DIAGNOSTICS: Mutex<Option<EdgeDiagnostics>> = Mutex::new(None);

// build.rs 用 Qt Shader Tools 的 qsb -d 检查实际打包的二进制；工具不可用时
// 这些常量为 None，诊断不会把“未验证”写成“已就绪”。
const EDGE_VERTEX_QSB_SHADERS: Option<&str> = option_env!("SUJIAN_EDGE_VERTEX_QSB_SHADERS");
const EDGE_FRAGMENT_QSB_SHADERS: Option<&str> = option_env!("SUJIAN_EDGE_FRAGMENT_QSB_SHADERS");
const EDGE_VERTEX_QSB_STANDARD_GLSL: Option<&str> =
    option_env!("SUJIAN_EDGE_VERTEX_QSB_STANDARD_GLSL");
const EDGE_VERTEX_QSB_BATCHABLE_GLSL: Option<&str> =
    option_env!("SUJIAN_EDGE_VERTEX_QSB_BATCHABLE_GLSL");
const EDGE_FRAGMENT_QSB_STANDARD_GLSL: Option<&str> =
    option_env!("SUJIAN_EDGE_FRAGMENT_QSB_STANDARD_GLSL");
const EDGE_VERTEX_QSB_QTQUICK_READY: Option<&str> =
    option_env!("SUJIAN_EDGE_VERTEX_QSB_QTQUICK_READY");
const EDGE_FRAGMENT_QSB_QTQUICK_READY: Option<&str> =
    option_env!("SUJIAN_EDGE_FRAGMENT_QSB_QTQUICK_READY");

fn split_metadata(value: Option<&str>, separator: char) -> Vec<&str> {
    value
        .unwrap_or_default()
        .split(separator)
        .filter(|item| !item.is_empty())
        .collect()
}

/// 诊断用渲染统计：只含计数与实际资源状态，不含星图正文。
pub fn render_statistics_json() -> String {
    let snapshot = EDGE_DIAGNOSTICS
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_default();
    let vertex_shaders = split_metadata(EDGE_VERTEX_QSB_SHADERS, ';');
    let fragment_shaders = split_metadata(EDGE_FRAGMENT_QSB_SHADERS, ';');
    let vertex_standard = split_metadata(EDGE_VERTEX_QSB_STANDARD_GLSL, ',');
    let vertex_batchable = split_metadata(EDGE_VERTEX_QSB_BATCHABLE_GLSL, ',');
    let fragment_standard = split_metadata(EDGE_FRAGMENT_QSB_STANDARD_GLSL, ',');
    let opengl_ready =
        EDGE_VERTEX_QSB_QTQUICK_READY == Some("1") && EDGE_FRAGMENT_QSB_QTQUICK_READY == Some("1");
    serde_json::json!({
        "edgeLayerGeometryUpdates": EDGE_GEOMETRY_UPDATES.load(Ordering::Relaxed),
        "edgeLayerVertexUploads": EDGE_VERTEX_UPLOADS.load(Ordering::Relaxed),
        "edgeLayerCameraUpdates": EDGE_CAMERA_UPDATES.load(Ordering::Relaxed),
        "edgeRenderCount": snapshot.edge_render_count,
        "normalVertexCount": snapshot.normal_vertex_count,
        "selectedVertexCount": snapshot.selected_vertex_count,
        "normalGeometryVertexCount": snapshot.normal_geometry_vertex_count,
        "selectedGeometryVertexCount": snapshot.selected_geometry_vertex_count,
        "normalGeometryPresent": snapshot.normal_geometry_present,
        "selectedGeometryPresent": snapshot.selected_geometry_present,
        "normalMaterialPresent": snapshot.normal_material_present,
        "selectedMaterialPresent": snapshot.selected_material_present,
        "shaderResourceExists": snapshot.shader_resource_exists,
        "shaderBuildToolValidated": !vertex_shaders.is_empty()
            && !fragment_shaders.is_empty(),
        "shaderVertexBuildShaders": vertex_shaders,
        "shaderFragmentBuildShaders": fragment_shaders,
        "shaderVertexStandardGlslVariants": vertex_standard,
        "shaderVertexBatchableGlslVariants": vertex_batchable,
        "shaderFragmentStandardGlslVariants": fragment_standard,
        "shaderQtQuickVariantsReady": opengl_ready,
        // QSGMaterialShader 没有公开 API 查询 GPU 编译/绘制结果，不能伪报。
        "shaderPipelineVerified": false,
        "viewport": [snapshot.viewport_x, snapshot.viewport_y],
        "clipRect": [0.0, 0.0, snapshot.clip_width, snapshot.clip_height],
        "strokeHalf": snapshot.stroke_half,
        "zoom": snapshot.zoom,
        "normalWidthPx": snapshot.normal_width_px,
        "selectedWidthPx": snapshot.selected_width_px,
        "cameraTranslation": [snapshot.translate_x, snapshot.translate_y],
        "normalColor": snapshot.normal_color,
        "normalAlpha": snapshot.normal_alpha,
        "selectedAlpha": snapshot.selected_alpha,
        "reveal": snapshot.reveal,
        "sampleEdge": {
            "id": snapshot.sample_edge_id,
            "start": [snapshot.sample_start_x, snapshot.sample_start_y],
            "end": [snapshot.sample_end_x, snapshot.sample_end_y],
            "startScreen": [snapshot.sample_start_screen_x, snapshot.sample_start_screen_y],
            "endScreen": [snapshot.sample_end_screen_x, snapshot.sample_end_screen_y],
        },
    })
    .to_string()
}

cpp! {{
    #include <qmetaobject_rust.hpp>
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

    // qsb 直接编译进 qrc（main.rs 注册），不依赖进程临时目录或安装目录。
    static const QString kSujianEdgeVertexShader =
            QStringLiteral(":/shaders/starmap_edge.vert.qsb");
    static const QString kSujianEdgeFragmentShader =
            QStringLiteral(":/shaders/starmap_edge.frag.qsb");

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
            setShaderFileName(QSGMaterialShader::VertexStage, kSujianEdgeVertexShader);
            setShaderFileName(QSGMaterialShader::FragmentStage, kSujianEdgeFragmentShader);
        }

        bool updateUniformData(RenderState &state, QSGMaterial *new_material,
                               QSGMaterial *old_material) override
        {
            Q_UNUSED(old_material);
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

            // 自定义 uniform 每次都写：Qt 可能在多个批次/绘制通道里复用同一个
            // material 指针，但 uniform 缓冲区并不保证跨批次保留旧值。这里
            // 只有 4+2+1 个 float，成本远低于漏初始化导致的整层不可见。
            auto *material = static_cast<SujianEdgeMaterial *>(new_material);
            const float color[4] = { material->color.redF(),
                                     material->color.greenF(),
                                     material->color.blueF(),
                                     material->color.alphaF() };
            std::memcpy(data + 80, color, sizeof(color));
            const float viewport[2] = { material->viewport_x, material->viewport_y };
            std::memcpy(data + 96, viewport, sizeof(viewport));
            std::memcpy(data + 104, &material->stroke_half, sizeof(float));
            return true;
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
        const bool uniforms_changed =
            material->color.rgba() != color.rgba()
            || material->stroke_half != float(stroke_half)
            || material->viewport_x != float(viewport_x)
            || material->viewport_y != float(viewport_y);
        if (uniforms_changed) {
            material->color = color;
            material->stroke_half = float(stroke_half);
            material->viewport_x = float(viewport_x);
            material->viewport_y = float(viewport_y);
            node->markDirty(QSGNode::DirtyMaterial);
        }
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

    // 只报告 qrc 资源存在；QShader 解析与 stage/变体校验由 build.rs 的
    // 官方 qsb -d 完成。QSGMaterialShader 没有公开的 GPU pipeline 查询接口。
    static qint64 sujian_edge_shader_state_bits()
    {
        qint64 bits = 0;
        if (QFile::exists(kSujianEdgeVertexShader))
            bits |= 1LL << 0;
        if (QFile::exists(kSujianEdgeFragmentShader))
            bits |= 1LL << 1;
        return bits;
    }

    // 诊断位布局：normal_vc[0..19] | selected_vc[20..39] | flags[40..]。
    static qint64 sujian_edge_debug_state(QSGNode *root)
    {
        QSGTransformNode *transform = nullptr;
        if (root && root->firstChild()
                && root->firstChild()->type() == QSGNode::TransformNodeType)
            transform = static_cast<QSGTransformNode *>(root->firstChild());
        QSGGeometryNode *normal = sujian_edge_geometry_node(transform, 0);
        QSGGeometryNode *selected = sujian_edge_geometry_node(transform, 1);
        const qint64 normal_vc = normal && normal->geometry()
                ? normal->geometry()->vertexCount() : 0;
        const qint64 selected_vc = selected && selected->geometry()
                ? selected->geometry()->vertexCount() : 0;
        qint64 flags = 0;
        if (normal && normal->geometry())
            flags |= 1;
        if (selected && selected->geometry())
            flags |= 2;
        if (normal && normal->material())
            flags |= 4;
        if (selected && selected->material())
            flags |= 8;
        return (normal_vc & 0xfffff) | ((selected_vc & 0xfffff) << 20)
                | (flags << 40);
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
                                            double normal_stroke_half,
                                            double selected_stroke_half,
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
                                       nr, ng, nb, na, normal_stroke_half,
                                       viewport_x, viewport_y);
            sujian_apply_edge_geometry(selected, selected_vertices,
                                       selected_vertex_count, sr, sg, sb, sa,
                                       selected_stroke_half, viewport_x, viewport_y);
        } else {
            // 相机帧只更新颜色/线宽 uniform，不触碰顶点或几何脏标记。
            sujian_apply_edge_material(normal, nr, ng, nb, na, normal_stroke_half,
                                       viewport_x, viewport_y);
            sujian_apply_edge_material(selected, sr, sg, sb, sa, selected_stroke_half,
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
    normal_width_px: qt_property!(f64; ALIAS normalWidthPx WRITE set_normal_width_px),
    selected_width_px: qt_property!(f64; ALIAS selectedWidthPx WRITE set_selected_width_px),
    normal_alpha: qt_property!(f64; ALIAS normalAlpha WRITE set_normal_alpha),
    selected_alpha: qt_property!(f64; ALIAS selectedAlpha WRITE set_selected_alpha),
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

    /// 结构 / 选择变化：下一次同步重新投影并上传可见顶点。
    fn invalidate_geometry(&mut self) {
        self.mark_geometry_dirty();
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
        self.invalidate_geometry();
    }

    fn set_selected_edge_id(&mut self, value: QString) {
        if self.selected_edge_id == value {
            return;
        }
        self.selected_edge_id = value;
        self.invalidate_geometry();
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

    fn set_anchor_x(&mut self, value: f64) {
        if self.anchor_x == value {
            return;
        }
        self.anchor_x = value;
        self.request_paint();
    }

    fn set_anchor_y(&mut self, value: f64) {
        if self.anchor_y == value {
            return;
        }
        self.anchor_y = value;
        self.request_paint();
    }

    fn set_reveal(&mut self, value: f64) {
        if self.reveal == value {
            return;
        }
        self.reveal = value;
        self.request_paint();
    }

    fn set_normal_width_px(&mut self, value: f64) {
        if self.normal_width_px == value {
            return;
        }
        self.normal_width_px = value;
        self.request_paint();
    }

    fn set_selected_width_px(&mut self, value: f64) {
        if self.selected_width_px == value {
            return;
        }
        self.selected_width_px = value;
        self.request_paint();
    }

    fn set_normal_alpha(&mut self, value: f64) {
        if self.normal_alpha == value {
            return;
        }
        self.normal_alpha = value;
        self.request_paint();
    }

    fn set_selected_alpha(&mut self, value: f64) {
        if self.selected_alpha == value {
            return;
        }
        self.selected_alpha = value;
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

    fn sanitized_width_px(value: f64) -> f64 {
        if value.is_finite() {
            value.clamp(0.25, 16.0)
        } else {
            1.0
        }
    }

    /// Liang-Barsky：把屏幕空间线段裁到视口附近。只要线段穿过视口，
    /// 即使两个端点都远在视口外也会保留可见部分。
    fn clip_segment(
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        left: f64,
        top: f64,
        right: f64,
        bottom: f64,
    ) -> Option<(f64, f64, f64, f64)> {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let mut t0 = 0.0f64;
        let mut t1 = 1.0f64;
        for (p, q) in [
            (-dx, x0 - left),
            (dx, right - x0),
            (-dy, y0 - top),
            (dy, bottom - y0),
        ] {
            if p.abs() < f64::EPSILON {
                if q < 0.0 {
                    return None;
                }
                continue;
            }
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return None;
                }
                if r > t0 {
                    t0 = r;
                }
            } else {
                if r < t0 {
                    return None;
                }
                if r < t1 {
                    t1 = r;
                }
            }
        }
        if !(0.0..=1.0).contains(&t0) || !(0.0..=1.0).contains(&t1) || t0 > t1 {
            return None;
        }
        Some((x0 + t0 * dx, y0 + t0 * dy, x0 + t1 * dx, y0 + t1 * dy))
    }

    /// 唯一顶点生成入口：world 端点用 f64 投到屏幕，裁到视口附近，
    /// 再把屏幕局部坐标转 f32。箭头按固定屏幕像素重建，不随 zoom 变粗。
    fn build_screen_vertices(&self) -> (Vec<f32>, Vec<f32>) {
        let (width, height) = self.item_size();
        let zoom = self.zoom_level;
        if width <= 0.0
            || height <= 0.0
            || !zoom.is_finite()
            || zoom <= 0.0
            || !self.pan_x.is_finite()
            || !self.pan_y.is_finite()
        {
            return (Vec::new(), Vec::new());
        }
        let left = -EDGE_CULL_MARGIN_PX;
        let top = -EDGE_CULL_MARGIN_PX;
        let right = width + EDGE_CULL_MARGIN_PX;
        let bottom = height + EDGE_CULL_MARGIN_PX;
        let selected_id = self.selected_edge_id.to_string();
        let mut normal = Vec::with_capacity(self.edges.len() * 18);
        let mut selected = Vec::with_capacity(18);

        for edge in &self.edges {
            let sx0 = self.pan_x + edge.start_x * zoom;
            let sy0 = self.pan_y + edge.start_y * zoom;
            let sx1 = self.pan_x + edge.end_x * zoom;
            let sy1 = self.pan_y + edge.end_y * zoom;
            if ![sx0, sy0, sx1, sy1].iter().all(|value| value.is_finite()) {
                continue;
            }
            let dx = sx1 - sx0;
            let dy = sy1 - sy0;
            let len = (dx * dx + dy * dy).sqrt();
            if !len.is_finite() || len < 1e-9 {
                continue;
            }
            let ux = dx / len;
            let uy = dy / len;
            let target = if !selected_id.is_empty() && edge.edge_id == selected_id {
                &mut selected
            } else {
                &mut normal
            };

            if let Some((cx0, cy0, cx1, cy1)) =
                Self::clip_segment(sx0, sy0, sx1, sy1, left, top, right, bottom)
            {
                let cdx = cx1 - cx0;
                let cdy = cy1 - cy0;
                if (cdx * cdx + cdy * cdy).sqrt() > 1e-9 {
                    let (cx0, cy0, cx1, cy1) = (cx0 as f32, cy0 as f32, cx1 as f32, cy1 as f32);
                    let (ufx, ufy) = (ux as f32, uy as f32);
                    // 线段：offset.y = ±1，片元按到中心的屏幕距离算覆盖率。
                    target.extend_from_slice(&[
                        cx0, cy0, ufx, ufy, 0.0, -1.0, 0.0, cx0, cy0, ufx, ufy, 0.0, 1.0, 0.0, cx1,
                        cy1, ufx, ufy, 0.0, 1.0, 0.0, cx0, cy0, ufx, ufy, 0.0, -1.0, 0.0, cx1, cy1,
                        ufx, ufy, 0.0, 1.0, 0.0, cx1, cy1, ufx, ufy, 0.0, -1.0, 0.0,
                    ]);
                }
            }

            // 箭头：固定屏幕尺寸，尖端仍在真实投影位置。
            let tip_x = self.pan_x + edge.arrow_tip_x * zoom;
            let tip_y = self.pan_y + edge.arrow_tip_y * zoom;
            if !tip_x.is_finite() || !tip_y.is_finite() {
                continue;
            }
            if tip_x < left || tip_x > right || tip_y < top || tip_y > bottom {
                continue;
            }
            let base_x = tip_x - ux * EDGE_ARROW_LENGTH_PX;
            let base_y = tip_y - uy * EDGE_ARROW_LENGTH_PX;
            let nx = -uy * EDGE_ARROW_HALF_WIDTH_PX;
            let ny = ux * EDGE_ARROW_HALF_WIDTH_PX;
            let (tx, ty) = (tip_x as f32, tip_y as f32);
            let (lx, ly) = ((base_x + nx) as f32, (base_y + ny) as f32);
            let (rx, ry) = ((base_x - nx) as f32, (base_y - ny) as f32);
            let (ufx, ufy) = (ux as f32, uy as f32);
            target.extend_from_slice(&[
                tx, ty, ufx, ufy, 0.0, 0.0, 1.0, lx, ly, ufx, ufy, 0.0, 0.0, 1.0, rx, ry, ufx, ufy,
                0.0, 0.0, 1.0,
            ]);
        }
        (normal, selected)
    }
}

impl QQuickItem for StarMapEdgeLayer {
    fn component_complete(&mut self) {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return;
        }
        // SAFETY: obj 是有效 QQuickItem 指针；ItemHasContents 启用 updatePaintNode。
        // qsb 已通过 qrc 注册，材质 shader 直接按资源路径加载。
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
        // 视口尺寸参与屏幕投影和裁剪，必须重新生成可见顶点。
        self.mark_geometry_dirty();
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

        // 唯一绘制路径：f64 世界→屏幕投影、视口裁剪、f32 屏幕局部顶点。
        // 顶点已经是当前相机下的像素坐标，QSG 只做单位矩阵平移。
        let (normal, selected) = self.build_screen_vertices();
        let normal_ptr = normal.as_ptr();
        let selected_ptr = selected.as_ptr();
        let normal_count = (normal.len() / 7) as i32;
        let selected_count = (selected.len() / 7) as i32;
        let (width, height) = self.item_size();
        let zoom = if self.zoom_level.is_finite() && self.zoom_level > 0.0 {
            self.zoom_level
        } else {
            1.0
        };
        let normal_width_px = Self::sanitized_width_px(self.normal_width_px);
        let selected_width_px = Self::sanitized_width_px(self.selected_width_px);
        let normal_stroke_half = normal_width_px * 0.5;
        let selected_stroke_half = selected_width_px * 0.5;
        let (nr, ng, nb) = Self::color_parts(&self.normal_color_hex.to_string());
        let (sr, sg, sb) = Self::color_parts(&self.selected_color_hex.to_string());
        let reveal = if self.reveal.is_finite() {
            self.reveal.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let normal_alpha = if self.normal_alpha.is_finite() {
            self.normal_alpha.clamp(0.0, 1.0)
        } else {
            0.55
        } * reveal;
        let selected_alpha = if self.selected_alpha.is_finite() {
            self.selected_alpha.clamp(0.0, 1.0)
        } else {
            1.0
        } * reveal;
        let tx = 0.0;
        let ty = 0.0;
        let qsg_matrix_zoom = 1.0;
        let geometry_dirty = true;
        EDGE_GEOMETRY_UPDATES.fetch_add(1, Ordering::Relaxed);
        EDGE_VERTEX_UPLOADS.fetch_add(
            (normal_count.max(0) + selected_count.max(0)) as u64,
            Ordering::Relaxed,
        );
        // SAFETY: raw 是本 item 渲染线程上的 paint node；两个顶点数组在调用期间
        // 保持有效，C++ 只在本函数内拷贝，不保留 Rust 指针。
        cpp!(unsafe [
            raw as "QSGNode *",
            tx as "double", ty as "double", qsg_matrix_zoom as "double",
            geometry_dirty as "bool",
            normal_ptr as "const float *",
            normal_count as "int",
            selected_ptr as "const float *",
            selected_count as "int",
            nr as "int", ng as "int", nb as "int", normal_alpha as "double",
            sr as "int", sg as "int", sb as "int", selected_alpha as "double",
            normal_stroke_half as "double", selected_stroke_half as "double",
            width as "double", height as "double"
        ] {
            sujian_starmap_edges_update(raw, tx, ty, qsg_matrix_zoom, geometry_dirty,
                                        normal_ptr, normal_count,
                                        selected_ptr, selected_count,
                                        nr, ng, nb, normal_alpha,
                                        sr, sg, sb, selected_alpha,
                                        normal_stroke_half, selected_stroke_half,
                                        width, height);
        });
        // SAFETY: raw 仍是本 item 的 paint node；只读几何/材质状态用于诊断。
        let debug_state = cpp!(unsafe [raw as "QSGNode *"] -> i64 as "qint64" {
            return sujian_edge_debug_state(raw);
        });
        let shader_bits = cpp!(unsafe [] -> i64 as "qint64" {
            return sujian_edge_shader_state_bits();
        });
        let geometry_flags = debug_state >> 40;
        let sample = self.edges.first();
        let sample_start_screen = sample
            .map(|edge| {
                (
                    edge.start_x * zoom + self.pan_x,
                    edge.start_y * zoom + self.pan_y,
                )
            })
            .unwrap_or((0.0, 0.0));
        let sample_end_screen = sample
            .map(|edge| {
                (
                    edge.end_x * zoom + self.pan_x,
                    edge.end_y * zoom + self.pan_y,
                )
            })
            .unwrap_or((0.0, 0.0));
        if let Ok(mut guard) = EDGE_DIAGNOSTICS.lock() {
            *guard = Some(EdgeDiagnostics {
                edge_render_count: self.edges.len(),
                normal_vertex_count: normal_count,
                selected_vertex_count: selected_count,
                normal_geometry_vertex_count: (debug_state & 0xfffff) as i32,
                selected_geometry_vertex_count: ((debug_state >> 20) & 0xfffff) as i32,
                normal_geometry_present: geometry_flags & 1 != 0,
                selected_geometry_present: geometry_flags & 2 != 0,
                normal_material_present: geometry_flags & 4 != 0,
                selected_material_present: geometry_flags & 8 != 0,
                viewport_x: width,
                viewport_y: height,
                stroke_half: normal_stroke_half,
                zoom,
                normal_width_px,
                selected_width_px,
                translate_x: tx,
                translate_y: ty,
                normal_color: format!("#{nr:02x}{ng:02x}{nb:02x}"),
                normal_alpha,
                selected_alpha,
                reveal,
                sample_edge_id: sample.map(|edge| edge.edge_id.clone()).unwrap_or_default(),
                sample_start_x: sample.map(|edge| edge.start_x).unwrap_or(0.0),
                sample_start_y: sample.map(|edge| edge.start_y).unwrap_or(0.0),
                sample_end_x: sample.map(|edge| edge.end_x).unwrap_or(0.0),
                sample_end_y: sample.map(|edge| edge.end_y).unwrap_or(0.0),
                sample_start_screen_x: sample_start_screen.0,
                sample_start_screen_y: sample_start_screen.1,
                sample_end_screen_x: sample_end_screen.0,
                sample_end_screen_y: sample_end_screen.1,
                clip_width: width,
                clip_height: height,
                shader_resource_exists: shader_bits & (1 << 0) != 0 && shader_bits & (1 << 1) != 0,
            });
        }
        self.geometry_dirty = false;
        // SAFETY: raw 是刚取回所有权的节点指针，交给 Qt 继续持有。
        unsafe { SGNode::from_raw(raw) }
    }
}
