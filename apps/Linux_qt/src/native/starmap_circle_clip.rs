// =============================================================================
// native/starmap_circle_clip.rs — 星图穿越过渡的平台侧圆形裁剪容器
// =============================================================================
//
// 层级：Linux_qt 平台可视组件（QML 原生类型 `Sujian.StarMapCircleClip`）。
//
// Issue #949 复核（评论 2026-10-18T12:12:29Z 问题 4）：过渡圆环不能只画边。
// 子场景 Loader 的**可见区域**必须裁到 `viewportMotion.ringCenterX/Y/ringRadius`
// 的圆内：圆外持续显示旧画面层，圆内才是实时矢量子图。外层 `Item.clip`
// 只能裁矩形视口，裁不出圆形。
//
// 实现路线（只用 Qt 公共 API，不依赖私有头文件）：
//   - 本组件是 QQuickItem：`component_complete()` 里 setClip(true)，Qt 自身
//     会为本 item 创建一个 QQuickDefaultClipNode 作为**子项容器**
//     （QQuickWindowPrivate::updateDirtyNodes 的子项节点都挂在这个裁剪节点下）；
//   - 渲染阶段（`update_paint_node()`）从本 item 自己的 paint node 向上取到
//     这个裁剪容器，把它的几何换成圆心 / 半径对应的圆（水平条带三角带，
//     与 Qt 自身裁剪节点同一拓扑；`setIsRectangular(false)` 让渲染器走
//     stencil 路径）；
//   - 关闭裁剪时把容器恢复成"整幅不裁剪"的矩形（`setIsRectangular(true)` +
//     超大 clipRect），几何留着下次复用；
//   - 只影响显示层：不改任何星图业务状态，过渡结束即停用，活动画布回到
//     现有 StarMapActiveScene 的直接矢量渲染。
//
// 为什么不在私有头上走 QQuickDefaultClipNode::setRect/setRadius：
//   本机 Qt 6.11.2 的 `QtQuick/private/qquickitem_p.h` 依赖未安装的
//   QtCore/QtGui 私有头（QtCore/private/qproperty_p.h 等在
//   qt6-qtbase-private-devel 里），include 即编译失败；改用 "setClip(true) +
//   公共 QSGClipNode 几何" 可以得到同一个圆形裁剪效果，且不引入私有依赖。
//
// 线程约束：场景图节点只能在渲染线程创建 / 修改；几何参数（QML 属性）在
// GUI 线程写入，同步阶段（GUI 线程被阻塞）在 `update_paint_node()` 里读取，
// 与 Qt 自身读取 clipRect 的时机一致。
// =============================================================================

use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::scenegraph::{ContainerNode, SGNode};
use std::os::raw::c_void;

cpp! {{
    #include <qmetaobject_rust.hpp>
    #include <QtQuick/QQuickItem>
    #include <QtQuick/QSGNode>
    #include <QtQuick/QSGGeometry>
    #include <QtQuick/QSGClipNode>
    #include <QtCore/QRectF>
    #include <QtMath>
    #include <cmath>

    // 圆的顶点行数：从圆顶到圆底按角度均匀取 2×kCircleRows+1 行，
    // 每行两个顶点（右 / 左），整圆因此是一个 122 边多边形，屏幕上看不出折线。
    // 用 DrawTriangleStrip 与 Qt 自身的裁剪节点（QQuickDefaultClipNode）同一
    // 拓扑，避免同一批次裁剪链里出现混合图元。
    static constexpr int kCircleRows = 60;

    // 本组件的 paint node 挂在 Qt 为本 item 创建的子项裁剪容器下
    //（setClip(true) 之后 childContainerNode() 就是那个 QSGClipNode）；
    // 只有父节点类型确实是裁剪节点才动它，避免在别的节点树上误裁剪。
    static QSGClipNode *sujian_starmap_clip_container(QSGNode *paint_node)
    {
        QSGNode *parent = paint_node ? paint_node->parent() : nullptr;
        if (!parent || parent->type() != QSGNode::ClipNodeType)
            return nullptr;
        return static_cast<QSGClipNode *>(parent);
    }

    // 按 clipEnabled / 圆心 / 半径同步裁剪容器，返回可复用的几何指针。
    // enabled=false 时把容器恢复成超大矩形（不裁剪），几何保留待复用。
    static size_t sujian_starmap_circle_clip_apply(QSGNode *paint_node,
                                                   size_t geometry, bool enabled,
                                                   double center_x, double center_y,
                                                   double radius)
    {
        QSGClipNode *clip = sujian_starmap_clip_container(paint_node);
        if (!clip)
            return geometry;

        if (!enabled || !(radius > 0.0) || !std::isfinite(radius)
                || !std::isfinite(center_x) || !std::isfinite(center_y)) {
            // 停用：矩形裁剪口径回到"不裁任何内容"（超大矩形与父级裁剪取交）。
            clip->setIsRectangular(true);
            clip->setClipRect(QRectF(-1.0e7, -1.0e7, 2.0e7, 2.0e7));
            clip->markDirty(QSGNode::DirtyGeometry);
            return geometry;
        }

        QSGGeometry *g = reinterpret_cast<QSGGeometry *>(geometry);
        if (g && clip->geometry() != g) {
            // 场景图失效后旧几何已随旧裁剪节点销毁：缓存指针不再指向本节点
            // 的几何，重新分配一个，避免悬挂指针。
            g = nullptr;
        }
        constexpr int vertexCount = 2 * (kCircleRows + 1);
        if (!g) {
            g = new QSGGeometry(QSGGeometry::defaultAttributes_Point2D(), vertexCount);
            g->setDrawingMode(QSGGeometry::DrawTriangleStrip);
        }
        g->allocate(vertexCount);
        QSGGeometry::Point2D *v = g->vertexDataAsPoint2D();
        for (int i = 0; i <= kCircleRows; ++i) {
            // 从圆顶（angle 0）扫到圆底（angle π）：每行给出右 / 左两个端点，
            // 相邻两行组成一个梯形，整条 strip 恰好铺满圆面。
            const double angle = M_PI * double(i) / double(kCircleRows);
            const double dy = -radius * std::cos(angle);
            const double half = radius * std::sin(angle);
            v[2 * i].set(float(center_x + half), float(center_y + dy));
            v[2 * i + 1].set(float(center_x - half), float(center_y + dy));
        }
        g->markVertexDataDirty();

        clip->setGeometry(g);
        clip->setFlag(QSGNode::OwnsGeometry, true);
        clip->setIsRectangular(false);
        clip->setClipRect(QRectF(center_x - radius, center_y - radius,
                                 radius * 2.0, radius * 2.0));
        clip->markDirty(QSGNode::DirtyGeometry);
        return size_t(g);
    }
}}

/// 圆形裁剪容器（QML 类型 `Sujian.StarMapCircleClip`）。
///
/// QML 侧只在星图穿越过渡期使用：包住"圆内那一层"的 Loader，并把
/// `clipCenterX/Y/clipRadius` 绑到 `StarMapViewportMotion` 的同一个 progress
/// 圆环几何上；`clipEnabled` 绑定 `activeSceneIsInsideRing` /
/// `outgoingSceneIsInsideRing`，过渡结束即停用。
#[derive(QObject, Default)]
pub struct StarMapCircleClip {
    base: qt_base_class!(trait QQuickItem),

    /// 是否启用圆形裁剪（false = 不裁剪，仍是 Qt 的矩形容器口径）。
    clip_enabled: qt_property!(bool; ALIAS clipEnabled WRITE set_clip_enabled),
    /// 圆心 / 半径（item 局部坐标 = 星图画布坐标）。
    clip_center_x: qt_property!(f64; ALIAS clipCenterX WRITE set_clip_center_x),
    clip_center_y: qt_property!(f64; ALIAS clipCenterY WRITE set_clip_center_y),
    clip_radius: qt_property!(f64; ALIAS clipRadius WRITE set_clip_radius),

    /// 渲染线程用的圆形几何指针（0 = 尚未分配）。只在本组件自己的
    /// `update_paint_node()` 里读写；几何随裁剪容器（OwnsGeometry）一起销毁。
    circle_geometry: usize,
}

impl StarMapCircleClip {
    /// 请求一次场景图同步：下一次同步阶段把最新参数应用到裁剪容器。
    fn request_paint(&mut self) {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return;
        }
        // SAFETY: obj 是本组件 QQuickItem 的 C++ 指针；仅调用 QQuickItem::update()
        // 安排内容脏标记，不解引用 Rust 侧任何状态。
        cpp!(unsafe [obj as "QQuickItem *"] {
            if (obj)
                obj->update();
        });
    }

    fn set_clip_enabled(&mut self, enabled: bool) {
        if self.clip_enabled == enabled {
            return;
        }
        self.clip_enabled = enabled;
        self.request_paint();
    }

    fn set_clip_center_x(&mut self, x: f64) {
        if self.clip_center_x == x {
            return;
        }
        self.clip_center_x = x;
        self.request_paint();
    }

    fn set_clip_center_y(&mut self, y: f64) {
        if self.clip_center_y == y {
            return;
        }
        self.clip_center_y = y;
        self.request_paint();
    }

    fn set_clip_radius(&mut self, radius: f64) {
        if self.clip_radius == radius {
            return;
        }
        self.clip_radius = radius;
        self.request_paint();
    }
}

impl QQuickItem for StarMapCircleClip {
    fn component_complete(&mut self) {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return;
        }
        // SAFETY: obj 是有效的 QQuickItem 指针。ItemHasContents 让
        // updatePaintNode 参与同步；setClip(true) 让 Qt 创建子项裁剪容器
        //（QQuickDefaultClipNode），本组件的圆形几何稍后替换它。
        cpp!(unsafe [obj as "QQuickItem *"] {
            if (obj) {
                obj->setFlag(QQuickItem::ItemHasContents, true);
                obj->setClip(true);
            }
        });
        self.request_paint();
    }

    fn geometry_changed(&mut self, _new_geometry: QRectF, _old_geometry: QRectF) {
        // 尺寸变化时 Qt 会把裁剪容器重置成整幅矩形；重新按当前圆参数应用一次。
        self.request_paint();
    }

    fn release_resources(&mut self) {
        // 场景图失效：圆形几何随旧裁剪节点一起销毁，清掉缓存指针，
        // 下一次 update_paint_node 重新查找容器 / 分配几何。
        self.circle_geometry = 0;
    }

    fn update_paint_node(&mut self, node: SGNode<ContainerNode>) -> SGNode<ContainerNode> {
        // 第一次调用时还没有 paint node：挂一个空占位节点，作为访问裁剪容器的
        // 句柄（paint node 的父节点就是 setClip(true) 创建的子项容器）。
        let mut raw = node.into_raw();
        if raw.is_null() {
            // SAFETY: 空场景图节点只作为本 item paint node 占位，由父节点回收。
            raw = cpp!(unsafe [] -> *mut c_void as "QSGNode *" {
                QSGNode *placeholder = new QSGNode;
                placeholder->setFlag(QSGNode::OwnedByParent);
                return placeholder;
            });
        }
        // SAFETY: raw 是本 item 在渲染线程上的 paint node；几何参数只在同步阶段
        // 读取（GUI 线程此时被阻塞）。
        let geometry_in = self.circle_geometry;
        let enabled = self.clip_enabled;
        let center_x = self.clip_center_x;
        let center_y = self.clip_center_y;
        let radius = self.clip_radius;
        let geometry = cpp!(unsafe [
            raw as "QSGNode *",
            geometry_in as "size_t",
            enabled as "bool",
            center_x as "double",
            center_y as "double",
            radius as "double"
        ] -> usize as "size_t" {
            return sujian_starmap_circle_clip_apply(raw, geometry_in, enabled,
                                                    center_x, center_y, radius);
        });
        self.circle_geometry = geometry;
        // SAFETY: raw 是刚取回所有权的节点指针，交给 Qt 继续持有。
        unsafe { SGNode::from_raw(raw) }
    }
}
