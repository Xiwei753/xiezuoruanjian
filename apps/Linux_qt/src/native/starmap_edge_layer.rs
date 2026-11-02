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

// 相机在世界空间保持 double 连续变换；只有进入 QSG 的这一层才夹到 float
// 安全的局部范围。极端缩放时按“视口可见范围”缓存一份局部几何，平移/缩放
// 在同一个缓存桶内只更新统一矩阵，不重新扫描边。
const MIN_RENDER_ZOOM: f64 = 1e-6;
const MAX_RENDER_ZOOM: f64 = 1e6;
const MAX_ANCHOR_SCREEN_OFFSET: f64 = 1e7;
const EXTREME_CULL_MARGIN: f64 = 64.0;
// 局部原点按屏幕像素分桶：平移超过一个桶才需要重建可见范围几何。
const EXTREME_ORIGIN_BUCKET_PX: f64 = 1024.0;

/// 极端镜头的几何缓存键。zoom 桶决定世界分桶大小，position 桶决定
/// 写入顶点的局部比例；两者都按 2 的幂量化，避免每帧重建。
#[derive(Clone, PartialEq)]
struct ExtremeGeometryKey {
    zoom_bucket_bits: u64,
    position_bucket_bits: u64,
    origin_cell_x: i64,
    origin_cell_y: i64,
    viewport_width_bits: u64,
    viewport_height_bits: u64,
    geometry_epoch: u64,
}

struct ExtremeGeometryParams {
    key: ExtremeGeometryKey,
    origin_x: f64,
    origin_y: f64,
    position_scale: f64,
    position_bucket: f64,
}

struct ExtremeGeometryCache {
    key: ExtremeGeometryKey,
    origin_x: f64,
    origin_y: f64,
    normal: Vec<f32>,
    selected: Vec<f32>,
}

/// 当前已经提交给 QSG 的几何编码。相机从极端模式退回普通模式时，
/// 必须显式重建 anchor-relative 顶点，不能只更新矩阵。
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum EdgeGeometryMode {
    #[default]
    Anchor,
    Extreme,
}

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
    render_zoom: f64,
    extreme_camera: bool,
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
        "renderZoom": snapshot.render_zoom,
        "extremeCamera": snapshot.extreme_camera,
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
    geometry_epoch: u64,
    cached_normal: Vec<f32>,
    cached_selected: Vec<f32>,
    extreme_cache: Option<ExtremeGeometryCache>,
    uploaded_geometry_mode: EdgeGeometryMode,
    uploaded_extreme_key: Option<ExtremeGeometryKey>,
    uploaded_anchor_position_bucket: Option<f64>,
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

    /// 结构 / 选择 / 锚点变化：旧缓存不再可信，清空后只重建一次可见范围。
    fn invalidate_geometry(&mut self) {
        self.geometry_epoch = self.geometry_epoch.wrapping_add(1);
        self.extreme_cache = None;
        self.cached_normal.clear();
        self.cached_selected.clear();
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
        self.camera_geometry_changed();
    }

    fn set_pan_x(&mut self, value: f64) {
        if self.pan_x == value {
            return;
        }
        self.pan_x = value;
        self.camera_geometry_changed();
    }

    fn set_pan_y(&mut self, value: f64) {
        if self.pan_y == value {
            return;
        }
        self.pan_y = value;
        self.camera_geometry_changed();
    }

    fn set_anchor_x(&mut self, value: f64) {
        if self.anchor_x == value {
            return;
        }
        self.anchor_x = value;
        self.invalidate_geometry();
    }

    fn set_anchor_y(&mut self, value: f64) {
        if self.anchor_y == value {
            return;
        }
        self.anchor_y = value;
        self.invalidate_geometry();
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

    /// 进入 QSG 前的缩放上限：世界相机仍用 f64 连续值，这里只保护 float 矩阵。
    fn render_zoom(&self) -> f64 {
        let zoom = self.zoom_level;
        if zoom.is_finite() && zoom > 0.0 {
            zoom.clamp(MIN_RENDER_ZOOM, MAX_RENDER_ZOOM)
        } else {
            1.0
        }
    }

    /// 常规镜头沿用 anchor 局部几何，相机变化只更新矩阵；只有缩放跨出
    /// float 安全档位，或 anchor 已离视口过远时，才改用以视口中心为原点的
    /// 极端模式并重建可见顶点。
    fn uses_extreme_camera(&self) -> bool {
        let zoom = self.zoom_level;
        if !zoom.is_finite() || zoom <= 0.0 {
            return false;
        }
        if zoom < MIN_RENDER_ZOOM || zoom > MAX_RENDER_ZOOM {
            return true;
        }
        let (width, height) = self.item_size();
        if width <= 0.0 || height <= 0.0 {
            return false;
        }
        if !self.pan_x.is_finite()
            || !self.pan_y.is_finite()
            || !self.anchor_x.is_finite()
            || !self.anchor_y.is_finite()
        {
            return true;
        }
        let origin_x = (width / 2.0 - self.pan_x) / zoom;
        let origin_y = (height / 2.0 - self.pan_y) / zoom;
        let tx = width / 2.0 + (self.anchor_x - origin_x) * zoom;
        let ty = height / 2.0 + (self.anchor_y - origin_y) * zoom;
        !tx.is_finite()
            || !ty.is_finite()
            || tx.abs() > MAX_ANCHOR_SCREEN_OFFSET
            || ty.abs() > MAX_ANCHOR_SCREEN_OFFSET
    }

    fn sanitized_width_px(value: f64) -> f64 {
        if value.is_finite() {
            value.clamp(0.25, 16.0)
        } else {
            1.0
        }
    }

    /// 相机变化时只有缓存桶真正变了才重建；同一桶内只更新矩阵/uniform。
    /// 注意必须比较“已上传的几何模式”，而不是只看当前是否极端：
    /// 极端 → 普通时 QSG 里仍是 bucket-local 顶点，必须重建并上传。
    fn camera_geometry_changed(&mut self) {
        if !self.uses_extreme_camera() {
            let anchor_ready = self.uploaded_geometry_mode == EdgeGeometryMode::Anchor
                && self.uploaded_anchor_position_bucket == Some(1.0);
            if anchor_ready && !self.geometry_dirty {
                self.request_paint();
            } else {
                self.mark_geometry_dirty();
            }
            return;
        }
        let Some(params) = self.extreme_geometry_params() else {
            let position_scale = self.zoom_level / self.render_zoom();
            let position_bucket = Self::pow2_bucket(position_scale);
            let anchor_ready = self.uploaded_geometry_mode == EdgeGeometryMode::Anchor
                && self.uploaded_anchor_position_bucket == position_bucket;
            if anchor_ready && !self.geometry_dirty {
                self.request_paint();
            } else {
                self.mark_geometry_dirty();
            }
            return;
        };
        let cache_valid = self.uploaded_geometry_mode == EdgeGeometryMode::Extreme
            && self.uploaded_extreme_key.as_ref() == Some(&params.key)
            && self
                .extreme_cache
                .as_ref()
                .is_some_and(|cache| cache.key == params.key)
            && !self.geometry_dirty;
        if cache_valid {
            self.request_paint();
        } else {
            self.mark_geometry_dirty();
        }
    }

    fn pow2_bucket(value: f64) -> Option<f64> {
        if !value.is_finite() || value <= 0.0 {
            return None;
        }
        let exponent = value.log2().round().clamp(-1022.0, 1022.0);
        let bucket = 2f64.powf(exponent);
        if bucket.is_finite() && bucket > 0.0 {
            Some(bucket)
        } else {
            None
        }
    }

    /// 极端镜头缓存参数：
    /// - zoom_bucket 决定世界分桶大小（每个桶约 1024 屏幕像素）；
    /// - position_bucket 决定写入 f32 顶点的局部比例，避免把 1e100 直接
    ///   交给 QSG 矩阵；
    /// - 局部原点取视口中心所在的分桶原点，桶内平移只改统一平移矩阵。
    fn extreme_geometry_params(&self) -> Option<ExtremeGeometryParams> {
        let zoom = self.zoom_level;
        let (width, height) = self.item_size();
        if !zoom.is_finite() || zoom <= 0.0 || width <= 0.0 || height <= 0.0 {
            return None;
        }
        if !self.pan_x.is_finite() || !self.pan_y.is_finite() {
            return None;
        }
        let zoom_bucket = Self::pow2_bucket(zoom)?;
        let cell_size = EXTREME_ORIGIN_BUCKET_PX / zoom_bucket;
        if !cell_size.is_finite() || cell_size <= 0.0 {
            return None;
        }
        let center_x = (width / 2.0 - self.pan_x) / zoom;
        let center_y = (height / 2.0 - self.pan_y) / zoom;
        if !center_x.is_finite() || !center_y.is_finite() {
            return None;
        }
        let origin_cell_x = (center_x / cell_size).floor();
        let origin_cell_y = (center_y / cell_size).floor();
        if origin_cell_x < i64::MIN as f64
            || origin_cell_x > i64::MAX as f64
            || origin_cell_y < i64::MIN as f64
            || origin_cell_y > i64::MAX as f64
        {
            return None;
        }
        let origin_cell_x = origin_cell_x as i64;
        let origin_cell_y = origin_cell_y as i64;
        let origin_x = origin_cell_x as f64 * cell_size;
        let origin_y = origin_cell_y as f64 * cell_size;
        if !origin_x.is_finite() || !origin_y.is_finite() {
            return None;
        }
        let position_scale = zoom / self.render_zoom();
        let position_bucket = Self::pow2_bucket(position_scale)?;
        Some(ExtremeGeometryParams {
            key: ExtremeGeometryKey {
                zoom_bucket_bits: zoom_bucket.to_bits(),
                position_bucket_bits: position_bucket.to_bits(),
                origin_cell_x,
                origin_cell_y,
                viewport_width_bits: width.to_bits(),
                viewport_height_bits: height.to_bits(),
                geometry_epoch: self.geometry_epoch,
            },
            origin_x,
            origin_y,
            position_scale,
            position_bucket,
        })
    }

    /// 当前视口对应的世界矩形（含一屏外缓冲），用于裁剪长线和箭头。
    fn visible_world_rect(&self) -> Option<(f64, f64, f64, f64)> {
        let zoom = self.zoom_level;
        let (width, height) = self.item_size();
        if !zoom.is_finite() || zoom <= 0.0 || width <= 0.0 || height <= 0.0 {
            return None;
        }
        if !self.pan_x.is_finite() || !self.pan_y.is_finite() {
            return None;
        }
        // 缩放/原点桶内只更新矩阵；中心最多移动近一个 1024px 桶，
        // 桶内缩放最多变化到约一半。缓存范围额外覆盖两倍桶宽和视口，
        // 避免平移或缩小时在缓存边缘露出未生成的线段。
        let margin =
            (EXTREME_CULL_MARGIN + EXTREME_ORIGIN_BUCKET_PX * 2.0 + width.max(height) * 0.5) / zoom;
        let left = (0.0 - self.pan_x) / zoom - margin;
        let top = (0.0 - self.pan_y) / zoom - margin;
        let right = (width - self.pan_x) / zoom + margin;
        let bottom = (height - self.pan_y) / zoom + margin;
        if [left, top, right, bottom]
            .iter()
            .all(|value| value.is_finite())
        {
            Some((left, top, right, bottom))
        } else {
            None
        }
    }

    /// Liang-Barsky：把线段裁到可见世界矩形，长线只保留穿过视口的局部，
    /// 不会因为两个端点都在视口外就整条丢掉。
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
        let checks = [
            (-dx, x0 - left),
            (dx, right - x0),
            (-dy, y0 - top),
            (dy, bottom - y0),
        ];
        for (p, q) in checks {
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

    fn arrow_visible(&self, edge: &EdgeRender, rect: (f64, f64, f64, f64)) -> bool {
        let min_x = f64::from(edge.arrow_tip_x)
            .min(f64::from(edge.arrow_left_x))
            .min(f64::from(edge.arrow_right_x));
        let max_x = f64::from(edge.arrow_tip_x)
            .max(f64::from(edge.arrow_left_x))
            .max(f64::from(edge.arrow_right_x));
        let min_y = f64::from(edge.arrow_tip_y)
            .min(f64::from(edge.arrow_left_y))
            .min(f64::from(edge.arrow_right_y));
        let max_y = f64::from(edge.arrow_tip_y)
            .max(f64::from(edge.arrow_left_y))
            .max(f64::from(edge.arrow_right_y));
        max_x >= rect.0 && min_x <= rect.2 && max_y >= rect.1 && min_y <= rect.3
    }

    /// 顶点布局：pos(2) + dir(2) + offset(2) + kind(1)，共 7 个 float。
    /// 常规镜头：顶点相对图锚点，相机变化只改统一矩阵。顶点布局：
    /// pos(2) + dir(2) + offset(2) + kind(1)，共 7 个 float。
    /// 线段 offset=(0,±1) 由 shader 乘屏幕半线宽；箭头 offset 是相对箭尖的
    /// 局部偏移，由 shader 按当前变换换算成屏幕像素。
    fn build_vertices(&self) -> (Vec<f32>, Vec<f32>) {
        if !self.anchor_x.is_finite() || !self.anchor_y.is_finite() {
            return (Vec::new(), Vec::new());
        }
        self.build_vertices_relative(self.anchor_x, self.anchor_y, 1.0, None)
    }

    /// 极端镜头：只把视口可见范围（含缓冲）内的边写入局部几何，并把
    /// 长线裁到可见矩形。局部原点取 1024px 分桶原点，桶内平移只改矩阵。
    fn build_extreme_vertices(&self, params: &ExtremeGeometryParams) -> (Vec<f32>, Vec<f32>) {
        let visible = match self.visible_world_rect() {
            Some(rect) => rect,
            None => return (Vec::new(), Vec::new()),
        };
        self.build_vertices_relative(
            params.origin_x,
            params.origin_y,
            params.position_bucket,
            Some(visible),
        )
    }

    fn build_vertices_relative(
        &self,
        base_x: f64,
        base_y: f64,
        position_scale: f64,
        visible: Option<(f64, f64, f64, f64)>,
    ) -> (Vec<f32>, Vec<f32>) {
        if !position_scale.is_finite() || position_scale <= 0.0 {
            return (Vec::new(), Vec::new());
        }
        let selected_id = self.selected_edge_id.to_string();
        let mut normal = Vec::with_capacity(self.edges.len() * 63);
        let mut selected = Vec::with_capacity(63);

        let local = |wx: f64, wy: f64| -> Option<(f32, f32)> {
            let lx = (wx - base_x) * position_scale;
            let ly = (wy - base_y) * position_scale;
            if !lx.is_finite() || !ly.is_finite() {
                return None;
            }
            let (lx, ly) = (lx as f32, ly as f32);
            if lx.is_finite() && ly.is_finite() {
                Some((lx, ly))
            } else {
                None
            }
        };

        for edge in &self.edges {
            let start_x = f64::from(edge.start_x);
            let start_y = f64::from(edge.start_y);
            let end_x = f64::from(edge.end_x);
            let end_y = f64::from(edge.end_y);
            // 方向在世界坐标中归一化；局部坐标只做统一正比例缩放，
            // 因此单位方向在局部空间保持不变。这样即使远端端点超出
            // f32 局部范围，只要线段穿过视口仍能先裁剪再转换。
            let world_dx = end_x - start_x;
            let world_dy = end_y - start_y;
            let world_len = (world_dx * world_dx + world_dy * world_dy).sqrt();
            if !world_len.is_finite() || world_len < 1e-12 {
                continue;
            }
            let ux = (world_dx / world_len) as f32;
            let uy = (world_dy / world_len) as f32;
            if !ux.is_finite() || !uy.is_finite() {
                continue;
            }

            let target = if !selected_id.is_empty() && edge.edge_id == selected_id {
                &mut selected
            } else {
                &mut normal
            };
            let mut push = |x: f32, y: f32, ox: f32, oy: f32, kind: f32| {
                target.extend_from_slice(&[x, y, ux, uy, ox, oy, kind]);
            };

            // 线段：先裁到可见世界矩形，再只转换裁剪后的局部端点。
            let clipped = match visible {
                Some((left, top, right, bottom)) => {
                    Self::clip_segment(start_x, start_y, end_x, end_y, left, top, right, bottom)
                }
                None => Some((start_x, start_y, end_x, end_y)),
            };
            if let Some((cx0, cy0, cx1, cy1)) = clipped {
                if let (Some((sx, sy)), Some((ex, ey))) = (local(cx0, cy0), local(cx1, cy1)) {
                    let dx = ex - sx;
                    let dy = ey - sy;
                    let len = (dx * dx + dy * dy).sqrt();
                    if len.is_finite() && len > 1e-6 {
                        push(sx, sy, 0.0, -1.0, 0.0);
                        push(sx, sy, 0.0, 1.0, 0.0);
                        push(ex, ey, 0.0, 1.0, 0.0);
                        push(sx, sy, 0.0, -1.0, 0.0);
                        push(ex, ey, 0.0, 1.0, 0.0);
                        push(ex, ey, 0.0, -1.0, 0.0);
                    }
                }
            }

            // 箭头：只有三角形 bbox 与可见范围相交时才写入；长线被裁掉
            // 不影响位于可见端的箭头。
            if let Some(rect) = visible {
                if !self.arrow_visible(edge, rect) {
                    continue;
                }
            }
            let (tx, ty) = match local(f64::from(edge.arrow_tip_x), f64::from(edge.arrow_tip_y)) {
                Some(point) => point,
                None => continue,
            };
            let (lx, ly) = match local(f64::from(edge.arrow_left_x), f64::from(edge.arrow_left_y)) {
                Some(point) => point,
                None => continue,
            };
            let (rx, ry) = match local(f64::from(edge.arrow_right_x), f64::from(edge.arrow_right_y))
            {
                Some(point) => point,
                None => continue,
            };
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
        let tx = width / 2.0 + (self.anchor_x - origin_x) * zoom;
        let ty = height / 2.0 + (self.anchor_y - origin_y) * zoom;
        if tx.is_finite() && ty.is_finite() {
            (tx, ty)
        } else {
            (0.0, 0.0)
        }
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
        // 视口尺寸变了，极端模式的可见块键必须失效；普通 anchor 几何
        // 与视口无关，只更新 uniform 中的 viewport。
        if self.uploaded_geometry_mode == EdgeGeometryMode::Extreme {
            self.extreme_cache = None;
            self.mark_geometry_dirty();
        } else {
            self.request_paint();
        }
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

        let extreme = self.uses_extreme_camera();
        let geometry_dirty = self.geometry_dirty;
        let params = if extreme {
            self.extreme_geometry_params()
        } else {
            None
        };
        if geometry_dirty {
            if let Some(params) = params.as_ref() {
                let rebuild = match &self.extreme_cache {
                    Some(cache) => cache.key != params.key,
                    None => true,
                };
                if rebuild {
                    let (normal, selected) = self.build_extreme_vertices(params);
                    self.extreme_cache = Some(ExtremeGeometryCache {
                        key: params.key.clone(),
                        origin_x: params.origin_x,
                        origin_y: params.origin_y,
                        normal,
                        selected,
                    });
                }
                self.uploaded_geometry_mode = EdgeGeometryMode::Extreme;
                self.uploaded_extreme_key = Some(params.key.clone());
                self.uploaded_anchor_position_bucket = None;
            } else {
                self.extreme_cache = None;
                let position_scale = self.zoom_level / self.render_zoom();
                let (normal, selected) = self.build_vertices_relative(
                    self.anchor_x,
                    self.anchor_y,
                    position_scale,
                    None,
                );
                self.cached_normal = normal;
                self.cached_selected = selected;
                self.uploaded_geometry_mode = EdgeGeometryMode::Anchor;
                self.uploaded_extreme_key = None;
                self.uploaded_anchor_position_bucket = Self::pow2_bucket(position_scale);
            }
        }
        let (width, height) = self.item_size();
        let zoom = if self.zoom_level > 0.0 && self.zoom_level.is_finite() {
            self.zoom_level
        } else {
            1.0
        };
        let render_zoom = self.render_zoom();
        let anchor_translation = self.camera_translation();
        let (normal, selected, tx, ty, transform_zoom) = if extreme {
            if let (Some(cache), Some(params)) = (&self.extreme_cache, &params) {
                let origin_world_x = (width / 2.0 - self.pan_x) / zoom;
                let origin_world_y = (height / 2.0 - self.pan_y) / zoom;
                let tx = width / 2.0 + (cache.origin_x - origin_world_x) * zoom;
                let ty = height / 2.0 + (cache.origin_y - origin_world_y) * zoom;
                // 顶点按 position_bucket 预缩放；矩阵再乘回真实 zoom 与
                // position_bucket 的比值。两者都在 float 安全范围内。
                let ratio = params.position_scale / params.position_bucket;
                let transform_zoom = render_zoom * ratio;
                if tx.is_finite() && ty.is_finite() && transform_zoom.is_finite() {
                    (
                        cache.normal.as_slice(),
                        cache.selected.as_slice(),
                        tx.clamp(-MAX_ANCHOR_SCREEN_OFFSET, MAX_ANCHOR_SCREEN_OFFSET),
                        ty.clamp(-MAX_ANCHOR_SCREEN_OFFSET, MAX_ANCHOR_SCREEN_OFFSET),
                        transform_zoom.clamp(1e-9, MAX_ANCHOR_SCREEN_OFFSET),
                    )
                } else {
                    (
                        self.cached_normal.as_slice(),
                        self.cached_selected.as_slice(),
                        anchor_translation.0,
                        anchor_translation.1,
                        render_zoom,
                    )
                }
            } else {
                (
                    self.cached_normal.as_slice(),
                    self.cached_selected.as_slice(),
                    anchor_translation.0,
                    anchor_translation.1,
                    render_zoom,
                )
            }
        } else {
            (
                self.cached_normal.as_slice(),
                self.cached_selected.as_slice(),
                anchor_translation.0,
                anchor_translation.1,
                render_zoom,
            )
        };
        // 顶点可能按 position_bucket 预缩放，QSG 矩阵必须乘回真实比例；
        // 不是 render_zoom 本身。普通模式两者相等。
        let qsg_matrix_zoom = transform_zoom;
        let normal_ptr = normal.as_ptr();
        let selected_ptr = selected.as_ptr();
        let normal_count = (normal.len() / 7) as i32;
        let selected_count = (selected.len() / 7) as i32;
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
                    f64::from(edge.start_x) * zoom + self.pan_x,
                    f64::from(edge.start_y) * zoom + self.pan_y,
                )
            })
            .unwrap_or((0.0, 0.0));
        let sample_end_screen = sample
            .map(|edge| {
                (
                    f64::from(edge.end_x) * zoom + self.pan_x,
                    f64::from(edge.end_y) * zoom + self.pan_y,
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
                render_zoom: qsg_matrix_zoom,
                extreme_camera: extreme,
                normal_width_px,
                selected_width_px,
                translate_x: tx,
                translate_y: ty,
                normal_color: format!("#{nr:02x}{ng:02x}{nb:02x}"),
                normal_alpha,
                selected_alpha,
                reveal,
                sample_edge_id: sample.map(|edge| edge.edge_id.clone()).unwrap_or_default(),
                sample_start_x: sample.map(|edge| f64::from(edge.start_x)).unwrap_or(0.0),
                sample_start_y: sample.map(|edge| f64::from(edge.start_y)).unwrap_or(0.0),
                sample_end_x: sample.map(|edge| f64::from(edge.end_x)).unwrap_or(0.0),
                sample_end_y: sample.map(|edge| f64::from(edge.end_y)).unwrap_or(0.0),
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
