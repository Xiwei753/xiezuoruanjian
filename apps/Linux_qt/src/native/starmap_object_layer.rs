// =============================================================================
// native/starmap_object_layer.rs — 星图节点卡片 / 子图圆壳批量场景图图层
// =============================================================================
//
// Issue #1035：节点卡片和子图入口圆壳不再各自创建 QML 矩形 / clip / Loader
// 子树，也不再按视口交集改写 delegate 尺寸。这里把所有普通节点卡片与入口
// 圆壳合并成少量 QSGGeometryNode：
//   - 顶点保存相对稳定 world anchor 的完整对象矩形（f64 → f32 局部坐标）；
//   - 相机平移 / 缩放只更新一个 QSGTransformNode 矩阵和少量 uniform；
//   - 展开进度（圆点 → 完整卡片）只改 shader 的有效半宽 / 圆角，不重建顶点；
//   - 选择 / 编辑 / 拖动才重新分组或改写对应对象顶点。
// 文字仍由 QML 实时矢量排版，且整层共用一个 world anchor 相机变换。
// =============================================================================

use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::scenegraph::{ContainerNode, SGNode};
use std::os::raw::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Deserialize;

static OBJECT_GEOMETRY_UPDATES: AtomicU64 = AtomicU64::new(0);
static OBJECT_VERTEX_UPLOADS: AtomicU64 = AtomicU64::new(0);
static OBJECT_CAMERA_UPDATES: AtomicU64 = AtomicU64::new(0);
static OBJECT_TEXT_CREATED: AtomicU64 = AtomicU64::new(0);
static OBJECT_TEXT_DESTROYED: AtomicU64 = AtomicU64::new(0);
static OBJECT_ZOOM_FRAME_COUNT: AtomicU64 = AtomicU64::new(0);
static OBJECT_ZOOM_FRAME_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static OBJECT_ZOOM_FRAME_MAX_US: AtomicU64 = AtomicU64::new(0);

const FLOATS_PER_VERTEX: usize = 8;
const VERTICES_PER_OBJECT: usize = 6;

#[derive(Clone, Deserialize)]
struct NodeRect {
    id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Deserialize)]
struct EmbedRect {
    #[serde(rename = "instanceId")]
    id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Default)]
struct ObjectDiagnostics {
    node_count: usize,
    embed_count: usize,
    node_normal_vertex_count: i32,
    node_selected_vertex_count: i32,
    embed_normal_vertex_count: i32,
    embed_selected_vertex_count: i32,
    node_normal_geometry_present: bool,
    node_selected_geometry_present: bool,
    embed_normal_geometry_present: bool,
    embed_selected_geometry_present: bool,
    visible_text_count: i32,
    anchor_x: f64,
    anchor_y: f64,
    zoom: f64,
    translate_x: f64,
    translate_y: f64,
    reveal: f64,
    corner_radius: f64,
    sample_node_id: String,
    sample_node_x: f64,
    sample_node_y: f64,
    sample_node_width: f64,
    sample_node_height: f64,
    shader_resource_exists: bool,
}

static OBJECT_DIAGNOSTICS: Mutex<Option<ObjectDiagnostics>> = Mutex::new(None);

const OBJECT_VERTEX_QSB_SHADERS: Option<&str> = option_env!("SUJIAN_OBJECT_VERTEX_QSB_SHADERS");
const OBJECT_FRAGMENT_QSB_SHADERS: Option<&str> = option_env!("SUJIAN_OBJECT_FRAGMENT_QSB_SHADERS");
const OBJECT_VERTEX_QSB_STANDARD_GLSL: Option<&str> =
    option_env!("SUJIAN_OBJECT_VERTEX_QSB_STANDARD_GLSL");
const OBJECT_VERTEX_QSB_BATCHABLE_GLSL: Option<&str> =
    option_env!("SUJIAN_OBJECT_VERTEX_QSB_BATCHABLE_GLSL");
const OBJECT_FRAGMENT_QSB_STANDARD_GLSL: Option<&str> =
    option_env!("SUJIAN_OBJECT_FRAGMENT_QSB_STANDARD_GLSL");
const OBJECT_VERTEX_QSB_QTQUICK_READY: Option<&str> =
    option_env!("SUJIAN_OBJECT_VERTEX_QSB_QTQUICK_READY");
const OBJECT_FRAGMENT_QSB_QTQUICK_READY: Option<&str> =
    option_env!("SUJIAN_OBJECT_FRAGMENT_QSB_QTQUICK_READY");

fn split_metadata(value: Option<&str>, separator: char) -> Vec<&str> {
    value
        .unwrap_or_default()
        .split(separator)
        .filter(|item| !item.is_empty())
        .collect()
}

/// 诊断用渲染统计：只含计数与实际资源状态，不含星图正文。
pub fn render_statistics_json() -> String {
    let snapshot = OBJECT_DIAGNOSTICS
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_default();
    let vertex_shaders = split_metadata(OBJECT_VERTEX_QSB_SHADERS, ';');
    let fragment_shaders = split_metadata(OBJECT_FRAGMENT_QSB_SHADERS, ';');
    let vertex_standard = split_metadata(OBJECT_VERTEX_QSB_STANDARD_GLSL, ',');
    let vertex_batchable = split_metadata(OBJECT_VERTEX_QSB_BATCHABLE_GLSL, ',');
    let fragment_standard = split_metadata(OBJECT_FRAGMENT_QSB_STANDARD_GLSL, ',');
    let opengl_ready = OBJECT_VERTEX_QSB_QTQUICK_READY == Some("1")
        && OBJECT_FRAGMENT_QSB_QTQUICK_READY == Some("1");
    let zoom_frame_count = OBJECT_ZOOM_FRAME_COUNT.load(Ordering::Relaxed);
    let zoom_frame_total_ms = OBJECT_ZOOM_FRAME_TOTAL_US.load(Ordering::Relaxed) as f64 / 1000.0;
    let zoom_frame_max_ms = OBJECT_ZOOM_FRAME_MAX_US.load(Ordering::Relaxed) as f64 / 1000.0;
    let zoom_frame_average_ms = if zoom_frame_count == 0 {
        0.0
    } else {
        zoom_frame_total_ms / zoom_frame_count as f64
    };
    serde_json::json!({
        "objectGeometryUpdates": OBJECT_GEOMETRY_UPDATES.load(Ordering::Relaxed),
        "objectVertexUploads": OBJECT_VERTEX_UPLOADS.load(Ordering::Relaxed),
        "objectCameraUpdates": OBJECT_CAMERA_UPDATES.load(Ordering::Relaxed),
        "objectTextCreated": OBJECT_TEXT_CREATED.load(Ordering::Relaxed),
        "objectTextDestroyed": OBJECT_TEXT_DESTROYED.load(Ordering::Relaxed),
        "objectZoomFrameCount": zoom_frame_count,
        "objectZoomFrameTotalMs": zoom_frame_total_ms,
        "objectZoomFrameMaxMs": zoom_frame_max_ms,
        "objectZoomFrameAverageMs": zoom_frame_average_ms,
        "objectNodeCount": snapshot.node_count,
        "objectEmbedCount": snapshot.embed_count,
        "nodeNormalVertexCount": snapshot.node_normal_vertex_count,
        "nodeSelectedVertexCount": snapshot.node_selected_vertex_count,
        "embedNormalVertexCount": snapshot.embed_normal_vertex_count,
        "embedSelectedVertexCount": snapshot.embed_selected_vertex_count,
        "nodeNormalGeometryPresent": snapshot.node_normal_geometry_present,
        "nodeSelectedGeometryPresent": snapshot.node_selected_geometry_present,
        "embedNormalGeometryPresent": snapshot.embed_normal_geometry_present,
        "embedSelectedGeometryPresent": snapshot.embed_selected_geometry_present,
        "visibleTextInstanceCount": snapshot.visible_text_count,
        "shaderResourceExists": snapshot.shader_resource_exists,
        "shaderBuildToolValidated": !vertex_shaders.is_empty()
            && !fragment_shaders.is_empty(),
        "shaderVertexBuildShaders": vertex_shaders,
        "shaderFragmentBuildShaders": fragment_shaders,
        "shaderVertexStandardGlslVariants": vertex_standard,
        "shaderVertexBatchableGlslVariants": vertex_batchable,
        "shaderFragmentStandardGlslVariants": fragment_standard,
        "shaderQtQuickVariantsReady": opengl_ready,
        "shaderPipelineVerified": false,
        "anchor": [snapshot.anchor_x, snapshot.anchor_y],
        "zoom": snapshot.zoom,
        "cameraTranslation": [snapshot.translate_x, snapshot.translate_y],
        "reveal": snapshot.reveal,
        "cornerRadius": snapshot.corner_radius,
        "sampleNode": {
            "id": snapshot.sample_node_id,
            "x": snapshot.sample_node_x,
            "y": snapshot.sample_node_y,
            "width": snapshot.sample_node_width,
            "height": snapshot.sample_node_height,
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

    static const QString kSujianObjectVertexShader =
            QStringLiteral(":/shaders/starmap_object.vert.qsb");
    static const QString kSujianObjectFragmentShader =
            QStringLiteral(":/shaders/starmap_object.frag.qsb");

    class SujianObjectMaterial : public QSGMaterial
    {
    public:
        SujianObjectMaterial() { setFlag(QSGMaterial::Blending); }

        QColor fill;
        QColor border;
        float border_px = 1.0f;
        float viewport_x = 1.0f;
        float viewport_y = 1.0f;
        float zoom = 1.0f;
        float reveal = 1.0f;
        float dot_world_radius = 1.0f;

        QSGMaterialType *type() const override
        {
            static QSGMaterialType material_type;
            return &material_type;
        }

        int compare(const QSGMaterial *other) const override
        {
            const auto *rhs = static_cast<const SujianObjectMaterial *>(other);
            if (fill.rgba() != rhs->fill.rgba())
                return fill.rgba() < rhs->fill.rgba() ? -1 : 1;
            if (border.rgba() != rhs->border.rgba())
                return border.rgba() < rhs->border.rgba() ? -1 : 1;
            if (border_px != rhs->border_px)
                return border_px < rhs->border_px ? -1 : 1;
            if (viewport_x != rhs->viewport_x)
                return viewport_x < rhs->viewport_x ? -1 : 1;
            if (viewport_y != rhs->viewport_y)
                return viewport_y < rhs->viewport_y ? -1 : 1;
            if (zoom != rhs->zoom)
                return zoom < rhs->zoom ? -1 : 1;
            if (reveal != rhs->reveal)
                return reveal < rhs->reveal ? -1 : 1;
            if (dot_world_radius != rhs->dot_world_radius)
                return dot_world_radius < rhs->dot_world_radius ? -1 : 1;
            return 0;
        }

        QSGMaterialShader *createShader(QSGRendererInterface::RenderMode) const override;
    };

    class SujianObjectShader : public QSGMaterialShader
    {
    public:
        SujianObjectShader()
        {
            setShaderFileName(QSGMaterialShader::VertexStage, kSujianObjectVertexShader);
            setShaderFileName(QSGMaterialShader::FragmentStage, kSujianObjectFragmentShader);
        }

        bool updateUniformData(RenderState &state, QSGMaterial *new_material,
                               QSGMaterial *old_material) override
        {
            Q_UNUSED(old_material);
            QByteArray *buffer = state.uniformData();
            if (buffer->size() < 136)
                buffer->resize(136);
            char *data = buffer->data();
            if (state.isMatrixDirty())
                std::memcpy(data, state.combinedMatrix().constData(), 64);
            if (state.isOpacityDirty()) {
                const float opacity = state.opacity();
                std::memcpy(data + 64, &opacity, sizeof(float));
            }

            auto *material = static_cast<SujianObjectMaterial *>(new_material);
            const float fill[4] = { material->fill.redF(), material->fill.greenF(),
                                    material->fill.blueF(), material->fill.alphaF() };
            const float border[4] = { material->border.redF(),
                                      material->border.greenF(),
                                      material->border.blueF(),
                                      material->border.alphaF() };
            const float viewport[2] = { material->viewport_x, material->viewport_y };
            std::memcpy(data + 80, fill, sizeof(fill));
            std::memcpy(data + 96, border, sizeof(border));
            std::memcpy(data + 112, viewport, sizeof(viewport));
            std::memcpy(data + 120, &material->border_px, sizeof(float));
            std::memcpy(data + 124, &material->zoom, sizeof(float));
            std::memcpy(data + 128, &material->reveal, sizeof(float));
            std::memcpy(data + 132, &material->dot_world_radius, sizeof(float));
            return true;
        }
    };

    QSGMaterialShader *SujianObjectMaterial::createShader(QSGRendererInterface::RenderMode) const
    {
        return new SujianObjectShader;
    }

    static const QSGGeometry::Attribute sujian_object_attributes[] = {
        QSGGeometry::Attribute::create(0, 2, QSGGeometry::FloatType, true),
        QSGGeometry::Attribute::create(1, 2, QSGGeometry::FloatType),
        QSGGeometry::Attribute::create(2, 2, QSGGeometry::FloatType),
        QSGGeometry::Attribute::create(3, 1, QSGGeometry::FloatType),
        QSGGeometry::Attribute::create(4, 1, QSGGeometry::FloatType),
    };
    static const QSGGeometry::AttributeSet sujian_object_attribute_set = {
        5, 8 * int(sizeof(float)), sujian_object_attributes
    };

    static QSGTransformNode *sujian_object_transform_node(QSGNode *root)
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

    static QSGGeometryNode *sujian_object_geometry_node(QSGTransformNode *transform, int index)
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

    static QSGGeometryNode *sujian_object_ensure_geometry_node(QSGTransformNode *transform,
                                                              int index)
    {
        QSGGeometryNode *existing = sujian_object_geometry_node(transform, index);
        if (existing)
            return existing;
        auto *node = new QSGGeometryNode;
        node->setFlag(QSGNode::OwnedByParent, true);
        // 按索引补齐前面的空节点；本图层固定 4 个批次的顺序。
        while (transform->childCount() < index) {
            auto *placeholder = new QSGGeometryNode;
            placeholder->setFlag(QSGNode::OwnedByParent, true);
            transform->appendChildNode(placeholder);
        }
        transform->appendChildNode(node);
        return node;
    }

    static SujianObjectMaterial *sujian_object_material(QSGGeometryNode *node)
    {
        QSGMaterial *existing = node->material();
        if (existing && existing->type() == SujianObjectMaterial().type())
            return static_cast<SujianObjectMaterial *>(existing);
        auto *material = new SujianObjectMaterial;
        node->setMaterial(material);
        node->setFlag(QSGNode::OwnsMaterial, true);
        return material;
    }

    static void sujian_apply_object_material(QSGGeometryNode *node,
                                             int fr, int fg, int fb, double fa,
                                             int br, int bg, int bb, double ba,
                                             double border_px,
                                             double viewport_x, double viewport_y,
                                             double zoom, double reveal,
                                             double dot_world_radius)
    {
        SujianObjectMaterial *material = sujian_object_material(node);
        QColor fill(fr, fg, fb);
        fill.setAlphaF(fa);
        QColor border(br, bg, bb);
        border.setAlphaF(ba);
        const bool changed = material->fill.rgba() != fill.rgba()
            || material->border.rgba() != border.rgba()
            || material->border_px != float(border_px)
            || material->viewport_x != float(viewport_x)
            || material->viewport_y != float(viewport_y)
            || material->zoom != float(zoom)
            || material->reveal != float(reveal)
            || material->dot_world_radius != float(dot_world_radius);
        if (changed) {
            material->fill = fill;
            material->border = border;
            material->border_px = float(border_px);
            material->viewport_x = float(viewport_x);
            material->viewport_y = float(viewport_y);
            material->zoom = float(zoom);
            material->reveal = float(reveal);
            material->dot_world_radius = float(dot_world_radius);
            node->markDirty(QSGNode::DirtyMaterial);
        }
    }

    static void sujian_apply_object_geometry(QSGGeometryNode *node,
                                             const float *vertices, int vertex_count,
                                             int fr, int fg, int fb, double fa,
                                             int br, int bg, int bb, double ba,
                                             double border_px,
                                             double viewport_x, double viewport_y,
                                             double zoom, double reveal,
                                             double dot_world_radius)
    {
        QSGGeometry *geometry = node->geometry();
        if (!geometry) {
            geometry = new QSGGeometry(sujian_object_attribute_set, vertex_count);
            geometry->setDrawingMode(QSGGeometry::DrawTriangles);
            node->setGeometry(geometry);
            node->setFlag(QSGNode::OwnsGeometry, true);
        } else if (geometry->vertexCount() != vertex_count) {
            geometry->allocate(vertex_count);
        }
        if (vertex_count > 0 && vertices) {
            std::memcpy(geometry->vertexData(), vertices,
                        size_t(vertex_count) * size_t(sujian_object_attribute_set.stride));
        }
        geometry->markVertexDataDirty();
        node->markDirty(QSGNode::DirtyGeometry);
        sujian_apply_object_material(node, fr, fg, fb, fa, br, bg, bb, ba,
                                     border_px, viewport_x, viewport_y, zoom,
                                     reveal, dot_world_radius);
    }

    static qint64 sujian_object_shader_state_bits()
    {
        qint64 bits = 0;
        if (QFile::exists(kSujianObjectVertexShader))
            bits |= 1LL << 0;
        if (QFile::exists(kSujianObjectFragmentShader))
            bits |= 1LL << 1;
        return bits;
    }

    // 诊断位布局：4 个批次顶点数各 15 bit，随后 4 个 geometry 存在位。
    static qint64 sujian_object_debug_state(QSGNode *root)
    {
        QSGTransformNode *transform = nullptr;
        if (root && root->firstChild()
                && root->firstChild()->type() == QSGNode::TransformNodeType)
            transform = static_cast<QSGTransformNode *>(root->firstChild());
        qint64 state = 0;
        for (int i = 0; i < 4; ++i) {
            QSGGeometryNode *node = sujian_object_geometry_node(transform, i);
            const qint64 count = node && node->geometry()
                    ? qint64(node->geometry()->vertexCount()) : 0;
            state |= (count & 0x7fff) << (i * 15);
            if (node && node->geometry())
                state |= qint64(1) << (60 + i);
        }
        return state;
    }

    static void sujian_starmap_objects_update(
            QSGNode *root,
            double translate_x, double translate_y, double zoom,
            bool geometry_dirty,
            const float *node_normal, int node_normal_count,
            const float *node_selected, int node_selected_count,
            const float *embed_normal, int embed_normal_count,
            const float *embed_selected, int embed_selected_count,
            int node_fr, int node_fg, int node_fb, double node_fa,
            int embed_fr, int embed_fg, int embed_fb, double embed_fa,
            int selected_r, int selected_g, int selected_b, double selected_a,
            int node_border_r, int node_border_g, int node_border_b, double node_border_a,
            int embed_border_r, int embed_border_g, int embed_border_b,
            double embed_border_a,
            double node_border_px, double node_selected_border_px,
            double embed_border_px, double embed_selected_border_px,
            double viewport_x, double viewport_y,
            double reveal, double dot_world_radius)
    {
        QSGTransformNode *transform = sujian_object_transform_node(root);
        if (!transform)
            return;

        QMatrix4x4 matrix;
        matrix.translate(float(translate_x), float(translate_y));
        matrix.scale(float(zoom));
        transform->setMatrix(matrix);
        transform->markDirty(QSGNode::DirtyMatrix);

        QSGGeometryNode *batches[4] = {
            sujian_object_ensure_geometry_node(transform, 0),
            sujian_object_ensure_geometry_node(transform, 1),
            sujian_object_ensure_geometry_node(transform, 2),
            sujian_object_ensure_geometry_node(transform, 3),
        };
        const float *vertices[4] = {
            node_normal, node_selected, embed_normal, embed_selected
        };
        const int counts[4] = {
            node_normal_count, node_selected_count, embed_normal_count, embed_selected_count
        };
        const int fill_r[4] = { node_fr, node_fr, embed_fr, embed_fr };
        const int fill_g[4] = { node_fg, node_fg, embed_fg, embed_fg };
        const int fill_b[4] = { node_fb, node_fb, embed_fb, embed_fb };
        const double fill_a[4] = { node_fa, node_fa, embed_fa, embed_fa };
        const int border_r[4] = {
            node_border_r, selected_r, embed_border_r, selected_r
        };
        const int border_g[4] = {
            node_border_g, selected_g, embed_border_g, selected_g
        };
        const int border_b[4] = {
            node_border_b, selected_b, embed_border_b, selected_b
        };
        const double border_a[4] = {
            node_border_a, selected_a, embed_border_a, selected_a
        };
        const double border_px[4] = {
            node_border_px, node_selected_border_px,
            embed_border_px, embed_selected_border_px
        };

        for (int i = 0; i < 4; ++i) {
            QSGGeometryNode *node = batches[i];
            if (!node)
                continue;
            if (geometry_dirty || !node->geometry()) {
                sujian_apply_object_geometry(
                    node, vertices[i], counts[i],
                    fill_r[i], fill_g[i], fill_b[i], fill_a[i],
                    border_r[i], border_g[i], border_b[i], border_a[i],
                    border_px[i], viewport_x, viewport_y, zoom, reveal,
                    dot_world_radius);
            } else {
                sujian_apply_object_material(
                    node, fill_r[i], fill_g[i], fill_b[i], fill_a[i],
                    border_r[i], border_g[i], border_b[i], border_a[i],
                    border_px[i], viewport_x, viewport_y, zoom, reveal,
                    dot_world_radius);
            }
        }
    }
}}

/// 星图对象批量图层（QML 类型 `Sujian.StarMapObjectLayer`）。
#[derive(QObject, Default)]
pub struct StarMapObjectLayer {
    base: qt_base_class!(trait QQuickItem),

    nodes_json: qt_property!(QString; ALIAS nodesJson WRITE set_nodes_json),
    embeds_json: qt_property!(QString; ALIAS embedsJson WRITE set_embeds_json),
    selected_kind: qt_property!(QString; ALIAS selectedKind WRITE set_selected_kind),
    selected_id: qt_property!(QString; ALIAS selectedId WRITE set_selected_id),
    editing_kind: qt_property!(QString; ALIAS editingKind WRITE set_editing_kind),
    editing_id: qt_property!(QString; ALIAS editingId WRITE set_editing_id),
    editing_progress: qt_property!(f64; ALIAS editingProgress WRITE set_editing_progress),
    drag_kind: qt_property!(QString; ALIAS dragKind WRITE set_drag_kind),
    drag_id: qt_property!(QString; ALIAS dragId WRITE set_drag_id),
    drag_x: qt_property!(f64; ALIAS dragX WRITE set_drag_x),
    drag_y: qt_property!(f64; ALIAS dragY WRITE set_drag_y),
    zoom_level: qt_property!(f64; ALIAS zoomLevel WRITE set_zoom_level),
    pan_x: qt_property!(f64; ALIAS panX WRITE set_pan_x),
    pan_y: qt_property!(f64; ALIAS panY WRITE set_pan_y),
    anchor_x: qt_property!(f64; ALIAS anchorX WRITE set_anchor_x),
    anchor_y: qt_property!(f64; ALIAS anchorY WRITE set_anchor_y),
    reveal: qt_property!(f64; ALIAS reveal WRITE set_reveal),
    reveal_dot_radius_px: qt_property!(f64; ALIAS revealDotRadiusPx WRITE set_reveal_dot_radius_px),
    corner_radius: qt_property!(f64; ALIAS cornerRadius WRITE set_corner_radius),
    visible_text_count: qt_property!(i32; ALIAS visibleTextCount WRITE set_visible_text_count),
    node_fill_hex: qt_property!(QString; ALIAS nodeFillHex WRITE set_node_fill_hex),
    node_border_hex: qt_property!(QString; ALIAS nodeBorderHex WRITE set_node_border_hex),
    selected_border_hex: qt_property!(QString; ALIAS selectedBorderHex WRITE set_selected_border_hex),
    embed_fill_hex: qt_property!(QString; ALIAS embedFillHex WRITE set_embed_fill_hex),
    embed_border_hex: qt_property!(QString; ALIAS embedBorderHex WRITE set_embed_border_hex),
    node_border_px: qt_property!(f64; ALIAS nodeBorderPx WRITE set_node_border_px),
    node_selected_border_px: qt_property!(f64; ALIAS nodeSelectedBorderPx WRITE set_node_selected_border_px),
    embed_border_px: qt_property!(f64; ALIAS embedBorderPx WRITE set_embed_border_px),
    embed_selected_border_px: qt_property!(f64; ALIAS embedSelectedBorderPx WRITE set_embed_selected_border_px),

    note_text_created: qt_method!(fn(&mut self)),
    note_text_destroyed: qt_method!(fn(&mut self)),
    note_zoom_frame: qt_method!(fn(&mut self, duration_ms: f64)),

    nodes: Vec<NodeRect>,
    embeds: Vec<EmbedRect>,
    geometry_dirty: bool,
    cached_node_normal: Vec<f32>,
    cached_node_selected: Vec<f32>,
    cached_embed_normal: Vec<f32>,
    cached_embed_selected: Vec<f32>,
}

impl StarMapObjectLayer {
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

    fn set_nodes_json(&mut self, value: QString) {
        if self.nodes_json == value {
            return;
        }
        self.nodes_json = value.clone();
        self.nodes = serde_json::from_str(&value.to_string()).unwrap_or_else(|err| {
            log::warn!("StarMapObjectLayer invalid nodes JSON: {err}");
            Vec::new()
        });
        self.mark_geometry_dirty();
    }

    fn set_embeds_json(&mut self, value: QString) {
        if self.embeds_json == value {
            return;
        }
        self.embeds_json = value.clone();
        self.embeds = serde_json::from_str(&value.to_string()).unwrap_or_else(|err| {
            log::warn!("StarMapObjectLayer invalid embeds JSON: {err}");
            Vec::new()
        });
        self.mark_geometry_dirty();
    }

    fn set_selected_kind(&mut self, value: QString) {
        if self.selected_kind == value {
            return;
        }
        self.selected_kind = value;
        self.mark_geometry_dirty();
    }

    fn set_selected_id(&mut self, value: QString) {
        if self.selected_id == value {
            return;
        }
        self.selected_id = value;
        self.mark_geometry_dirty();
    }

    fn set_editing_kind(&mut self, value: QString) {
        if self.editing_kind == value {
            return;
        }
        self.editing_kind = value;
        self.mark_geometry_dirty();
    }

    fn set_editing_id(&mut self, value: QString) {
        if self.editing_id == value {
            return;
        }
        self.editing_id = value;
        self.mark_geometry_dirty();
    }

    fn set_editing_progress(&mut self, value: f64) {
        if self.editing_progress == value {
            return;
        }
        self.editing_progress = value;
        self.mark_geometry_dirty();
    }

    fn set_drag_kind(&mut self, value: QString) {
        if self.drag_kind == value {
            return;
        }
        self.drag_kind = value;
        self.mark_geometry_dirty();
    }

    fn set_drag_id(&mut self, value: QString) {
        if self.drag_id == value {
            return;
        }
        self.drag_id = value;
        self.mark_geometry_dirty();
    }

    fn set_drag_x(&mut self, value: f64) {
        if self.drag_x == value {
            return;
        }
        self.drag_x = value;
        self.mark_geometry_dirty();
    }

    fn set_drag_y(&mut self, value: f64) {
        if self.drag_y == value {
            return;
        }
        self.drag_y = value;
        self.mark_geometry_dirty();
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

    fn set_reveal_dot_radius_px(&mut self, value: f64) {
        if self.reveal_dot_radius_px == value {
            return;
        }
        self.reveal_dot_radius_px = value;
        self.request_paint();
    }

    fn set_corner_radius(&mut self, value: f64) {
        if self.corner_radius == value {
            return;
        }
        self.corner_radius = value;
        self.mark_geometry_dirty();
    }

    fn set_visible_text_count(&mut self, value: i32) {
        self.visible_text_count = value.max(0);
    }

    fn set_node_fill_hex(&mut self, value: QString) {
        if self.node_fill_hex == value {
            return;
        }
        self.node_fill_hex = value;
        self.request_paint();
    }

    fn set_node_border_hex(&mut self, value: QString) {
        if self.node_border_hex == value {
            return;
        }
        self.node_border_hex = value;
        self.request_paint();
    }

    fn set_selected_border_hex(&mut self, value: QString) {
        if self.selected_border_hex == value {
            return;
        }
        self.selected_border_hex = value;
        self.request_paint();
    }

    fn set_embed_fill_hex(&mut self, value: QString) {
        if self.embed_fill_hex == value {
            return;
        }
        self.embed_fill_hex = value;
        self.request_paint();
    }

    fn set_embed_border_hex(&mut self, value: QString) {
        if self.embed_border_hex == value {
            return;
        }
        self.embed_border_hex = value;
        self.request_paint();
    }

    fn set_node_border_px(&mut self, value: f64) {
        if self.node_border_px == value {
            return;
        }
        self.node_border_px = value;
        self.request_paint();
    }

    fn set_node_selected_border_px(&mut self, value: f64) {
        if self.node_selected_border_px == value {
            return;
        }
        self.node_selected_border_px = value;
        self.request_paint();
    }

    fn set_embed_border_px(&mut self, value: f64) {
        if self.embed_border_px == value {
            return;
        }
        self.embed_border_px = value;
        self.request_paint();
    }

    fn set_embed_selected_border_px(&mut self, value: f64) {
        if self.embed_selected_border_px == value {
            return;
        }
        self.embed_selected_border_px = value;
        self.request_paint();
    }

    fn note_text_created(&mut self) {
        OBJECT_TEXT_CREATED.fetch_add(1, Ordering::Relaxed);
    }

    fn note_text_destroyed(&mut self) {
        OBJECT_TEXT_DESTROYED.fetch_add(1, Ordering::Relaxed);
    }

    fn note_zoom_frame(&mut self, duration_ms: f64) {
        if !duration_ms.is_finite() || duration_ms < 0.0 {
            return;
        }
        let micros = (duration_ms * 1000.0).round() as u64;
        OBJECT_ZOOM_FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
        OBJECT_ZOOM_FRAME_TOTAL_US.fetch_add(micros, Ordering::Relaxed);
        OBJECT_ZOOM_FRAME_MAX_US.fetch_max(micros, Ordering::Relaxed);
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

    fn sanitized_px(value: f64, fallback: f64) -> f64 {
        if value.is_finite() {
            value.clamp(0.25, 8.0)
        } else {
            fallback
        }
    }

    fn sanitized_radius(value: f64) -> f64 {
        if value.is_finite() {
            value.clamp(0.0, 4096.0)
        } else {
            6.0
        }
    }

    fn editing_alpha(&self, kind: &str, id: &str) -> f32 {
        let editing_kind = self.editing_kind.to_string();
        let editing_id = self.editing_id.to_string();
        if editing_kind == kind && editing_id == id {
            let progress = if self.editing_progress.is_finite() {
                self.editing_progress.clamp(0.0, 1.0)
            } else {
                0.0
            };
            (1.0 - progress) as f32
        } else {
            1.0
        }
    }

    fn is_selected(&self, kind: &str, id: &str) -> bool {
        let selected_kind = self.selected_kind.to_string();
        let selected_id = self.selected_id.to_string();
        (selected_kind == kind && selected_id == id)
            || (self.editing_kind.to_string() == kind && self.editing_id.to_string() == id)
    }

    fn dragged_position(&self, kind: &str, id: &str, x: f64, y: f64) -> (f64, f64) {
        if self.drag_kind.to_string() == kind && self.drag_id.to_string() == id {
            (self.drag_x, self.drag_y)
        } else {
            (x, y)
        }
    }

    /// 顶点只保存相对稳定 anchor 的完整 world 矩形；相机变化不重建这里。
    fn build_anchor_vertices(&self) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>) {
        let anchor_x = self.anchor_x;
        let anchor_y = self.anchor_y;
        if !anchor_x.is_finite() || !anchor_y.is_finite() {
            return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        }
        let corner = Self::sanitized_radius(self.corner_radius);
        let node_count = self.nodes.len();
        let embed_count = self.embeds.len();
        let mut node_normal = Vec::with_capacity(node_count * 48);
        let mut node_selected = Vec::with_capacity(48);
        let mut embed_normal = Vec::with_capacity(embed_count * 48);
        let mut embed_selected = Vec::with_capacity(48);

        for node in &self.nodes {
            let (x, y) = self.dragged_position("node", &node.id, node.x, node.y);
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            let selected = self.is_selected("node", &node.id);
            let alpha = self.editing_alpha("node", &node.id);
            let target = if selected {
                &mut node_selected
            } else {
                &mut node_normal
            };
            Self::push_rounded_rect(
                target,
                x,
                y,
                node.width,
                node.height,
                anchor_x,
                anchor_y,
                corner,
                alpha,
                false,
            );
        }

        for embed in &self.embeds {
            let (x, y) = self.dragged_position("embed", &embed.id, embed.x, embed.y);
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            let selected = self.is_selected("embed", &embed.id);
            let alpha = self.editing_alpha("embed", &embed.id);
            let target = if selected {
                &mut embed_selected
            } else {
                &mut embed_normal
            };
            Self::push_rounded_rect(
                target,
                x,
                y,
                embed.width,
                embed.height,
                anchor_x,
                anchor_y,
                corner,
                alpha,
                true,
            );
        }

        (node_normal, node_selected, embed_normal, embed_selected)
    }

    fn push_rounded_rect(
        target: &mut Vec<f32>,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        anchor_x: f64,
        anchor_y: f64,
        corner: f64,
        alpha: f32,
        circle: bool,
    ) {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return;
        }
        let half_w = width * 0.5;
        let half_h = height * 0.5;
        let center_x = x + half_w - anchor_x;
        let center_y = y + half_h - anchor_y;
        if !center_x.is_finite() || !center_y.is_finite() {
            return;
        }
        let radius = if circle {
            half_w.min(half_h)
        } else {
            corner.min(half_w.min(half_h))
        } as f32;
        let (cx, cy) = (center_x as f32, center_y as f32);
        let (hw, hh) = (half_w as f32, half_h as f32);
        if !cx.is_finite() || !cy.is_finite() || !hw.is_finite() || !hh.is_finite() {
            return;
        }
        // 两个三角形；aCorner 是对象局部归一化角点，shader 据此算 SDF。
        for (corner_x, corner_y) in [
            (-1.0f32, -1.0f32),
            (1.0, -1.0),
            (-1.0, 1.0),
            (1.0, -1.0),
            (1.0, 1.0),
            (-1.0, 1.0),
        ] {
            target.extend_from_slice(&[cx, cy, hw, hh, corner_x, corner_y, radius, alpha]);
        }
    }
}

impl QQuickItem for StarMapObjectLayer {
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

        let (width, height) = self.item_size();
        let zoom = if self.zoom_level.is_finite() && self.zoom_level > 0.0 {
            self.zoom_level
        } else {
            1.0
        };
        let geometry_dirty = self.geometry_dirty;
        if geometry_dirty {
            let (node_normal, node_selected, embed_normal, embed_selected) =
                self.build_anchor_vertices();
            self.cached_node_normal = node_normal;
            self.cached_node_selected = node_selected;
            self.cached_embed_normal = embed_normal;
            self.cached_embed_selected = embed_selected;
        }
        let node_normal_count = (self.cached_node_normal.len() / FLOATS_PER_VERTEX) as i32;
        let node_selected_count = (self.cached_node_selected.len() / FLOATS_PER_VERTEX) as i32;
        let embed_normal_count = (self.cached_embed_normal.len() / FLOATS_PER_VERTEX) as i32;
        let embed_selected_count = (self.cached_embed_selected.len() / FLOATS_PER_VERTEX) as i32;

        let (nfr, nfg, nfb) = Self::color_parts(&self.node_fill_hex.to_string());
        let (efr, efg, efb) = Self::color_parts(&self.embed_fill_hex.to_string());
        let (sr, sg, sb) = Self::color_parts(&self.selected_border_hex.to_string());
        let (nbr, nbg, nbb) = Self::color_parts(&self.node_border_hex.to_string());
        let (ebr, ebg, ebb) = Self::color_parts(&self.embed_border_hex.to_string());
        let node_border_px = Self::sanitized_px(self.node_border_px, 1.0);
        let node_selected_border_px = Self::sanitized_px(self.node_selected_border_px, 2.0);
        let embed_border_px = Self::sanitized_px(self.embed_border_px, 2.0);
        let embed_selected_border_px = Self::sanitized_px(self.embed_selected_border_px, 3.0);
        let reveal = if self.reveal.is_finite() {
            self.reveal.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let dot_px = if self.reveal_dot_radius_px.is_finite() {
            self.reveal_dot_radius_px.max(0.0)
        } else {
            4.0
        };
        let dot_world_radius = dot_px / zoom;
        let (tx, ty) = if self.anchor_x.is_finite()
            && self.anchor_y.is_finite()
            && self.pan_x.is_finite()
            && self.pan_y.is_finite()
        {
            (
                self.pan_x + self.anchor_x * zoom,
                self.pan_y + self.anchor_y * zoom,
            )
        } else {
            (0.0, 0.0)
        };

        if geometry_dirty {
            OBJECT_GEOMETRY_UPDATES.fetch_add(1, Ordering::Relaxed);
            OBJECT_VERTEX_UPLOADS.fetch_add(
                (node_normal_count.max(0)
                    + node_selected_count.max(0)
                    + embed_normal_count.max(0)
                    + embed_selected_count.max(0)) as u64,
                Ordering::Relaxed,
            );
        } else {
            OBJECT_CAMERA_UPDATES.fetch_add(1, Ordering::Relaxed);
        }

        let node_normal_ptr = self.cached_node_normal.as_ptr();
        let node_selected_ptr = self.cached_node_selected.as_ptr();
        let embed_normal_ptr = self.cached_embed_normal.as_ptr();
        let embed_selected_ptr = self.cached_embed_selected.as_ptr();
        let node_alpha = 1.0f64;
        let embed_alpha = 1.0f64;
        let selected_alpha = 1.0f64;
        let node_border_alpha = 1.0f64;
        let embed_border_alpha = 1.0f64;
        // SAFETY: raw 是本 item 渲染线程上的 paint node；四个顶点数组在调用期间
        // 保持有效，C++ 只在本函数内拷贝，不保留 Rust 指针。
        cpp!(unsafe [
            raw as "QSGNode *",
            tx as "double", ty as "double", zoom as "double",
            geometry_dirty as "bool",
            node_normal_ptr as "const float *", node_normal_count as "int",
            node_selected_ptr as "const float *", node_selected_count as "int",
            embed_normal_ptr as "const float *", embed_normal_count as "int",
            embed_selected_ptr as "const float *", embed_selected_count as "int",
            nfr as "int", nfg as "int", nfb as "int", node_alpha as "double",
            efr as "int", efg as "int", efb as "int", embed_alpha as "double",
            sr as "int", sg as "int", sb as "int", selected_alpha as "double",
            nbr as "int", nbg as "int", nbb as "int", node_border_alpha as "double",
            ebr as "int", ebg as "int", ebb as "int", embed_border_alpha as "double",
            node_border_px as "double", node_selected_border_px as "double",
            embed_border_px as "double", embed_selected_border_px as "double",
            width as "double", height as "double",
            reveal as "double", dot_world_radius as "double"
        ] {
            sujian_starmap_objects_update(
                raw, tx, ty, zoom, geometry_dirty,
                node_normal_ptr, node_normal_count,
                node_selected_ptr, node_selected_count,
                embed_normal_ptr, embed_normal_count,
                embed_selected_ptr, embed_selected_count,
                nfr, nfg, nfb, node_alpha,
                efr, efg, efb, embed_alpha,
                sr, sg, sb, selected_alpha,
                nbr, nbg, nbb, node_border_alpha,
                ebr, ebg, ebb, embed_border_alpha,
                node_border_px, node_selected_border_px,
                embed_border_px, embed_selected_border_px,
                width, height, reveal, dot_world_radius);
        });

        // SAFETY: raw 仍是本 item 的 paint node；只读几何状态用于诊断。
        let debug_state = cpp!(unsafe [raw as "QSGNode *"] -> i64 as "qint64" {
            return sujian_object_debug_state(raw);
        });
        let shader_bits = cpp!(unsafe [] -> i64 as "qint64" {
            return sujian_object_shader_state_bits();
        });
        let sample = self.nodes.first();
        if let Ok(mut guard) = OBJECT_DIAGNOSTICS.lock() {
            *guard = Some(ObjectDiagnostics {
                node_count: self.nodes.len(),
                embed_count: self.embeds.len(),
                node_normal_vertex_count: node_normal_count,
                node_selected_vertex_count: node_selected_count,
                embed_normal_vertex_count: embed_normal_count,
                embed_selected_vertex_count: embed_selected_count,
                node_normal_geometry_present: (debug_state >> 60) & 1 != 0,
                node_selected_geometry_present: (debug_state >> 61) & 1 != 0,
                embed_normal_geometry_present: (debug_state >> 62) & 1 != 0,
                embed_selected_geometry_present: (debug_state >> 63) & 1 != 0,
                visible_text_count: self.visible_text_count,
                anchor_x: self.anchor_x,
                anchor_y: self.anchor_y,
                zoom,
                translate_x: tx,
                translate_y: ty,
                reveal,
                corner_radius: self.corner_radius,
                sample_node_id: sample.map(|node| node.id.clone()).unwrap_or_default(),
                sample_node_x: sample.map(|node| node.x).unwrap_or(0.0),
                sample_node_y: sample.map(|node| node.y).unwrap_or(0.0),
                sample_node_width: sample.map(|node| node.width).unwrap_or(0.0),
                sample_node_height: sample.map(|node| node.height).unwrap_or(0.0),
                shader_resource_exists: shader_bits & (1 << 0) != 0 && shader_bits & (1 << 1) != 0,
            });
        }
        self.geometry_dirty = false;
        // SAFETY: raw 是刚取回所有权的节点指针，交给 Qt 继续持有。
        unsafe { SGNode::from_raw(raw) }
    }
}
