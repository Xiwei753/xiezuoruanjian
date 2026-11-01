// =============================================================================
// native/starmap_grid_item.rs — 星图背景点阵场景图图层（Issue #1029）
// =============================================================================
//
// 层级：Linux_qt 平台可视组件（QML 原生类型 `Sujian.StarMapGridItem`）。
//
// 旧实现用 QML Canvas 画点阵，并在每次 pan/zoom 时 requestPaint：每帧都要
// CPU 生成一张与视口等大的 QImage，再作为纹理上传 GPU。这里改为一个全屏
// 四边形 + 自定义 QSGMaterialShader：点阵在 fragment shader 中按 spacing /
// phase uniform 计算。
//
// 评论修正：pan / zoom 只更新 matrix 与 uniform，几何始终只有 6 个顶点，
// 不随相机变化 allocate / memcpy 任何点阵三角形。LOD 仍按旧 Canvas 口径：
// 屏幕间距 >= 16px，world 间距按 5 倍档抬升；LOD 只改 uniform，也不重建几何。
// =============================================================================

use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::scenegraph::{ContainerNode, SGNode};
use std::os::raw::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

const GRID_VERTEX_SHADER: &[u8] = include_bytes!("shaders/starmap_grid.vert.qsb");
const GRID_FRAGMENT_SHADER: &[u8] = include_bytes!("shaders/starmap_grid.frag.qsb");

static GRID_GEOMETRY_UPDATES: AtomicU64 = AtomicU64::new(0);
static GRID_VERTEX_UPLOADS: AtomicU64 = AtomicU64::new(0);
static GRID_CAMERA_UPDATES: AtomicU64 = AtomicU64::new(0);

/// 诊断用渲染统计：只含计数，不含星图正文。
pub fn render_statistics_json() -> String {
    serde_json::json!({
        "gridGeometryUpdates": GRID_GEOMETRY_UPDATES.load(Ordering::Relaxed),
        "gridVertexUploads": GRID_VERTEX_UPLOADS.load(Ordering::Relaxed),
        "gridCameraUpdates": GRID_CAMERA_UPDATES.load(Ordering::Relaxed),
    })
    .to_string()
}

cpp! {{
    #include <qmetaobject_rust.hpp>
    #include <QtCore/QCoreApplication>
    #include <QtCore/QDir>
    #include <QtCore/QFile>
    #include <QtGui/QColor>
    #include <QtGui/QVector4D>
    #include <QtQuick/QQuickItem>
    #include <QtQuick/QSGGeometry>
    #include <QtQuick/QSGGeometryNode>
    #include <QtQuick/QSGMaterial>
    #include <QtQuick/QSGMaterialShader>
    #include <QtQuick/QSGNode>
    #include <QtQuick/QSGRendererInterface>
    #include <cstring>

    static QString g_sujian_grid_vertex_shader_path;
    static QString g_sujian_grid_fragment_shader_path;

    static bool sujian_write_grid_shader_file(const QString &path,
                                              const char *bytes, int length)
    {
        QFile file(path);
        if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
            qWarning("Sujian star map grid shader write failed: cannot open %s: %s",
                     qPrintable(path), qPrintable(file.errorString()));
            return false;
        }
        const qint64 written = file.write(bytes, length);
        if (written != length) {
            qWarning("Sujian star map grid shader write failed for %s: wrote %lld of %d bytes: %s",
                     qPrintable(path), static_cast<long long>(written), length,
                     qPrintable(file.errorString()));
            return false;
        }
        return true;
    }

    static void sujian_set_grid_shaders(const char *vertex, int vertex_len,
                                        const char *fragment, int fragment_len)
    {
        if (!g_sujian_grid_vertex_shader_path.isEmpty())
            return;
        const QString base = QDir::tempPath() + QStringLiteral("/sujian-starmap-")
                + QString::number(QCoreApplication::applicationPid());
        g_sujian_grid_vertex_shader_path = base + QStringLiteral("-grid.vert.qsb");
        g_sujian_grid_fragment_shader_path = base + QStringLiteral("-grid.frag.qsb");
        sujian_write_grid_shader_file(g_sujian_grid_vertex_shader_path, vertex, vertex_len);
        sujian_write_grid_shader_file(g_sujian_grid_fragment_shader_path, fragment, fragment_len);
    }

    class SujianGridMaterial : public QSGMaterial
    {
    public:
        SujianGridMaterial() { setFlag(QSGMaterial::Blending); }

        QColor color;
        float viewport_x = 1.0f;
        float viewport_y = 1.0f;
        float spacing = 16.0f;
        float phase_x = 0.0f;
        float phase_y = 0.0f;
        // 材质是原地更新的；不能依赖 old/new 指针做字段比较。
        bool uniform_dirty = false;

        QSGMaterialType *type() const override
        {
            static QSGMaterialType material_type;
            return &material_type;
        }

        int compare(const QSGMaterial *other) const override
        {
            const auto *rhs = static_cast<const SujianGridMaterial *>(other);
            if (color.rgba() != rhs->color.rgba())
                return color.rgba() < rhs->color.rgba() ? -1 : 1;
            if (viewport_x != rhs->viewport_x)
                return viewport_x < rhs->viewport_x ? -1 : 1;
            if (viewport_y != rhs->viewport_y)
                return viewport_y < rhs->viewport_y ? -1 : 1;
            if (spacing != rhs->spacing)
                return spacing < rhs->spacing ? -1 : 1;
            if (phase_x != rhs->phase_x)
                return phase_x < rhs->phase_x ? -1 : 1;
            if (phase_y != rhs->phase_y)
                return phase_y < rhs->phase_y ? -1 : 1;
            return 0;
        }

        QSGMaterialShader *createShader(QSGRendererInterface::RenderMode) const override;
    };

    class SujianGridShader : public QSGMaterialShader
    {
    public:
        SujianGridShader()
        {
            setShaderFileName(QSGMaterialShader::VertexStage,
                              g_sujian_grid_vertex_shader_path);
            setShaderFileName(QSGMaterialShader::FragmentStage,
                              g_sujian_grid_fragment_shader_path);
        }

        bool updateUniformData(RenderState &state, QSGMaterial *new_material,
                               QSGMaterial *old_material) override
        {
            QByteArray *buffer = state.uniformData();
            if (buffer->size() < 128)
                buffer->resize(128);
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

            auto *material = static_cast<SujianGridMaterial *>(new_material);
            if (old_material != new_material || material->uniform_dirty) {
                const float color[4] = { material->color.redF(),
                                         material->color.greenF(),
                                         material->color.blueF(),
                                         material->color.alphaF() };
                std::memcpy(data + 80, color, sizeof(color));
                const float viewport[2] = { material->viewport_x, material->viewport_y };
                std::memcpy(data + 96, viewport, sizeof(viewport));
                std::memcpy(data + 104, &material->spacing, sizeof(float));
                const float phase[2] = { material->phase_x, material->phase_y };
                std::memcpy(data + 112, phase, sizeof(phase));
                material->uniform_dirty = false;
                changed = true;
            }
            return changed;
        }
    };

    QSGMaterialShader *SujianGridMaterial::createShader(QSGRendererInterface::RenderMode) const
    {
        return new SujianGridShader;
    }

    static const float sujian_grid_vertices[] = {
        0.0f, 0.0f, 1.0f, 0.0f, 0.0f, 1.0f,
        0.0f, 1.0f, 1.0f, 0.0f, 1.0f, 1.0f,
    };

    static SujianGridMaterial *sujian_grid_material(QSGGeometryNode *node)
    {
        QSGMaterial *existing = node->material();
        if (existing && existing->type() == SujianGridMaterial().type())
            return static_cast<SujianGridMaterial *>(existing);
        auto *material = new SujianGridMaterial;
        node->setMaterial(material);
        node->setFlag(QSGNode::OwnsMaterial, true);
        return material;
    }

    static void sujian_starmap_grid_update(QSGNode *root,
                                           bool geometry_dirty,
                                           double viewport_x, double viewport_y,
                                           double spacing, double phase_x, double phase_y,
                                           int r, int g, int b, double alpha)
    {
        QSGGeometryNode *node = nullptr;
        QSGNode *child = root ? root->firstChild() : nullptr;
        if (child && child->type() == QSGNode::GeometryNodeType)
            node = static_cast<QSGGeometryNode *>(child);
        if (!node) {
            node = new QSGGeometryNode;
            node->setFlag(QSGNode::OwnedByParent, true);
            root->appendChildNode(node);
        }

        if (geometry_dirty || !node->geometry()) {
            QSGGeometry *geometry = node->geometry();
            if (!geometry) {
                geometry = new QSGGeometry(QSGGeometry::defaultAttributes_Point2D(), 6);
                geometry->setDrawingMode(QSGGeometry::DrawTriangles);
                node->setGeometry(geometry);
                node->setFlag(QSGNode::OwnsGeometry, true);
            } else {
                geometry->allocate(6);
            }
            std::memcpy(geometry->vertexData(), sujian_grid_vertices,
                        6 * 2 * sizeof(float));
            geometry->markVertexDataDirty();
            node->markDirty(QSGNode::DirtyGeometry);
        }

        SujianGridMaterial *material = sujian_grid_material(node);
        QColor color(r, g, b);
        color.setAlphaF(alpha);
        const bool uniforms_changed =
            material->color.rgba() != color.rgba()
            || material->viewport_x != float(viewport_x)
            || material->viewport_y != float(viewport_y)
            || material->spacing != float(spacing)
            || material->phase_x != float(phase_x)
            || material->phase_y != float(phase_y);
        if (uniforms_changed) {
            material->color = color;
            material->viewport_x = float(viewport_x);
            material->viewport_y = float(viewport_y);
            material->spacing = float(spacing);
            material->phase_x = float(phase_x);
            material->phase_y = float(phase_y);
            material->uniform_dirty = true;
            node->markDirty(QSGNode::DirtyMaterial);
        }
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

    geometry_dirty: bool,
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
        // 只改 spacing uniform；点阵几何不重建。
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

    fn spacing(&self) -> Option<f64> {
        let zoom = self.zoom_level;
        if !(zoom > 0.0) || !zoom.is_finite() {
            return None;
        }
        let mut world_spacing = 50.0_f64;
        let mut spacing = world_spacing * zoom;
        while spacing < 16.0 {
            world_spacing *= 5.0;
            spacing = world_spacing * zoom;
            if !spacing.is_finite() {
                return None;
            }
        }
        (spacing > 0.0).then_some(spacing)
    }

    fn camera_phase(&self, spacing: f64) -> (f64, f64) {
        if !(spacing > 0.0) || !spacing.is_finite() {
            return (0.0, 0.0);
        }
        let phase = |value: f64| {
            if value.is_finite() {
                value - (value / spacing).floor() * spacing
            } else {
                0.0
            }
        };
        (phase(self.pan_x), phase(self.pan_y))
    }
}

impl QQuickItem for StarMapGridItem {
    fn component_complete(&mut self) {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return;
        }
        let vertex_ptr = GRID_VERTEX_SHADER.as_ptr();
        let vertex_len = GRID_VERTEX_SHADER.len() as i32;
        let fragment_ptr = GRID_FRAGMENT_SHADER.as_ptr();
        let fragment_len = GRID_FRAGMENT_SHADER.len() as i32;
        // SAFETY: obj 是有效 QQuickItem 指针；ItemHasContents 启用 updatePaintNode。
        // shader 字节是 'static，C++ 立即复制到 QByteArray。
        cpp!(unsafe [
            obj as "QQuickItem *",
            vertex_ptr as "const char *", vertex_len as "int",
            fragment_ptr as "const char *", fragment_len as "int"
        ] {
            if (obj)
                obj->setFlag(QQuickItem::ItemHasContents, true);
            sujian_set_grid_shaders(vertex_ptr, vertex_len, fragment_ptr, fragment_len);
        });
        self.geometry_dirty = true;
        self.request_paint();
    }

    fn geometry_changed(&mut self, _new_geometry: QRectF, _old_geometry: QRectF) {
        // 视口尺寸只改 uniform；全屏四边形仍是 0..1，不需要重建几何。
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
        let (width, height) = self.item_size();
        let spacing = self.spacing().unwrap_or(16.0);
        let (phase_x, phase_y) = self.camera_phase(spacing);
        let color = qmetaobject::QColor::from_name(&self.dot_color_hex.to_string());
        let alpha = if self.dot_opacity.is_finite() {
            self.dot_opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if geometry_dirty {
            GRID_GEOMETRY_UPDATES.fetch_add(1, Ordering::Relaxed);
            GRID_VERTEX_UPLOADS.fetch_add(6, Ordering::Relaxed);
        } else {
            GRID_CAMERA_UPDATES.fetch_add(1, Ordering::Relaxed);
        }
        let r = color.red();
        let g = color.green();
        let b = color.blue();
        // SAFETY: raw 是本 item 渲染线程上的 paint node；C++ 不保留 Rust 指针。
        cpp!(unsafe [
            raw as "QSGNode *",
            geometry_dirty as "bool",
            width as "double", height as "double",
            spacing as "double",
            phase_x as "double", phase_y as "double",
            r as "int", g as "int", b as "int", alpha as "double"
        ] {
            sujian_starmap_grid_update(raw, geometry_dirty,
                                       width, height, spacing, phase_x, phase_y,
                                       r, g, b, alpha);
        });
        self.geometry_dirty = false;
        // SAFETY: raw 是刚取回所有权的节点指针，交给 Qt 继续持有。
        unsafe { SGNode::from_raw(raw) }
    }
}
